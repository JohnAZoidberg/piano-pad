use anyhow::Result;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

pub struct Audio {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    song_sink: Option<Sink>,
}

impl Audio {
    pub fn new() -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().map_err(|e| anyhow::anyhow!("Audio init failed: {e}"))?;
        Ok(Self {
            _stream: stream,
            handle,
            song_sink: None,
        })
    }

    /// Start playing the song MP3 file at the given speed, optionally seeking forward.
    pub fn play_song(&mut self, path: &Path, speed: f32, seek: Duration) -> Result<()> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let source = Decoder::new(reader)?;
        let sink = Sink::try_new(&self.handle)?;
        sink.set_speed(speed);
        sink.append(source);
        if !seek.is_zero() {
            let _ = sink.try_seek(seek);
        }
        self.song_sink = Some(sink);
        Ok(())
    }

    /// Stop the currently playing song.
    pub fn stop_song(&mut self) {
        if let Some(sink) = self.song_sink.take() {
            sink.stop();
        }
    }
}
