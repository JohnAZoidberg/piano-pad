//! Diagnostic tool for measuring tile-to-beat sync accuracy.
//!
//! Simulates the game's tick model without hardware or audio, then reports how
//! closely each tile's arrival at the hit zone matches the beat's song time.
//!
//! Usage: cargo run --bin sync_test -- [--rhythm] [song.mp3]

use anyhow::Result;
use piano_pad::beats::{self, BeatMode};
use std::path::PathBuf;

const ROWS: usize = 6;
const TICK_MS: u64 = 200;
const SCROLL_TICKS: usize = 4;

const PITCH_NAMES: [&str; 4] = ["bass", "low-mid", "mid-high", "high"];
const RHYTHM_NAMES: [&str; 4] = ["downbeat", "offbeat", "16th", "syncopation"];

/// Maximum acceptable sync error (one tick — the theoretical quantization limit).
/// Beats are spawned on the first tick at or after their time, so they can be
/// up to one tick late. Errors beyond this indicate a simulation bug.
const TOLERANCE_SECS: f64 = TICK_MS as f64 / 1000.0;

struct SimTile {
    row: usize,
    beat_idx: usize,
}

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
    let names = match mode {
        BeatMode::Pitch => &PITCH_NAMES,
        BeatMode::Rhythm => &RHYTHM_NAMES,
    };

    println!("Analyzing: {} (mode: {mode_name})", path.display());

    let beats = beats::detect_beats(&path, mode)?;
    if beats.is_empty() {
        println!("No beats detected.");
        return Ok(());
    }

    let total_beats = beats.len();

    // Simulate the game tick loop.
    // pre_ticks matches SCROLL_DELAY_MS / TICK_MS = SCROLL_TICKS + 1 = 5.
    // The extra +1 accounts for the first real game tick firing one tick_duration
    // AFTER the song starts (the game loop waits before its first tick).
    let pre_ticks = SCROLL_TICKS + 1;
    let mut tiles: Vec<SimTile> = Vec::new();
    let mut next_beat_idx: usize = 0;

    // sync_error[i] = song_time - beat_time when beat i's tile reaches row 4
    let mut sync_errors: Vec<(usize, f64)> = Vec::new(); // (beat_idx, error)

    // Run pre-ticks + enough ticks for all beats to scroll through
    let max_ticks =
        pre_ticks + ((beats.last().unwrap().time * 1000.0 / TICK_MS as f64) as usize) + ROWS + 10;

    for tick in 0..max_ticks {
        // Game's tick order: move → remove → spawn → increment
        // We check arrivals after move, before remove.

        // 1. Move all tiles down one row
        for tile in &mut tiles {
            tile.row += 1;
        }

        // 2. Check for tiles at row SCROLL_TICKS (first row of hit zone) — record sync error.
        //    In the real game, ticks 0..pre_ticks run instantly (pre-simulation), then the
        //    song starts and the first real tick fires after one tick_duration. So:
        //      song_time at tick T = (T - pre_ticks + 1) * TICK_MS/1000  for T >= pre_ticks
        //    During pre-ticks (T < pre_ticks), song hasn't started yet.
        if tick >= pre_ticks {
            let song_time = (tick - pre_ticks + 1) as f64 * TICK_MS as f64 / 1000.0;
            for tile in &tiles {
                if tile.row == SCROLL_TICKS {
                    let beat_time = beats[tile.beat_idx].time;
                    let error = song_time - beat_time;
                    sync_errors.push((tile.beat_idx, error));
                }
            }
        }

        // 3. Remove tiles that fell past the grid
        tiles.retain(|t| t.row + 1 < ROWS);

        // 4. Spawn tiles whose beat.time <= tick * TICK_MS / 1000
        //    (elapsed_ticks == tick at this point, incremented after spawn in game.rs)
        let elapsed_secs = tick as f64 * TICK_MS as f64 / 1000.0;
        while next_beat_idx < beats.len() {
            if beats[next_beat_idx].time <= elapsed_secs {
                tiles.push(SimTile {
                    row: 0,
                    beat_idx: next_beat_idx,
                });
                next_beat_idx += 1;
            } else {
                break;
            }
        }

        // Stop once all beats have been processed and no tiles remain
        if next_beat_idx >= beats.len() && tiles.is_empty() {
            break;
        }
    }

    // Compute statistics
    let n = sync_errors.len();
    if n == 0 {
        println!("No tiles reached the hit zone.");
        return Ok(());
    }

    let errors: Vec<f64> = sync_errors.iter().map(|(_, e)| *e).collect();
    let mean_error = errors.iter().sum::<f64>() / n as f64;
    let max_error = errors.iter().map(|e| e.abs()).fold(0.0f64, f64::max);
    let variance = errors.iter().map(|e| (e - mean_error).powi(2)).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();

    let within_50ms = errors.iter().filter(|e| e.abs() <= 0.05).count();
    let within_100ms = errors.iter().filter(|e| e.abs() <= 0.1).count();
    let within_tolerance = errors.iter().filter(|e| e.abs() <= TOLERANCE_SECS).count();

    let tolerance_ms = (TOLERANCE_SECS * 1000.0) as u64;
    println!("\nSync test: {total_beats} beats");
    println!("  Mean error:   {:+.2}s", mean_error);
    println!("  Max error:    {:+.2}s", max_error);
    println!("  Std dev:       {:.2}s", std_dev);
    println!(
        "  Within 50ms:   {:.1}% ({}/{})",
        within_50ms as f64 / n as f64 * 100.0,
        within_50ms,
        n
    );
    println!(
        "  Within 100ms:  {:.1}% ({}/{})",
        within_100ms as f64 / n as f64 * 100.0,
        within_100ms,
        n
    );
    println!(
        "  Within {tolerance_ms}ms:  {:.1}% ({}/{})",
        within_tolerance as f64 / n as f64 * 100.0,
        within_tolerance,
        n
    );

    // Per-column breakdown
    println!("\nPer-column breakdown:");
    for (col, name) in names.iter().enumerate() {
        let col_errors: Vec<f64> = sync_errors
            .iter()
            .filter(|(idx, _)| beats[*idx].col == col)
            .map(|(_, e)| *e)
            .collect();
        if col_errors.is_empty() {
            continue;
        }
        let col_mean = col_errors.iter().sum::<f64>() / col_errors.len() as f64;
        println!(
            "  Col {} ({:>12}): {:4} beats, mean error {:+.2}s",
            col,
            name,
            col_errors.len(),
            col_mean
        );
    }

    // Pass/fail verdict
    println!();
    if within_tolerance == n {
        println!("PASS: all beats within {tolerance_ms}ms tolerance (1 tick)");
    } else {
        println!(
            "FAIL: {}/{} beats outside {tolerance_ms}ms tolerance",
            n - within_tolerance,
            n
        );
        // Show worst offenders
        let mut worst: Vec<(usize, f64)> = sync_errors
            .iter()
            .filter(|(_, e)| e.abs() > TOLERANCE_SECS)
            .cloned()
            .collect();
        worst.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap());
        for (idx, error) in worst.iter().take(10) {
            let beat = &beats[*idx];
            println!(
                "  Beat #{} at {:.3}s (col {}): error {:+.3}s",
                idx, beat.time, beat.col, error
            );
        }
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
