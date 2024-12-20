# Luffy Media

Luffy Media is a media server that can receive and process media streams from various sources.

## Dependencies

- install gstreamer

```bash
export LIBRARY_PATH="/opt/homebrew/lib:$LIBRARY_PATH"
source ~/.zshrc
cargo build
```


## Development

### install gstreamer

### test gst

```bash
gst-launch-1.0 rtspsrc location=rtsp://192.168.20.198:8554/camera1 ! \
    rtph264depay ! \
    h264parse ! \
    avdec_h264 ! \
    videoconvert ! \
    autovideosink
```
gst-launch-1.0 rtspsrc location=rtsp://192.168.20.198:8554/camera1 latency=0 ! \
rtph264depay ! h264parse ! \
webrtcbin bundle-policy=max-bundle name=webrtcbin \
webrtcbin. ! fakesink