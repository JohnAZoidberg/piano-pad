use anyhow::{Context, Result};
use std::f32::consts::PI;
use std::path::Path;
use std::process::Command;

const COLS: usize = 4;
const FRAME_SIZE: usize = 1024;
const HOP_SIZE: usize = 512;
const THRESHOLD_WINDOW: usize = 15;
const THRESHOLD_MULTIPLIER: f32 = 1.5;
const THRESHOLD_FLOOR: f32 = 0.005;
const MIN_INTERVAL_SECS: f64 = 0.15;
const SAMPLE_RATE: u32 = 44100;

/// BPM range for tempo estimation.
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;

/// Frequency band boundaries in FFT bins (for FRAME_SIZE=1024, SAMPLE_RATE=44100).
/// Band 0 (bass):     0–200 Hz   → bins 0..5
/// Band 1 (low-mid):  200–800 Hz → bins 5..19
/// Band 2 (mid-high): 800–4 kHz  → bins 19..93
/// Band 3 (high):     4 kHz+     → bins 93..513
const BAND_BINS: [usize; 5] = [0, 5, 19, 93, 513];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatMode {
    /// Columns correspond to pitch frequency bands (bass, low-mid, mid-high, high).
    Pitch,
    /// Columns correspond to metrical position (downbeat, offbeat, subdivision, syncopation).
    Rhythm,
}

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

/// Compute power spectrum |X[k]|² / N for a real-valued input.
/// Input length must be a power of 2. Returns N/2 + 1 bins.
fn power_spectrum(input: &[f32]) -> Vec<f32> {
    let n = input.len();
    debug_assert!(n.is_power_of_two());
    let bits = n.trailing_zeros();

    // Bit-reversal permutation into complex arrays
    let mut re = vec![0.0f32; n];
    let mut im = vec![0.0f32; n];
    for (i, &val) in input.iter().enumerate() {
        let j = (i as u32).reverse_bits() >> (32 - bits);
        re[j as usize] = val;
    }

    // Cooley-Tukey butterfly stages
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -2.0 * PI / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let w_re = (angle * k as f32).cos();
                let w_im = (angle * k as f32).sin();
                let a = start + k;
                let b = start + k + half;
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
            }
        }
        len *= 2;
    }

    // Power spectrum normalized by N
    let inv_n = 1.0 / n as f32;
    (0..=n / 2)
        .map(|k| (re[k] * re[k] + im[k] * im[k]) * inv_n)
        .collect()
}

/// Compute mean energy in each of the 4 frequency sub-bands for a single frame.
fn sub_band_energies(frame: &[f32]) -> [f32; COLS] {
    // Apply Hann window to reduce spectral leakage
    let n = frame.len();
    let windowed: Vec<f32> = frame
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
            s * w
        })
        .collect();
    let spectrum = power_spectrum(&windowed);
    let mut energies = [0.0f32; COLS];
    for band in 0..COLS {
        let start = BAND_BINS[band];
        let end = BAND_BINS[band + 1].min(spectrum.len());
        if end > start {
            energies[band] = spectrum[start..end].iter().sum::<f32>() / (end - start) as f32;
        }
    }
    energies
}

/// Detect onsets in a single energy band. Returns beat times in seconds.
fn detect_band_onsets(
    band_energies: &[f32],
    sample_rate: u32,
    min_interval_frames: usize,
) -> Vec<f64> {
    let mut onsets = vec![0.0f32; band_energies.len()];
    for i in 1..band_energies.len() {
        onsets[i] = (band_energies[i] - band_energies[i - 1]).max(0.0);
    }

    let mut beat_times = Vec::new();
    let mut last_beat_frame: Option<usize> = None;

    for i in 0..onsets.len() {
        let win_start = i.saturating_sub(THRESHOLD_WINDOW);
        let win_end = (i + THRESHOLD_WINDOW + 1).min(onsets.len());
        let local_mean: f32 =
            onsets[win_start..win_end].iter().sum::<f32>() / (win_end - win_start) as f32;
        let threshold = (local_mean * THRESHOLD_MULTIPLIER).max(THRESHOLD_FLOOR);

        if onsets[i] > threshold {
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

/// Detect beats with pitch frequency-based column assignment.
/// Each column corresponds to a frequency band: bass, low-mid, mid-high, high.
fn find_beats_pitch(samples: &[f32], sample_rate: u32) -> Vec<Beat> {
    let num_frames = if samples.len() >= FRAME_SIZE {
        (samples.len() - FRAME_SIZE) / HOP_SIZE + 1
    } else {
        return Vec::new();
    };

    // Compute sub-band energies per frame
    let mut band_energies: Vec<[f32; COLS]> = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let start = i * HOP_SIZE;
        let end = start + FRAME_SIZE;
        band_energies.push(sub_band_energies(&samples[start..end]));
    }

    let min_interval_frames = (MIN_INTERVAL_SECS * sample_rate as f64 / HOP_SIZE as f64) as usize;

    // Detect onsets independently per band
    let mut beats = Vec::new();
    for col in 0..COLS {
        let energies: Vec<f32> = band_energies.iter().map(|e| e[col]).collect();
        let times = detect_band_onsets(&energies, sample_rate, min_interval_frames);
        for time in times {
            beats.push(Beat { time, col });
        }
    }

    // Sort by time
    beats.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());

    // Apply global minimum interval to prevent overlapping tiles
    let mut filtered = Vec::new();
    let mut last_time = f64::NEG_INFINITY;
    for beat in beats {
        if beat.time - last_time >= MIN_INTERVAL_SECS {
            last_time = beat.time;
            filtered.push(beat);
        }
    }

    filtered
}

// ── Rhythm mode: metrical position-based column assignment ──

/// Compute broadband onset strength per frame. Returns (energies, onsets).
fn broadband_onsets(samples: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let num_frames = if samples.len() >= FRAME_SIZE {
        (samples.len() - FRAME_SIZE) / HOP_SIZE + 1
    } else {
        return (Vec::new(), Vec::new());
    };

    let mut energies = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let start = i * HOP_SIZE;
        let end = start + FRAME_SIZE;
        let energy: f32 =
            samples[start..end].iter().map(|s| s * s).sum::<f32>() / FRAME_SIZE as f32;
        energies.push(energy);
    }

    let mut onsets = vec![0.0f32; energies.len()];
    for i in 1..energies.len() {
        onsets[i] = (energies[i] - energies[i - 1]).max(0.0);
    }

    (energies, onsets)
}

/// Estimate tempo (beat period in frames) via autocorrelation of onset strength.
fn estimate_tempo(onsets: &[f32], sample_rate: u32) -> Option<f64> {
    let frames_per_sec = sample_rate as f64 / HOP_SIZE as f64;
    let min_lag = (frames_per_sec * 60.0 / MAX_BPM) as usize;
    let max_lag = (frames_per_sec * 60.0 / MIN_BPM) as usize;

    if onsets.len() < max_lag * 2 {
        return None;
    }

    let mut best_lag = min_lag;
    let mut best_corr = 0.0f64;

    for lag in min_lag..=max_lag {
        let corr: f64 = onsets
            .iter()
            .zip(onsets[lag..].iter())
            .map(|(&a, &b)| a as f64 * b as f64)
            .sum();
        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }

    Some(best_lag as f64)
}

/// Find the phase (offset in frames) that best aligns a grid with onset peaks.
fn estimate_phase(onsets: &[f32], period: f64) -> f64 {
    let period_frames = period.round() as usize;
    if period_frames == 0 {
        return 0.0;
    }

    let mut best_phase = 0;
    let mut best_score = 0.0f64;

    for phase in 0..period_frames {
        let mut score = 0.0f64;
        let mut pos = phase;
        while pos < onsets.len() {
            score += onsets[pos] as f64;
            pos += period_frames;
        }
        if score > best_score {
            best_score = score;
            best_phase = phase;
        }
    }

    best_phase as f64
}

/// Detect beats using rhythmic hierarchy analysis.
/// Estimates tempo, then scans grid-aligned positions at each subdivision level.
/// Columns represent hierarchy levels (coarser levels take priority):
///   0 = quarter note (beat), 1 = eighth note (offbeat),
///   2 = sixteenth note, 3 = syncopation (off-grid).
///
/// Beat times are grid-aligned so they stay perfectly in sync with the music.
fn find_beats_rhythm(samples: &[f32], sample_rate: u32) -> Vec<Beat> {
    let (_, onsets) = broadband_onsets(samples);
    if onsets.is_empty() {
        return Vec::new();
    }

    let period_frames = match estimate_tempo(&onsets, sample_rate) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let phase_frames = estimate_phase(&onsets, period_frames);
    let frames_per_sec = sample_rate as f64 / HOP_SIZE as f64;
    let period_secs = period_frames / frames_per_sec;
    let bpm = 60.0 / period_secs;
    let duration_frames = onsets.len();

    eprintln!("Tempo: {bpm:.1} BPM (period: {period_secs:.3}s)");

    // Adaptive onset threshold per frame (same method as detect_band_onsets)
    let thresholds: Vec<f32> = (0..duration_frames)
        .map(|i| {
            let win_start = i.saturating_sub(THRESHOLD_WINDOW);
            let win_end = (i + THRESHOLD_WINDOW + 1).min(duration_frames);
            let local_mean: f32 =
                onsets[win_start..win_end].iter().sum::<f32>() / (win_end - win_start) as f32;
            (local_mean * THRESHOLD_MULTIPLIER).max(THRESHOLD_FLOOR)
        })
        .collect();

    // Search tolerance: how far from a grid position to look for an onset.
    // 30% of a sixteenth note period, so onsets snap to the nearest grid position.
    let tolerance = (period_frames / 4.0 * 0.3).max(1.0) as usize;

    // Track which onset frames have been claimed by a coarser level
    let mut claimed = vec![false; duration_frames];
    let mut beats = Vec::new();

    // Each hierarchy level defines unique grid offsets within one beat period.
    // Quarter notes: offset 0  (the beat itself)
    // Eighth notes:  offset P/2  (halfway between beats)
    // Sixteenth notes: offsets P/4 and 3P/4  (between quarter and eighth positions)
    let levels: &[(&[f64], usize)] = &[
        (&[0.0], 0),                                                           // quarter → col 0
        (&[period_frames / 2.0], 1),                                           // eighth → col 1
        (&[period_frames / 4.0, period_frames * 3.0 / 4.0], 2),               // sixteenth → col 2
    ];

    for &(offsets, col) in levels {
        for &offset in offsets {
            // Walk grid positions: phase + offset, phase + offset + P, phase + offset + 2P, ...
            let mut grid_frame = phase_frames + offset;
            while grid_frame < 0.0 {
                grid_frame += period_frames;
            }

            while (grid_frame as usize) < duration_frames {
                let center = grid_frame as usize;
                let search_start = center.saturating_sub(tolerance);
                let search_end = (center + tolerance + 1).min(duration_frames);

                // Find the strongest unclaimed onset near this grid position
                let mut best_frame = None;
                let mut best_strength = 0.0f32;
                for f in search_start..search_end {
                    if !claimed[f] && onsets[f] > best_strength && onsets[f] > thresholds[f] {
                        best_strength = onsets[f];
                        best_frame = Some(f);
                    }
                }

                if let Some(bf) = best_frame {
                    // Use grid-aligned time for perfect sync with the music
                    let time = grid_frame / frames_per_sec;
                    beats.push(Beat { time, col });

                    // Claim frames around this onset so finer levels don't double-count
                    let claim_start = bf.saturating_sub(tolerance);
                    let claim_end = (bf + tolerance + 1).min(duration_frames);
                    for flag in &mut claimed[claim_start..claim_end] {
                        *flag = true;
                    }
                }

                grid_frame += period_frames;
            }
        }
    }

    // Column 3 (syncopation): strong unclaimed onsets that don't fall on any grid
    let min_interval_frames = (MIN_INTERVAL_SECS * frames_per_sec) as usize;
    let mut last_sync_frame: Option<usize> = None;
    for (f, &strength) in onsets.iter().enumerate() {
        if !claimed[f] && strength > thresholds[f] {
            if let Some(last) = last_sync_frame {
                if f - last < min_interval_frames {
                    continue;
                }
            }
            let time = f as f64 / frames_per_sec;
            let too_close = beats.iter().any(|b| (b.time - time).abs() < MIN_INTERVAL_SECS);
            if !too_close {
                beats.push(Beat { time, col: 3 });
                last_sync_frame = Some(f);
            }
        }
    }

    beats.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    beats
}

/// Minimum time (seconds) between two beats in the same column.
/// Tiles are 2 rows tall and scroll 1 row per tick (200ms), so tiles in the same
/// column need at least 3 ticks (600ms) of spacing to never overlap visually.
const MIN_SAME_COL_SECS: f64 = 0.6;

/// Enforce per-column minimum spacing. When two beats in the same column are too
/// close, try to reassign the later one to a free column. If no column is free,
/// drop the beat entirely.
fn deoverlap(mut beats: Vec<Beat>) -> Vec<Beat> {
    // Track the last beat time per column
    let mut last_col_time = [f64::NEG_INFINITY; COLS];
    let mut result = Vec::with_capacity(beats.len());

    for mut beat in beats.drain(..) {
        if beat.time - last_col_time[beat.col] >= MIN_SAME_COL_SECS {
            // Fits in its assigned column
            last_col_time[beat.col] = beat.time;
            result.push(beat);
        } else {
            // Try to find an alternative column that has space
            let alt = (0..COLS)
                .filter(|&c| c != beat.col)
                .filter(|&c| beat.time - last_col_time[c] >= MIN_SAME_COL_SECS)
                .min_by_key(|&c| {
                    // Prefer the column that was used least recently
                    (last_col_time[c] * 1000.0) as i64
                });
            if let Some(c) = alt {
                beat.col = c;
                last_col_time[c] = beat.time;
                result.push(beat);
            }
            // else: drop beat — no column available without overlap
        }
    }

    result
}

// ── Public API ──

pub fn detect_beats(path: &Path, mode: BeatMode) -> Result<Vec<Beat>> {
    let samples = decode_audio(path)?;
    let beats = match mode {
        BeatMode::Pitch => find_beats_pitch(&samples, SAMPLE_RATE),
        BeatMode::Rhythm => find_beats_rhythm(&samples, SAMPLE_RATE),
    };
    Ok(deoverlap(beats))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate silence of the given duration.
    fn silence(duration_secs: f64) -> Vec<f32> {
        vec![0.0; (SAMPLE_RATE as f64 * duration_secs) as usize]
    }

    /// Generate a sine wave at a given frequency and amplitude.
    fn sine_wave(duration_secs: f64, freq: f32, amplitude: f32) -> Vec<f32> {
        let n = (SAMPLE_RATE as f64 * duration_secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                amplitude * (freq * 2.0 * PI * t).sin()
            })
            .collect()
    }

    /// Insert a burst of a specific frequency into samples at the given time.
    fn insert_tone_burst(
        samples: &mut [f32],
        time_secs: f64,
        freq: f32,
        amplitude: f32,
        duration_samples: usize,
    ) {
        let center = (time_secs * SAMPLE_RATE as f64) as usize;
        let start = center.saturating_sub(duration_samples / 2);
        let end = (start + duration_samples).min(samples.len());
        for i in start..end {
            let t = (i - start) as f32 / SAMPLE_RATE as f32;
            samples[i] += amplitude * (freq * 2.0 * PI * t).sin();
        }
    }

    #[test]
    fn test_power_spectrum_dc() {
        // Constant signal → all energy in bin 0
        let input = vec![1.0; FRAME_SIZE];
        let spectrum = power_spectrum(&input);
        assert!(spectrum[0] > 0.0);
        // Other bins should be ~0
        let other_energy: f32 = spectrum[1..].iter().sum();
        assert!(
            other_energy < spectrum[0] * 0.001,
            "Non-DC energy should be negligible"
        );
    }

    #[test]
    fn test_power_spectrum_sine() {
        // 440 Hz sine → peak near bin 440*1024/44100 ≈ bin 10
        let input: Vec<f32> = (0..FRAME_SIZE)
            .map(|i| (440.0 * 2.0 * PI * i as f32 / SAMPLE_RATE as f32).sin())
            .collect();
        let spectrum = power_spectrum(&input);
        let expected_bin = (440.0 * FRAME_SIZE as f32 / SAMPLE_RATE as f32).round() as usize;
        let peak_bin = spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert!(
            (peak_bin as i32 - expected_bin as i32).unsigned_abs() <= 1,
            "Peak at bin {peak_bin}, expected near {expected_bin}"
        );
    }

    #[test]
    fn test_silence_produces_no_beats() {
        let samples = silence(5.0);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        assert!(beats.is_empty(), "Silence should produce no beats");
    }

    #[test]
    fn test_constant_tone_produces_no_beats() {
        let samples = sine_wave(5.0, 440.0, 0.5);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        assert!(
            beats.is_empty(),
            "Constant tone should produce no beats, got {}",
            beats.len()
        );
    }

    #[test]
    fn test_too_short_produces_no_beats() {
        let samples = vec![0.0; FRAME_SIZE - 1];
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        assert!(beats.is_empty());
    }

    #[test]
    fn test_bass_burst_maps_to_col_0() {
        let mut samples = silence(3.0);
        // 100 Hz is squarely in band 0 (bass: 0–200 Hz)
        insert_tone_burst(&mut samples, 1.5, 100.0, 0.9, FRAME_SIZE * 2);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        assert!(!beats.is_empty(), "Should detect bass beat");
        assert_eq!(beats[0].col, 0, "Bass should map to column 0");
    }

    #[test]
    fn test_high_burst_maps_to_col_3() {
        let mut samples = silence(3.0);
        // 8000 Hz is squarely in band 3 (high: 4 kHz+)
        insert_tone_burst(&mut samples, 1.5, 8000.0, 0.9, FRAME_SIZE * 2);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        assert!(!beats.is_empty(), "Should detect high-freq beat");
        assert_eq!(beats[0].col, 3, "High frequency should map to column 3");
    }

    #[test]
    fn test_different_frequencies_map_to_different_columns() {
        let mut samples = silence(5.0);
        // Bass burst at t=1
        insert_tone_burst(&mut samples, 1.0, 100.0, 0.9, FRAME_SIZE * 2);
        // High burst at t=3 (well separated)
        insert_tone_burst(&mut samples, 3.0, 8000.0, 0.9, FRAME_SIZE * 2);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        assert!(beats.len() >= 2, "Should detect at least 2 beats");
        let cols: Vec<usize> = beats.iter().map(|b| b.col).collect();
        assert!(cols.contains(&0), "Should have a bass beat (col 0)");
        assert!(cols.contains(&3), "Should have a high-freq beat (col 3)");
    }

    #[test]
    fn test_min_interval_enforced() {
        let mut samples = silence(3.0);
        // Two bursts very close together — should deduplicate
        insert_tone_burst(&mut samples, 1.0, 100.0, 0.9, FRAME_SIZE * 2);
        insert_tone_burst(&mut samples, 1.05, 100.0, 0.9, FRAME_SIZE * 2);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        assert!(beats.len() <= 1, "Close beats should be deduplicated");
    }

    #[test]
    fn test_global_min_interval_across_bands() {
        let mut samples = silence(3.0);
        // Bass and high at the exact same time — only one should survive
        insert_tone_burst(&mut samples, 1.5, 100.0, 0.9, FRAME_SIZE * 2);
        insert_tone_burst(&mut samples, 1.5, 8000.0, 0.9, FRAME_SIZE * 2);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        // With global minimum interval, at most one beat per 150ms
        let close_pairs = beats
            .windows(2)
            .filter(|w| (w[1].time - w[0].time) < MIN_INTERVAL_SECS)
            .count();
        assert_eq!(
            close_pairs, 0,
            "No two beats should be closer than MIN_INTERVAL"
        );
    }

    #[test]
    fn test_beats_are_sorted() {
        let mut samples = silence(6.0);
        // Beats at different times in different bands
        insert_tone_burst(&mut samples, 1.0, 100.0, 0.9, FRAME_SIZE * 2);
        insert_tone_burst(&mut samples, 3.0, 500.0, 0.9, FRAME_SIZE * 2);
        insert_tone_burst(&mut samples, 5.0, 8000.0, 0.9, FRAME_SIZE * 2);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        for window in beats.windows(2) {
            assert!(
                window[0].time < window[1].time,
                "Beats should be sorted by time"
            );
        }
    }

    #[test]
    fn test_all_columns_valid() {
        let mut samples = silence(5.0);
        insert_tone_burst(&mut samples, 1.0, 100.0, 0.9, FRAME_SIZE * 2);
        insert_tone_burst(&mut samples, 2.0, 500.0, 0.9, FRAME_SIZE * 2);
        insert_tone_burst(&mut samples, 3.0, 2000.0, 0.9, FRAME_SIZE * 2);
        insert_tone_burst(&mut samples, 4.0, 8000.0, 0.9, FRAME_SIZE * 2);
        let beats = find_beats_pitch(&samples, SAMPLE_RATE);
        for beat in &beats {
            assert!(beat.col < COLS, "Column {} out of range", beat.col);
        }
    }

    // ── Rhythm mode tests ──

    #[test]
    fn test_rhythm_silence_no_beats() {
        let samples = silence(5.0);
        let beats = find_beats_rhythm(&samples, SAMPLE_RATE);
        assert!(beats.is_empty());
    }

    #[test]
    fn test_rhythm_columns_valid() {
        // Generate regular impulses at 120 BPM (0.5s apart) with some offbeats
        let mut samples = silence(10.0);
        for i in 0..16 {
            let t = 1.0 + i as f64 * 0.5;
            insert_tone_burst(&mut samples, t, 200.0, 0.9, FRAME_SIZE * 2);
        }
        let beats = find_beats_rhythm(&samples, SAMPLE_RATE);
        for beat in &beats {
            assert!(beat.col < COLS, "Column {} out of range", beat.col);
        }
    }

    #[test]
    fn test_rhythm_hierarchy_quarter_over_eighth() {
        // Quarter note impulses at 120 BPM (every 0.5s) should be col 0 (beat).
        // Add eighth-note offbeats (every 0.25s offset) as col 1.
        let mut samples = silence(10.0);
        // Quarter notes at 1.0, 1.5, 2.0, ..., 8.0
        for i in 0..15 {
            let t = 1.0 + i as f64 * 0.5;
            insert_tone_burst(&mut samples, t, 200.0, 0.9, FRAME_SIZE * 2);
        }
        // Eighth offbeats at 1.25, 1.75, 2.25, ...
        for i in 0..14 {
            let t = 1.25 + i as f64 * 0.5;
            insert_tone_burst(&mut samples, t, 200.0, 0.7, FRAME_SIZE * 2);
        }
        let beats = find_beats_rhythm(&samples, SAMPLE_RATE);
        // Should have both col 0 (quarter) and col 1 (eighth) beats
        let has_quarter = beats.iter().any(|b| b.col == 0);
        let has_eighth = beats.iter().any(|b| b.col == 1);
        assert!(has_quarter, "Should detect quarter note beats (col 0)");
        assert!(has_eighth, "Should detect eighth note offbeats (col 1)");
    }

    #[test]
    fn test_rhythm_beats_are_sorted() {
        let mut samples = silence(10.0);
        for i in 0..16 {
            let t = 1.0 + i as f64 * 0.5;
            insert_tone_burst(&mut samples, t, 200.0, 0.9, FRAME_SIZE * 2);
        }
        let beats = find_beats_rhythm(&samples, SAMPLE_RATE);
        for pair in beats.windows(2) {
            assert!(
                pair[0].time <= pair[1].time,
                "Beats should be sorted: {:.3} > {:.3}",
                pair[0].time,
                pair[1].time
            );
        }
    }

    // ── Deoverlap tests ──

    #[test]
    fn test_deoverlap_keeps_spaced_beats() {
        let beats = vec![
            Beat { time: 0.0, col: 0 },
            Beat { time: 1.0, col: 0 },
            Beat { time: 2.0, col: 0 },
        ];
        let result = deoverlap(beats);
        assert_eq!(result.len(), 3);
        // All stay in col 0
        assert!(result.iter().all(|b| b.col == 0));
    }

    #[test]
    fn test_deoverlap_reassigns_close_same_col() {
        // Two beats in col 0, only 0.2s apart — second should be reassigned
        let beats = vec![Beat { time: 0.0, col: 0 }, Beat { time: 0.2, col: 0 }];
        let result = deoverlap(beats);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].col, 0);
        assert_ne!(
            result[1].col, 0,
            "Close beat should be moved to another column"
        );
    }

    #[test]
    fn test_deoverlap_drops_when_all_cols_full() {
        // 5 beats all within 0.1s — only 4 columns available, fifth must be dropped
        let beats = vec![
            Beat { time: 0.0, col: 0 },
            Beat { time: 0.02, col: 1 },
            Beat { time: 0.04, col: 2 },
            Beat { time: 0.06, col: 3 },
            Beat { time: 0.08, col: 0 },
        ];
        let result = deoverlap(beats);
        assert_eq!(result.len(), 4, "Fifth beat should be dropped");
    }

    #[test]
    fn test_deoverlap_no_same_col_overlap() {
        // Dense beats, verify no two in the same column are closer than MIN_SAME_COL_SECS
        let beats: Vec<Beat> = (0..20)
            .map(|i| Beat {
                time: i as f64 * 0.2,
                col: i % COLS,
            })
            .collect();
        let result = deoverlap(beats);
        for col in 0..COLS {
            let col_times: Vec<f64> = result
                .iter()
                .filter(|b| b.col == col)
                .map(|b| b.time)
                .collect();
            for pair in col_times.windows(2) {
                assert!(
                    pair[1] - pair[0] >= MIN_SAME_COL_SECS,
                    "Col {col}: beats at {:.3}s and {:.3}s are too close (need {MIN_SAME_COL_SECS}s)",
                    pair[0], pair[1]
                );
            }
        }
    }

    #[test]
    fn test_ffmpeg_decodes_generated_wav() {
        let test_path = "/tmp/piano-pad-test.wav";
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

        let Ok(status) = gen else { return };
        if !status.success() {
            return;
        }

        let result = decode_audio(Path::new(test_path));
        let _ = std::fs::remove_file(test_path);

        let samples = result.expect("Should decode WAV file");
        assert!(
            samples.len() >= 40000 && samples.len() <= 48000,
            "Expected ~44100 samples, got {}",
            samples.len()
        );
        let max_abs = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max_abs > 0.1, "Decoded audio should not be silent");
    }
}
