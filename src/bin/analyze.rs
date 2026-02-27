//! Diagnostic tool for analyzing beat detection on a song.
//! Usage: cargo run --bin analyze -- [--rhythm] [song.mp3]
//!
//! Shows beat detection results using the same algorithm as the game.

use anyhow::{Context, Result};
use piano_pad::beats::{self, BeatMode};
use std::path::{Path, PathBuf};
use std::process::Command;

const SAMPLE_RATE: u32 = 44100;
const PITCH_NAMES: [&str; 4] = ["bass", "low-mid", "mid-high", "high"];
const RHYTHM_NAMES: [&str; 4] = ["downbeat", "offbeat", "16th", "syncopation"];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode = BeatMode::Pitch;
    let mut song_path: Option<PathBuf> = None;

    for arg in &args {
        match arg.as_str() {
            "--rhythm" => mode = BeatMode::Rhythm,
            _ => song_path = Some(PathBuf::from(arg)),
        }
    }

    let path = song_path.unwrap_or_else(|| find_first_mp3().expect("No MP3 found"));

    let mode_name = match mode {
        BeatMode::Pitch => "pitch",
        BeatMode::Rhythm => "rhythm",
    };
    println!("Analyzing: {} (mode: {mode_name})", path.display());

    let samples = decode_audio(&path)?;
    if samples.is_empty() {
        println!("ERROR: No audio samples decoded!");
        return Ok(());
    }

    let duration = samples.len() as f64 / SAMPLE_RATE as f64;
    println!("Samples: {}, Duration: {duration:.1}s", samples.len());

    let beats = beats::detect_beats(&path, mode)?;
    let names = match mode {
        BeatMode::Pitch => &PITCH_NAMES,
        BeatMode::Rhythm => &RHYTHM_NAMES,
    };

    // Count per column
    let mut per_col = [0u32; 4];
    for beat in &beats {
        per_col[beat.col] += 1;
    }

    println!("\n--- Beat summary ---");
    println!("Total beats: {}", beats.len());
    for (i, name) in names.iter().enumerate() {
        println!("  Col {i} ({name:>12}): {}", per_col[i]);
    }

    // Show first 40 beats
    println!("\n--- First 40 beats ---");
    for (i, beat) in beats.iter().take(40).enumerate() {
        println!(
            "  #{:3} t={:7.3}s  col={} ({})",
            i + 1,
            beat.time,
            beat.col,
            names[beat.col]
        );
    }

    if beats.len() > 40 {
        println!("  ... ({} more)", beats.len() - 40);
    }

    Ok(())
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
