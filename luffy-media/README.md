# Luffy Media

Luffy Media is a media server that can receive and process media streams from various sources.




## Development

### Stream Mac camera to rtsp by media mtx

1. brew install mediamtx
2. crate a config file on you ~/.mediamtx.yml with the following content:

```yaml
paths:
  camera1:
    source: publisher
```

3.  forwad you camera by ffmpeg:

```bash
ffmpeg -f avfoundation -framerate 30 -video_size 1280x720 -pix_fmt uyvy422 -i "0:none" \
  -vf format=nv12 \
  -c:v h264_videotoolbox \
  -b:v 2000k \
  -f rtsp \
  -rtsp_transport tcp \
  rtsp://localhost:8554/camera1
```
