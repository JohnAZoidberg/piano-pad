use anyhow::{Context, Result};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const SAMPLE_RATE: u32 = 44100;
const CHANNELS: u16 = 2;

pub struct Audio {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    song_sink: Option<Sink>,
}

/// Decode audio to stereo f32 samples at 44100 Hz using ffmpeg.
fn decode_song(path: &Path) -> Result<Vec<f32>> {
    let output = Command::new("ffmpeg")
        .args([
            "-i",
            path.to_str().context("Non-UTF8 path")?,
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            &CHANNELS.to_string(),
            "-ar",
            &SAMPLE_RATE.to_string(),
            "-",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .context("Failed to run ffmpeg — is it installed?")?;

    if !output.status.success() {
        anyhow::bail!("ffmpeg failed to decode audio");
    }

    let bytes = &output.stdout;
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();

    Ok(samples)
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

    /// Start playing the song at the given speed, optionally seeking forward.
    pub fn play_song(&mut self, path: &Path, speed: f32, seek: Duration) -> Result<()> {
        let samples = decode_song(path)?;
        let source = SamplesBuffer::new(CHANNELS, SAMPLE_RATE, samples);
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
