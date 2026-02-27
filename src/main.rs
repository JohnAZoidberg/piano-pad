mod audio;
mod beats;
mod display;
mod game;
mod input;
mod lamparray;

use anyhow::Result;
use crossterm::terminal;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use audio::Audio;
use beats::BeatMode;
use game::{Game, PressResult, State, SCROLL_DELAY_MS, TICK_MS};

const DEFAULT_SPEED: f32 = 1.0;
use input::{poll_input, GameInput};
use lamparray::{Color, LampArray};

const POLL_TIMEOUT: Duration = Duration::from_millis(5);
const FLASH_DURATION: Duration = Duration::from_millis(250);

const FLASH_MISS: Color = Color::new(255, 0, 0);

/// Number of terminal lines occupied by the grid display (6 rows + 1 status).
const GRID_LINES: usize = 7;

struct Flash {
    cells: Vec<(usize, usize, Color)>, // (row, col, color)
    until: Instant,
}

impl Flash {
    fn miss_press(row: usize, col: usize) -> Self {
        Self {
            cells: vec![(row, col, FLASH_MISS)],
            until: Instant::now() + FLASH_DURATION,
        }
    }

    fn miss_dropped() -> Self {
        let cells = (0..4).map(|col| (5, col, FLASH_MISS)).collect();
        Self {
            cells,
            until: Instant::now() + Duration::from_millis(100),
        }
    }
}

/// RAII guard that re-enables autonomous LED mode and disables raw terminal mode on drop.
struct CleanupGuard<'a> {
    lamp: Option<&'a LampArray>,
}

impl<'a> CleanupGuard<'a> {
    fn new(lamp: Option<&'a LampArray>) -> Self {
        Self { lamp }
    }
}

impl Drop for CleanupGuard<'_> {
    fn drop(&mut self) {
        if let Some(lamp) = self.lamp {
            let _ = lamp.enable_autonomous();
        }
        let _ = terminal::disable_raw_mode();
    }
}

struct Config {
    song_path: PathBuf,
    speed: f32,
    skip_intro: bool,
    beat_mode: BeatMode,
    no_pad: bool,
}

/// Parse CLI args: [--speed N] [--skip-intro] [--rhythm] [--no-pad] [song.mp3]
fn parse_args() -> Result<Config> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut speed = DEFAULT_SPEED;
    let mut skip_intro = false;
    let mut beat_mode = BeatMode::Pitch;
    let mut no_pad = false;
    let mut song_path: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--speed" => {
                i += 1;
                if i >= args.len() {
                    anyhow::bail!("--speed requires a value (e.g. --speed 0.5)");
                }
                speed = args[i].parse::<f32>().map_err(|_| {
                    anyhow::anyhow!(
                        "Invalid speed value '{}' (expected a number like 0.5)",
                        args[i]
                    )
                })?;
                if speed <= 0.0 {
                    anyhow::bail!("Speed must be positive (got {speed})");
                }
            }
            "--skip-intro" => {
                skip_intro = true;
            }
            "--rhythm" => {
                beat_mode = BeatMode::Rhythm;
            }
            "--no-pad" => {
                no_pad = true;
            }
            _ => {
                let path = PathBuf::from(&args[i]);
                if !path.exists() {
                    anyhow::bail!("Song file not found: {}", path.display());
                }
                song_path = Some(path);
            }
        }
        i += 1;
    }

    let song_path = match song_path {
        Some(p) => p,
        None => find_first_mp3()?,
    };

    Ok(Config {
        song_path,
        speed,
        skip_intro,
        beat_mode,
        no_pad,
    })
}

/// Find the first .mp3 in the songs/ directory.
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
    anyhow::bail!("No song found. Pass an MP3 path as argument or place one in songs/")
}

/// Lead-in time before first beat when skipping intro (seconds).
const INTRO_LEAD_IN: f64 = 1.5;

/// Build the status string for the current game state.
fn status_text(game: &Game) -> String {
    match game.state {
        State::Ready => "Press any macropad key to start!".to_string(),
        State::Playing => format!(
            "Score: {} / {}  |  Missed: {}",
            game.score, game.total_beats, game.misses
        ),
        State::SongComplete => format!(
            "SONG COMPLETE!  Score: {} / {}  |  Missed: {}  |  Press any key to restart",
            game.score, game.total_beats, game.misses
        ),
    }
}

fn run() -> Result<()> {
    // Parse CLI args
    let config = parse_args()?;
    let speed = config.speed;
    let song_path = config.song_path;

    // Wall-clock tick duration, stretched by 1/speed
    let tick_duration = Duration::from_micros((TICK_MS as f64 / speed as f64 * 1000.0) as u64);
    // Number of ticks to pre-simulate so tiles are already on-screen when the song starts
    let pre_ticks = (SCROLL_DELAY_MS / TICK_MS) as usize;

    // Find and analyze song before entering raw mode
    println!("Analyzing {}...", song_path.display());
    let mut beats = beats::detect_beats(&song_path, config.beat_mode)?;

    // Skip intro: shift beat times so the first beat arrives quickly
    let intro_skip = if config.skip_intro {
        if let Some(first) = beats.first() {
            let skip = (first.time - INTRO_LEAD_IN).max(0.0);
            if skip > 0.0 {
                for beat in &mut beats {
                    beat.time -= skip;
                }
                println!("Skipping {skip:.1}s intro");
            }
            skip
        } else {
            0.0
        }
    } else {
        0.0
    };
    let song_seek = Duration::from_secs_f64(intro_skip);

    if speed != DEFAULT_SPEED {
        println!("Found {} beats (speed: {speed:.1}x)", beats.len());
    } else {
        println!("Found {} beats", beats.len());
    }

    // Open hardware (optional with --no-pad)
    let lamp = if config.no_pad {
        None
    } else {
        let l = LampArray::open()?;
        l.disable_autonomous()?;
        Some(l)
    };

    // Open audio (optional — continue without sound if unavailable)
    let mut audio = Audio::new().ok();

    // Enable raw mode for keyboard input
    terminal::enable_raw_mode()?;
    let _guard = CleanupGuard::new(lamp.as_ref());

    let mut stdout = io::stdout();
    let mut game = Game::new(beats);
    let mut last_grid = [[Color::BLACK; 4]; 6];
    let mut dirty = true;
    let mut last_tick = Instant::now();
    let mut flash: Option<Flash> = None;
    let mut drew_grid = false;

    loop {
        // Expire flash
        if let Some(ref f) = flash {
            if Instant::now() >= f.until {
                flash = None;
                dirty = true;
            }
        }

        // Render when state changed
        if dirty {
            let mut grid = game.render();
            // Overlay flash on top of game grid
            if let Some(ref f) = flash {
                for &(row, col, color) in &f.cells {
                    grid[row][col] = color;
                }
            }

            // Terminal grid: reposition cursor and redraw
            if drew_grid {
                // Move cursor up to overwrite previous grid + status
                write!(stdout, "\x1b[{}A\r", GRID_LINES)?;
            }
            display::render_terminal_grid(&mut stdout, &grid, &status_text(&game))?;
            drew_grid = true;

            // Hardware: only send when grid colors actually changed
            if let Some(ref l) = lamp {
                if grid != last_grid {
                    l.render_grid(&grid)?;
                }
            }
            last_grid = grid;
            dirty = false;
        }

        // Poll input
        if let Some(input) = poll_input(POLL_TIMEOUT) {
            match game.state {
                State::Ready => match input {
                    GameInput::Quit => break,
                    GameInput::AnyKey | GameInput::Press(_, _) => {
                        game.start();
                        // Pre-simulate ticks so tiles are already on the grid,
                        // then start song immediately — no silent scrolling.
                        for _ in 0..pre_ticks {
                            game.tick();
                        }
                        if let Some(ref mut a) = audio {
                            let _ = a.play_song(&song_path, speed, song_seek);
                        }
                        last_tick = Instant::now();
                        flash = None;
                        dirty = true;
                    }
                },
                State::Playing => match input {
                    GameInput::Quit => break,
                    GameInput::Press(row, col) => {
                        let result = game.press(row, col);
                        match result {
                            PressResult::Hit => {
                                print_debug(
                                    &mut stdout,
                                    drew_grid,
                                    &format!(
                                        "HIT  row={row} col={col}  score={}/{}",
                                        game.score, game.total_beats
                                    ),
                                );
                                drew_grid = false;
                                dirty = true;
                            }
                            PressResult::Miss => {
                                flash = Some(Flash::miss_press(row, col));
                                print_debug(
                                    &mut stdout,
                                    drew_grid,
                                    &format!("MISS row={row} col={col}  misses={}", game.misses),
                                );
                                drew_grid = false;
                                dirty = true;
                            }
                            PressResult::Ignored => {}
                        }
                    }
                    GameInput::AnyKey => {}
                },
                State::SongComplete => match input {
                    GameInput::Quit => break,
                    GameInput::AnyKey | GameInput::Press(_, _) => {
                        game.reset();
                        flash = None;
                        dirty = true;
                    }
                },
            }
        }

        // Game timing (only during gameplay)
        if game.state == State::Playing {
            // Tick at constant intervals (stretched by 1/speed).
            // Advance last_tick by tick_duration (not Instant::now()) to prevent
            // cumulative drift — if a tick fires late, the next fires sooner.
            if last_tick.elapsed() >= tick_duration {
                let dropped = game.tick();
                last_tick += tick_duration;
                dirty = true;

                // Flash hit zone red when tiles fall through
                if dropped > 0 {
                    flash = Some(Flash::miss_dropped());
                }

                // Check for song complete
                if game.state == State::SongComplete {
                    if let Some(ref mut a) = audio {
                        a.stop_song();
                    }
                }
            }
        }
    }

    // _guard drops here: re-enables autonomous, disables raw mode
    drop(_guard);

    // Print final score after raw mode is off
    println!(
        "\r\nFinal score: {} / {}  (missed: {})\r",
        game.score, game.total_beats, game.misses
    );

    Ok(())
}

/// Print a debug line above the grid (works in raw mode).
/// Moves cursor above the grid first, prints the message, then lets the grid redraw below.
fn print_debug(stdout: &mut impl Write, drew_grid: bool, msg: &str) {
    if drew_grid {
        // Move up past the grid, print debug line, then the grid will be redrawn below
        let _ = write!(stdout, "\x1b[{}A\r\x1b[K{msg}\r\n", GRID_LINES);
    } else {
        let _ = write!(stdout, "\r\x1b[K{msg}\r\n");
    }
    let _ = stdout.flush();
}

fn main() {
    if let Err(e) = run() {
        // Make sure raw mode is off before printing error
        let _ = terminal::disable_raw_mode();
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
