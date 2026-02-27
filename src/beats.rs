use anyhow::{Context, Result};
use rand::Rng;
use rodio::source::Source;
use rodio::Decoder;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const COLS: usize = 4;
const FRAME_SIZE: usize = 1024;
const HOP_SIZE: usize = 512;
const THRESHOLD_WINDOW: usize = 15;
const THRESHOLD_MULTIPLIER: f32 = 1.5;
const THRESHOLD_FLOOR: f32 = 0.01;
const MIN_INTERVAL_SECS: f64 = 0.15;

#[derive(Debug, Clone)]
pub struct Beat {
    pub time: f64,
    pub col: usize,
}

pub fn detect_beats(path: &Path) -> Result<Vec<Beat>> {
    // Decode MP3 to mono f32 samples
    let file = File::open(path).context("Failed to open song file")?;
    let reader = BufReader::new(file);
    let decoder = Decoder::new(reader).context("Failed to decode audio file")?;

    let sample_rate = decoder.sample_rate();
    let channels = decoder.channels() as f32;

    // Collect all samples as mono f32
    let samples: Vec<f32> = if channels == 1.0 {
        decoder.map(|s| s as f32 / 32768.0).collect()
    } else {
        let raw: Vec<f32> = decoder.map(|s| s as f32 / 32768.0).collect();
        raw.chunks(channels as usize)
            .map(|chunk| chunk.iter().sum::<f32>() / channels)
            .collect()
    };

    // Compute frame energies
    let num_frames = if samples.len() >= FRAME_SIZE {
        (samples.len() - FRAME_SIZE) / HOP_SIZE + 1
    } else {
        0
    };

    let mut energies = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let start = i * HOP_SIZE;
        let end = start + FRAME_SIZE;
        let energy: f32 = samples[start..end].iter().map(|s| s * s).sum::<f32>() / FRAME_SIZE as f32;
        energies.push(energy);
    }

    // Onset strength: positive energy difference
    let mut onsets = vec![0.0f32; energies.len()];
    for i in 1..energies.len() {
        onsets[i] = (energies[i] - energies[i - 1]).max(0.0);
    }

    // Adaptive threshold and peak picking
    let min_interval_frames =
        (MIN_INTERVAL_SECS * sample_rate as f64 / HOP_SIZE as f64) as usize;

    let mut beat_times = Vec::new();
    let mut last_beat_frame: Option<usize> = None;

    for i in 0..onsets.len() {
        // Compute local mean over ±THRESHOLD_WINDOW frames
        let win_start = i.saturating_sub(THRESHOLD_WINDOW);
        let win_end = (i + THRESHOLD_WINDOW + 1).min(onsets.len());
        let local_mean: f32 =
            onsets[win_start..win_end].iter().sum::<f32>() / (win_end - win_start) as f32;

        let threshold = (local_mean * THRESHOLD_MULTIPLIER).max(THRESHOLD_FLOOR);

        if onsets[i] > threshold {
            // Check minimum interval
            if let Some(last) = last_beat_frame {
                if i - last < min_interval_frames {
                    continue;
                }
            }

            let time = (i * HOP_SIZE) as f64 / sample_rate as f64;
            beat_times.push(time);
            last_beat_frame = Some(i);
        }
    }

    // Assign columns with constraint: no more than 2 same column in a row
    let mut rng = rand::thread_rng();
    let mut beats = Vec::with_capacity(beat_times.len());
    let mut recent_cols: Vec<usize> = Vec::new();

    for time in beat_times {
        let col = loop {
            let c = rng.gen_range(0..COLS);
            // Check if last 2 columns are the same as this one
            if recent_cols.len() >= 2
                && recent_cols[recent_cols.len() - 1] == c
                && recent_cols[recent_cols.len() - 2] == c
            {
                continue;
            }
            break c;
        };
        recent_cols.push(col);
        beats.push(Beat { time, col });
    }

    Ok(beats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_three_same_columns_in_a_row() {
        // Create fake beat times
        let times: Vec<f64> = (0..100).map(|i| i as f64 * 0.5).collect();
        let mut rng = rand::thread_rng();
        let mut cols = Vec::new();

        for _ in &times {
            let col = loop {
                let c = rng.gen_range(0..COLS);
                if cols.len() >= 2
                    && cols[cols.len() - 1] == c
                    && cols[cols.len() - 2] == c
                {
                    continue;
                }
                break c;
            };
            cols.push(col);
        }

        // Verify no 3 consecutive same columns
        for window in cols.windows(3) {
            assert!(
                !(window[0] == window[1] && window[1] == window[2]),
                "Found 3 consecutive same columns"
            );
        }
    }
}
