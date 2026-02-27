# ffmpeg Notes

Useful ffmpeg commands for working with audio in this project.

## Decode MP3 to raw PCM (what the game does)

```sh
# Mono f32 little-endian at 44100 Hz — pipe to stdout
ffmpeg -i song.mp3 -f f32le -acodec pcm_f32le -ac 1 -ar 44100 -

# Same but write to a file
ffmpeg -i song.mp3 -f f32le -acodec pcm_f32le -ac 1 -ar 44100 output.raw
```

## Inspect a file

```sh
# Quick summary
ffprobe -hide_banner song.mp3

# Detailed format and stream info
ffprobe -show_format -show_streams song.mp3
```

## Convert between formats

```sh
# MP3 to WAV
ffmpeg -i song.mp3 song.wav

# Re-encode MP3 at 192kbps (useful if rodio can't decode the original)
ffmpeg -i song.mp3 -b:a 192k -y re-encoded.mp3

# Extract a section (e.g. 30s starting at 1:00)
ffmpeg -i song.mp3 -ss 60 -t 30 clip.mp3
```

## Why ffmpeg instead of rodio for decoding?

rodio uses symphonia as its MP3 decoder. Some valid MP3 files that ffmpeg/ffprobe
handle fine produce 0 samples from rodio's `Decoder` iterator. Using ffmpeg as a
subprocess for beat detection is more reliable. rodio is still used for playback
(via `Sink`) which works fine for the same files.
