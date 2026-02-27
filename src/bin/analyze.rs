//! Diagnostic tool for analyzing beat detection on a song.
//! Usage: cargo run --bin analyze -- [song.mp3]

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const FRAME_SIZE: usize = 1024;
const HOP_SIZE: usize = 512;
const THRESHOLD_WINDOW: usize = 15;
const THRESHOLD_MULTIPLIER: f32 = 1.5;
const THRESHOLD_FLOOR: f32 = 0.01;
const MIN_INTERVAL_SECS: f64 = 0.15;
const SAMPLE_RATE: u32 = 44100;

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| find_first_mp3().expect("No MP3 found"));

    println!("Analyzing: {}", path.display());
    analyze(&path)
}

fn find_first_mp3() -> Result<PathBuf> {
    let songs_dir = PathBuf::from("songs");
    if songs_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&songs_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());
        if let Some(entry) = entries.first() {
            return Ok(entry.path());
        }
    }
    anyhow::bail!("No MP3 found. Pass a path as argument or place one in songs/")
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
        .context("Failed to run ffmpeg")?;

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

fn analyze(path: &Path) -> Result<()> {
    let samples = decode_audio(path)?;
    let sample_rate = SAMPLE_RATE;

    if samples.is_empty() {
        println!("ERROR: No audio samples decoded!");
        return Ok(());
    }

    let duration = samples.len() as f64 / sample_rate as f64;
    println!(
        "Samples: {}, Duration: {duration:.1}s, Rate: {sample_rate}",
        samples.len()
    );

    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    let f32_min = samples.iter().cloned().fold(f32::INFINITY, f32::min);
    let f32_max = samples.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    println!("Range: [{f32_min:.4}, {f32_max:.4}], RMS: {rms:.6}");

    // Frame energies
    let num_frames = if samples.len() >= FRAME_SIZE {
        (samples.len() - FRAME_SIZE) / HOP_SIZE + 1
    } else {
        println!("ERROR: Not enough samples for even one frame!");
        return Ok(());
    };

    let mut energies = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let start = i * HOP_SIZE;
        let end = start + FRAME_SIZE;
        let energy: f32 =
            samples[start..end].iter().map(|s| s * s).sum::<f32>() / FRAME_SIZE as f32;
        energies.push(energy);
    }

    let e_min = energies.iter().cloned().fold(f32::INFINITY, f32::min);
    let e_max = energies.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let e_mean = energies.iter().sum::<f32>() / energies.len() as f32;
    println!("Frames: {num_frames}");
    println!("Energy: min={e_min:.8}, max={e_max:.8}, mean={e_mean:.8}");

    // Onset strengths
    let mut onsets = vec![0.0f32; energies.len()];
    for i in 1..energies.len() {
        onsets[i] = (energies[i] - energies[i - 1]).max(0.0);
    }

    let o_max = onsets.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let o_mean = onsets.iter().sum::<f32>() / onsets.len() as f32;
    let o_nonzero = onsets.iter().filter(|&&v| v > 0.0).count();
    println!("Onsets: max={o_max:.8}, mean={o_mean:.8}, nonzero={o_nonzero}");

    // Threshold analysis
    let min_interval_frames =
        (MIN_INTERVAL_SECS * sample_rate as f64 / HOP_SIZE as f64) as usize;

    let mut beat_count = 0;
    let mut last_beat_frame: Option<usize> = None;

    println!("\n--- Beats detected (first 30) ---");
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
            beat_count += 1;
            if beat_count <= 30 {
                println!(
                    "  #{beat_count:3} t={time:7.3}s  onset={:.6}  thresh={threshold:.6}",
                    onsets[i]
                );
            }
            last_beat_frame = Some(i);
        }
    }
    println!("Total beats: {beat_count}");

    if beat_count == 0 {
        println!("\n--- Diagnosis ---");
        println!(
            "Onset max ({o_max:.8}) vs threshold floor ({THRESHOLD_FLOOR}): {}",
            if o_max <= THRESHOLD_FLOOR {
                "ALL onsets below floor! Energy changes too small."
            } else {
                "Some onsets above floor, but adaptive threshold is filtering them."
            }
        );
        let mut top_onsets: Vec<(usize, f32)> = onsets.iter().copied().enumerate().collect();
        top_onsets.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        println!("Top 10 onset strengths:");
        for (i, (frame, val)) in top_onsets.iter().take(10).enumerate() {
            let time = (frame * HOP_SIZE) as f64 / sample_rate as f64;
            println!("  #{}: t={time:.3}s  onset={val:.8}", i + 1);
        }
    }

    Ok(())
}
