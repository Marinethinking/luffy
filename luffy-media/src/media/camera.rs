use anyhow::{Context, Result};
use gstreamer::{self as gst, prelude::*};
use gstreamer::{glib, MessageView};
use gstreamer_sdp as gst_sdp;
use gstreamer_webrtc::{self};

use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::media::service::WebRTCResponse;

use crate::ws::WS_SERVER;

type WebRTCBin = gstreamer::Element;

#[derive(Debug)]
pub enum CameraError {
    InitFailed(String),
    PipelineError(String),
    WebRTCError(String),
}

#[derive(Clone, Debug)]
pub struct Camera {
    id: String,
    name: String,
    rtsp_url: String,
    pipeline: Arc<Mutex<Option<gst::Pipeline>>>,
    _bus_watch: Arc<Mutex<Option<gst::bus::BusWatchGuard>>>,
}

#[derive(Debug, Serialize)]
pub struct WebRTCIceCandidate {
    pub request_id: String,
    pub camera_id: String,
    pub candidate: String,
    pub sdp_mline_index: u32,
}

impl Camera {
    pub async fn new(config: crate::config::CameraConfig) -> Result<Self> {
        gst::init().context("Failed to initialize GStreamer")?;

        Ok(Self {
            id: config.id,
            name: config.name,
            rtsp_url: config.rtsp_url,
            pipeline: Arc::new(Mutex::new(None)),
            _bus_watch: Arc::new(Mutex::new(None)),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    async fn create_pipeline(&self) -> Result<gst::Pipeline> {
        let pipeline_str = format!(
            "rtspsrc location={} name=src latency=0 ! \
             rtph264depay ! \
             h264parse ! \
             rtph264pay config-interval=-1 ! \
             tee name=videotee allow-not-linked=true ! \
             queue ! \
             fakesink",
            self.rtsp_url
        );

        let pipeline = gst::parse::launch(&pipeline_str)
            .context("Failed to create pipeline")?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow::anyhow!("Failed to downcast pipeline"))?;

        self.setup_pipeline_monitoring(&pipeline).await?;

        Ok(pipeline)
    }

    async fn create_webrtc_peer(&self, request_id: &str) -> Result<WebRTCBin> {
        info!("Creating WebRTC peer for camera: {}", self.id);
        let pipeline = self.pipeline.lock().await;
        let pipeline = pipeline.as_ref().context("Pipeline not initialized")?;

        // Create webrtcbin element
        let webrtc = gst::ElementFactory::make("webrtcbin")
            .property_from_str("bundle-policy", "max-bundle")
            .property_from_str("stun-server", "stun://stun.l.google.com:19302")
            .name(format!("webrtc-{}", request_id))
            .build()
            .context("Failed to create webrtcbin")?;

        // Add webrtcbin to pipeline
        pipeline.add(&webrtc)?;

        // Get the tee element
        let tee = pipeline
            .by_name("videotee")
            .context("Failed to get tee element")?;

        // Request src pad from tee
        let tee_src_pad = tee
            .request_pad_simple("src_%u")
            .context("Failed to get tee src pad")?;
        info!("Created tee src pad: {}", tee_src_pad.name());

        // Get the static sink pad from webrtcbin
        let webrtc_sink_pad = webrtc
            .static_pad("sink_0")
            .or_else(|| webrtc.request_pad_simple("sink_%u"))
            .context("Failed to get webrtc sink pad")?;
        info!("Got webrtc sink pad: {}", webrtc_sink_pad.name());

        // Link the pads
        tee_src_pad
            .link(&webrtc_sink_pad)
            .context("Failed to link tee to webrtcbin")?;

        // Sync states
        webrtc.sync_state_with_parent()?;

        Ok(webrtc)
    }

    pub async fn start(&self) -> Result<()> {
        let pipeline = self.create_pipeline().await?;
        pipeline
            .set_state(gst::State::Playing)
            .context("Failed to set pipeline to Playing")?;
        *self.pipeline.lock().await = Some(pipeline);
        info!(camera_id = %self.id, "Camera started successfully");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        // Stop main pipeline
        if let Some(pipeline) = &*self.pipeline.lock().await {
            pipeline
                .set_state(gst::State::Null)
                .context("Failed to set pipeline to Null")?;
        }
        Ok(())
    }

    pub async fn handler_offer(&self, request_id: String, offer: String) -> Result<()> {
        info!("Handling offer for camera: {}", self.id);

        // Check if pipeline exists, if not start it
        if self.pipeline.lock().await.is_none() {
            info!("Pipeline not initialized, starting camera");
            self.start().await?;
        }

        let webrtc = self.create_webrtc_peer(&request_id).await?;

        // Parse and set remote description (offer)
        let sdp = gst_sdp::SDPMessage::parse_buffer(offer.as_bytes())
            .context("Failed to parse SDP offer")?;
        let offer = gstreamer_webrtc::WebRTCSessionDescription::new(
            gstreamer_webrtc::WebRTCSDPType::Offer,
            sdp,
        );

        // Set remote description and wait for it to complete
        let promise = gst::Promise::new();
        webrtc.emit_by_name::<()>("set-remote-description", &[&offer, &promise]);
        let reply = promise.wait();
        match reply {
            gst::PromiseResult::Replied => info!("Remote description set successfully"),
            _ => anyhow::bail!("Failed to set remote description: {:?}", reply),
        }

        // Now create answer
        let promise = gst::Promise::new();
        webrtc.emit_by_name::<()>("create-answer", &[&None::<gst::Structure>, &promise]);

        // Wait for answer and debug
        let reply = promise.wait();
        info!("Answer promise reply: {:?}", reply);

        let answer = match reply {
            gst::PromiseResult::Replied => {
                let reply = promise.get_reply().unwrap();
                info!("Promise reply: {:?}", reply);

                // Check for error in the reply
                if let Ok(error) = reply.get::<glib::Error>("error") {
                    anyhow::bail!("Error creating answer: {}", error);
                }

                // Try to get the answer
                match reply.get::<gstreamer_webrtc::WebRTCSessionDescription>("sdp") {
                    Ok(desc) => desc.sdp().as_text().unwrap(),
                    Err(_) => {
                        info!("Available fields: {:?}", reply.fields());
                        anyhow::bail!("Failed to get answer from promise reply")
                    }
                }
            }
            _ => anyhow::bail!("Error creating answer: {:?}", reply),
        };

        // Set local description
        let sdp = gst_sdp::SDPMessage::parse_buffer(answer.as_bytes())
            .context("Failed to parse SDP answer")?;
        let local_desc = gstreamer_webrtc::WebRTCSessionDescription::new(
            gstreamer_webrtc::WebRTCSDPType::Answer,
            sdp,
        );
        webrtc.emit_by_name::<()>(
            "set-local-description",
            &[&local_desc, &None::<gst::Promise>],
        );
        debug!("Answer: {:?}", answer);
        let ws_msg = WebRTCResponse::Answer {
            request_id: request_id.clone(),
            camera_id: self.id.clone(),
            answer,
        };
        WS_SERVER
            .send_message(&request_id, &serde_json::to_string(&ws_msg).unwrap())
            .await
    }

    pub async fn add_ice_candidate(
        &self,
        request_id: String,
        candidate: String,
        sdp_mline_index: u32,
    ) -> Result<()> {
        // Find webrtc element by name instead of using peers HashMap
        if let Some(pipeline) = &*self.pipeline.lock().await {
            if let Some(webrtc) = pipeline.by_name(&format!("webrtc-{}", request_id)) {
                webrtc.emit_by_name::<()>(
                    "add-ice-candidate",
                    &[&sdp_mline_index, &None::<String>, &candidate],
                );
            }
        }
        Ok(())
    }

    async fn setup_pipeline_monitoring(&self, pipeline: &gst::Pipeline) -> Result<()> {
        let bus = pipeline.bus().context("Failed to get pipeline bus")?;

        let mut bus_watch = self._bus_watch.lock().await;
        *bus_watch = Some(bus.add_watch(move |_, msg| {
            match msg.view() {
                gst::MessageView::Error(err) => {
                    error!(
                        "Pipeline error: {} ({})",
                        err.error(),
                        err.debug().unwrap_or_default()
                    );
                }
                gst::MessageView::StateChanged(state) => {
                    if let Some(element) = state.src() {
                        info!(
                            "Element {:?} state changed: {:?}",
                            element.name(),
                            state.current()
                        );
                    }
                }
                gst::MessageView::Eos(_) => {
                    debug!("End of stream");
                }
                _ => (),
            }
            glib::ControlFlow::Continue
        })?);

        Ok(())
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        if let Ok(mut pipeline) = self.pipeline.try_lock() {
            if let Some(p) = pipeline.take() {
                let _ = p.set_state(gst::State::Null);
            }
        }
    }
}
