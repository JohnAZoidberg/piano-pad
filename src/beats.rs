use anyhow::{Context, Result};
use rand::Rng;
use std::path::Path;
use std::process::Command;

const COLS: usize = 4;
const FRAME_SIZE: usize = 1024;
const HOP_SIZE: usize = 512;
const THRESHOLD_WINDOW: usize = 15;
const THRESHOLD_MULTIPLIER: f32 = 1.5;
const THRESHOLD_FLOOR: f32 = 0.01;
const MIN_INTERVAL_SECS: f64 = 0.15;
const SAMPLE_RATE: u32 = 44100;

#[derive(Debug, Clone)]
pub struct Beat {
    pub time: f64,
    pub col: usize,
}

/// Decode audio to mono f32 samples at 44100 Hz using ffmpeg.
fn decode_audio(path: &Path) -> Result<Vec<f32>> {
    let output = Command::new("ffmpeg")
        .args([
            "-i",
            path.to_str().context("Non-UTF8 path")?,
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            "1",
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
    if bytes.len() % 4 != 0 {
        anyhow::bail!("ffmpeg output has unexpected length");
    }

    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();

    Ok(samples)
}

/// Detect beat timestamps from raw mono samples at the given sample rate.
/// Returns a list of beat times in seconds.
fn find_beat_times(samples: &[f32], sample_rate: u32) -> Vec<f64> {
    // Compute frame energies
    let num_frames = if samples.len() >= FRAME_SIZE {
        (samples.len() - FRAME_SIZE) / HOP_SIZE + 1
    } else {
        return Vec::new();
    };

    let mut energies = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let start = i * HOP_SIZE;
        let end = start + FRAME_SIZE;
        let energy: f32 =
            samples[start..end].iter().map(|s| s * s).sum::<f32>() / FRAME_SIZE as f32;
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

    beat_times
}

/// Assign random columns to beat times, ensuring no more than 2 same columns in a row.
fn assign_columns(beat_times: Vec<f64>) -> Vec<Beat> {
    let mut rng = rand::thread_rng();
    let mut beats = Vec::with_capacity(beat_times.len());
    let mut recent_cols: Vec<usize> = Vec::new();

    for time in beat_times {
        let col = loop {
            let c = rng.gen_range(0..COLS);
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

    beats
}

pub fn detect_beats(path: &Path) -> Result<Vec<Beat>> {
    let samples = decode_audio(path)?;
    let beat_times = find_beat_times(&samples, SAMPLE_RATE);
    Ok(assign_columns(beat_times))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate silence of the given duration.
    fn silence(duration_secs: f64) -> Vec<f32> {
        vec![0.0; (SAMPLE_RATE as f64 * duration_secs) as usize]
    }

    /// Generate a constant-amplitude signal (no energy changes → no onsets).
    fn constant_tone(duration_secs: f64, amplitude: f32) -> Vec<f32> {
        let n = (SAMPLE_RATE as f64 * duration_secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                amplitude * (440.0 * 2.0 * std::f32::consts::PI * t).sin()
            })
            .collect()
    }

    /// Insert a short loud burst (impulse) into samples at the given time.
    fn insert_impulse(samples: &mut [f32], time_secs: f64, amplitude: f32) {
        let center = (time_secs * SAMPLE_RATE as f64) as usize;
        let half_len = FRAME_SIZE; // wide enough to span at least one full frame
        let start = center.saturating_sub(half_len);
        let end = (center + half_len).min(samples.len());
        for s in &mut samples[start..end] {
            *s = amplitude;
        }
    }

    #[test]
    fn test_silence_produces_no_beats() {
        let samples = silence(5.0);
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        assert!(beats.is_empty(), "Silence should produce no beats");
    }

    #[test]
    fn test_constant_tone_produces_no_beats() {
        // A constant sine wave has steady energy — no onsets
        let samples = constant_tone(5.0, 0.5);
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        assert!(
            beats.is_empty(),
            "Constant tone should produce no beats, got {}",
            beats.len()
        );
    }

    #[test]
    fn test_too_short_produces_no_beats() {
        // Fewer samples than one frame
        let samples = vec![0.0; FRAME_SIZE - 1];
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        assert!(beats.is_empty());
    }

    #[test]
    fn test_single_impulse_detected() {
        let mut samples = silence(3.0);
        insert_impulse(&mut samples, 1.5, 0.8);
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        assert!(!beats.is_empty(), "Should detect at least one beat");
        // The beat should be near t=1.5s (within a few frames of tolerance)
        let first = beats[0];
        assert!(
            (first - 1.5).abs() < 0.1,
            "Beat at {first:.3}s should be near 1.5s"
        );
    }

    #[test]
    fn test_multiple_impulses_detected() {
        let mut samples = silence(5.0);
        let times = [1.0, 2.0, 3.0, 4.0];
        for &t in &times {
            insert_impulse(&mut samples, t, 0.8);
        }
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        assert_eq!(
            beats.len(),
            times.len(),
            "Expected {} beats, got {}",
            times.len(),
            beats.len()
        );
        // Each detected beat should be close to the inserted time
        for (detected, &expected) in beats.iter().zip(&times) {
            assert!(
                (detected - expected).abs() < 0.1,
                "Beat at {detected:.3}s should be near {expected:.1}s"
            );
        }
    }

    #[test]
    fn test_min_interval_enforced() {
        // Two impulses closer than MIN_INTERVAL_SECS should merge into one
        let mut samples = silence(3.0);
        insert_impulse(&mut samples, 1.0, 0.8);
        insert_impulse(&mut samples, 1.05, 0.8); // 50ms apart, below 150ms threshold
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        assert_eq!(beats.len(), 1, "Close impulses should be deduplicated");
    }

    #[test]
    fn test_quiet_impulse_below_floor_ignored() {
        // An impulse whose energy change is below THRESHOLD_FLOOR should be ignored
        let mut samples = silence(3.0);
        // Very tiny amplitude — energy will be amplitude^2 ≈ 0.0001, well below floor
        insert_impulse(&mut samples, 1.5, 0.01);
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        assert!(
            beats.is_empty(),
            "Very quiet impulse should be below threshold floor"
        );
    }

    #[test]
    fn test_beats_are_sorted() {
        let mut samples = silence(6.0);
        insert_impulse(&mut samples, 1.0, 0.8);
        insert_impulse(&mut samples, 3.0, 0.8);
        insert_impulse(&mut samples, 5.0, 0.8);
        let beats = find_beat_times(&samples, SAMPLE_RATE);
        for window in beats.windows(2) {
            assert!(
                window[0] < window[1],
                "Beats should be in ascending order"
            );
        }
    }

    #[test]
    fn test_no_three_same_columns_in_a_row() {
        let times: Vec<f64> = (0..100).map(|i| i as f64 * 0.5).collect();
        let beats = assign_columns(times);
        for window in beats.windows(3) {
            assert!(
                !(window[0].col == window[1].col && window[1].col == window[2].col),
                "Found 3 consecutive same columns"
            );
        }
    }

    #[test]
    fn test_assign_columns_preserves_times() {
        let times: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let beats = assign_columns(times.clone());
        assert_eq!(beats.len(), times.len());
        for (beat, &expected) in beats.iter().zip(&times) {
            assert_eq!(beat.time, expected);
            assert!(beat.col < COLS);
        }
    }

    #[test]
    fn test_assign_columns_empty() {
        let beats = assign_columns(Vec::new());
        assert!(beats.is_empty());
    }

    #[test]
    fn test_ffmpeg_decodes_generated_wav() {
        // Generate a small WAV file with ffmpeg, then decode it back
        let test_path = "/tmp/piano-pad-test.wav";

        // Generate 1 second of 440Hz sine wave as WAV
        let gen = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-ar",
                "44100",
                "-ac",
                "1",
                test_path,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        // Skip test if ffmpeg not available
        let Ok(status) = gen else { return };
        if !status.success() {
            return;
        }

        let result = decode_audio(Path::new(test_path));
        let _ = std::fs::remove_file(test_path);

        let samples = result.expect("Should decode WAV file");
        // 1 second at 44100 Hz
        assert!(
            samples.len() >= 40000 && samples.len() <= 48000,
            "Expected ~44100 samples, got {}",
            samples.len()
        );
        // Should not be silence
        let max_abs = samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(max_abs > 0.1, "Decoded audio should not be silent");
    }
}
