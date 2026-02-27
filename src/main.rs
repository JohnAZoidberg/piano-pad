mod audio;
mod beats;
mod game;
mod input;
mod lamparray;

use anyhow::Result;
use crossterm::terminal;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use audio::Audio;
use game::{Game, PressResult, State, SCROLL_DELAY_MS, TICK_MS};

const DEFAULT_SPEED: f32 = 1.0;
use input::{poll_input, GameInput};
use lamparray::{Color, LampArray};

const POLL_TIMEOUT: Duration = Duration::from_millis(5);
const FLASH_DURATION: Duration = Duration::from_millis(250);

const FLASH_HIT: Color = Color::new(255, 255, 255);
const FLASH_MISS: Color = Color::new(255, 0, 0);

struct Flash {
    cells: Vec<(usize, usize, Color)>, // (row, col, color)
    until: Instant,
}

impl Flash {
    fn hit(col: usize) -> Self {
        Self {
            cells: vec![(4, col, FLASH_HIT), (5, col, FLASH_HIT)],
            until: Instant::now() + FLASH_DURATION,
        }
    }

    fn miss_press(col: usize) -> Self {
        Self {
            cells: vec![(4, col, FLASH_MISS), (5, col, FLASH_MISS)],
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
    lamp: &'a LampArray,
}

impl<'a> CleanupGuard<'a> {
    fn new(lamp: &'a LampArray) -> Self {
        Self { lamp }
    }
}

impl Drop for CleanupGuard<'_> {
    fn drop(&mut self) {
        let _ = self.lamp.enable_autonomous();
        let _ = terminal::disable_raw_mode();
    }
}

struct Config {
    song_path: PathBuf,
    speed: f32,
    skip_intro: bool,
}

/// Parse CLI args: [--speed N] [--skip-intro] [song.mp3]
fn parse_args() -> Result<Config> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut speed = DEFAULT_SPEED;
    let mut skip_intro = false;
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

fn run() -> Result<()> {
    // Parse CLI args
    let config = parse_args()?;
    let speed = config.speed;
    let song_path = config.song_path;

    // Wall-clock tick and delay, stretched by 1/speed
    let tick_duration = Duration::from_micros((TICK_MS as f64 / speed as f64 * 1000.0) as u64);
    let scroll_delay = Duration::from_micros(
        (SCROLL_DELAY_MS as f64 / speed as f64 * 1000.0) as u64,
    );

    // Find and analyze song before entering raw mode
    println!("Analyzing {}...", song_path.display());
    let mut beats = beats::detect_beats(&song_path)?;

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

    // Open hardware
    let lamp = LampArray::open()?;
    lamp.disable_autonomous()?;

    // Open audio (optional — continue without sound if unavailable)
    let mut audio = Audio::new().ok();

    // Enable raw mode for keyboard input
    terminal::enable_raw_mode()?;
    let _guard = CleanupGuard::new(&lamp);

    let mut game = Game::new(beats);
    let mut last_grid = [[Color::BLACK; 4]; 6];
    let mut dirty = true;
    let mut last_tick = Instant::now();
    let mut song_started = false;
    let mut game_start_time = Instant::now();
    let mut flash: Option<Flash> = None;

    // Print initial status
    print_status(&game);

    loop {
        // Expire flash
        if let Some(ref f) = flash {
            if Instant::now() >= f.until {
                flash = None;
                dirty = true;
            }
        }

        // Render LEDs only when state changed
        if dirty {
            let mut grid = game.render();
            // Overlay flash on top of game grid
            if let Some(ref f) = flash {
                for &(row, col, color) in &f.cells {
                    grid[row][col] = color;
                }
            }
            if grid != last_grid {
                lamp.render_grid(&grid)?;
                last_grid = grid;
            }
            dirty = false;
        }

        // Poll input
        if let Some(input) = poll_input(POLL_TIMEOUT) {
            match game.state {
                State::Ready => match input {
                    GameInput::Quit => break,
                    GameInput::AnyKey | GameInput::Column(_) => {
                        game.start();
                        last_tick = Instant::now();
                        game_start_time = Instant::now();
                        song_started = false;
                        flash = None;
                        dirty = true;
                        print_status(&game);
                    }
                },
                State::Playing => match input {
                    GameInput::Quit => break,
                    GameInput::Column(col) => {
                        let result = game.press(col);
                        match result {
                            PressResult::Hit => {
                                flash = Some(Flash::hit(col));
                                print_debug(&format!(
                                    "HIT  col={col}  score={}/{}",
                                    game.score, game.total_beats
                                ));
                                dirty = true;
                                print_status(&game);
                            }
                            PressResult::Miss => {
                                flash = Some(Flash::miss_press(col));
                                print_debug(&format!(
                                    "MISS col={col}  misses={}",
                                    game.misses
                                ));
                                dirty = true;
                                print_status(&game);
                            }
                            PressResult::Ignored => {}
                        }
                    }
                    GameInput::AnyKey => {}
                },
                State::SongComplete => match input {
                    GameInput::Quit => break,
                    GameInput::AnyKey | GameInput::Column(_) => {
                        game.reset();
                        song_started = false;
                        flash = None;
                        dirty = true;
                        print_status(&game);
                    }
                },
            }
        }

        // Game timing (only during gameplay)
        if game.state == State::Playing {
            // Start song after scroll delay
            if !song_started && game_start_time.elapsed() >= scroll_delay {
                if let Some(ref mut a) = audio {
                    let _ = a.play_song(&song_path, speed, song_seek);
                }
                song_started = true;
            }

            // Tick at constant intervals (stretched by 1/speed)
            if last_tick.elapsed() >= tick_duration {
                let dropped = game.tick();
                last_tick = Instant::now();
                dirty = true;

                // Flash hit zone red when tiles fall through
                if dropped > 0 {
                    flash = Some(Flash::miss_dropped());
                    print_status(&game);
                }

                // Check for song complete
                if game.state == State::SongComplete {
                    if let Some(ref mut a) = audio {
                        a.stop_song();
                    }
                    print_status(&game);
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

/// Print game status to the terminal status line (works in raw mode).
fn print_status(game: &Game) {
    let status = match game.state {
        State::Ready => "Press any macropad key to start!".to_string(),
        State::Playing => format!(
            "Score: {} / {}  |  Missed: {}",
            game.score, game.total_beats, game.misses
        ),
        State::SongComplete => format!(
            "SONG COMPLETE!  Score: {} / {}  |  Missed: {}  |  Press any key to restart",
            game.score, game.total_beats, game.misses
        ),
    };
    // Move to beginning of line, clear it, print status
    print!("\r\x1b[K{status}");
    let _ = io::stdout().flush();
}

/// Print a debug line above the status line (works in raw mode).
fn print_debug(msg: &str) {
    // Print message, then newline, so status line can be reprinted below
    print!("\r\x1b[K{msg}\r\n");
    let _ = io::stdout().flush();
}

fn main() {
    if let Err(e) = run() {
        // Make sure raw mode is off before printing error
        let _ = terminal::disable_raw_mode();
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
