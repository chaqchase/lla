# Media Inspector Plugin

Provides MIME and media metadata as listing fields. PNG, GIF, and JPEG dimensions have
built-in parsers. Optional system tools enrich the result: `file` for MIME detection,
`exiftool` for EXIF, `ffprobe` for audio/video duration and streams, and `sips` or
ImageMagick `identify` as image-dimension fallbacks.

```bash
lla --enable-plugin media_inspector --long ./media
lla plugin run media_inspector inspect -- ./media/video.mp4
lla plugin run media_inspector tools
```

Missing optional tools do not prevent the plugin from loading or decorating entries.
