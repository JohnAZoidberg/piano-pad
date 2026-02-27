use anyhow::Result;
use rodio::source::{SineWave, Source};
use rodio::{OutputStream, OutputStreamHandle, Sink};
use std::time::Duration;

/// Column tones: C major arpeggio
const COLUMN_FREQS: [f32; 4] = [
    261.63, // Col 0: C4
    329.63, // Col 1: E4
    392.00, // Col 2: G4
    523.25, // Col 3: C5
];

const GAME_OVER_FREQ: f32 = 200.0;
const TONE_DURATION_MS: u64 = 150;
const GAME_OVER_DURATION_MS: u64 = 500;

pub struct Audio {
    _stream: OutputStream,
    handle: OutputStreamHandle,
}

impl Audio {
    pub fn new() -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().map_err(|e| anyhow::anyhow!("Audio init failed: {e}"))?;
        Ok(Self {
            _stream: stream,
            handle,
        })
    }

    /// Play a short tone for the given column (fire-and-forget).
    pub fn play_column_tone(&self, col: usize) {
        if col >= COLUMN_FREQS.len() {
            return;
        }
        self.play_tone(COLUMN_FREQS[col], TONE_DURATION_MS);
    }

    /// Play a low game-over tone.
    pub fn play_game_over(&self) {
        self.play_tone(GAME_OVER_FREQ, GAME_OVER_DURATION_MS);
    }

    fn play_tone(&self, freq: f32, duration_ms: u64) {
        let Ok(sink) = Sink::try_new(&self.handle) else {
            return;
        };
        let source = SineWave::new(freq).take_duration(Duration::from_millis(duration_ms));
        sink.append(source);
        sink.detach();
    }
}
