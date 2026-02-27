mod audio;
mod game;
mod input;
mod lamparray;

use anyhow::Result;
use crossterm::terminal;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use audio::Audio;
use game::{Game, PressResult, State};
use input::{poll_input, GameInput};
use lamparray::{Color, LampArray};

const POLL_TIMEOUT: Duration = Duration::from_millis(5);

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

fn run() -> Result<()> {
    // Open hardware
    let lamp = LampArray::open()?;
    lamp.disable_autonomous()?;

    // Open audio (optional — continue without sound if unavailable)
    let audio = Audio::new().ok();

    // Enable raw mode for keyboard input
    terminal::enable_raw_mode()?;
    let _guard = CleanupGuard::new(&lamp);

    let mut game = Game::new();
    let mut last_grid = [[Color::BLACK; 4]; 6];
    let mut dirty = true;
    let mut last_tick = Instant::now();

    // Print initial status
    print_status(&game);

    loop {
        // Render LEDs only when state changed
        if dirty {
            let grid = game.render();
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
                        dirty = true;
                        print_status(&game);
                    }
                },
                State::Playing => match input {
                    GameInput::Quit => break,
                    GameInput::Column(col) => {
                        let result = game.press_column(col);
                        match result {
                            PressResult::Hit => {
                                if let Some(ref a) = audio {
                                    a.play_column_tone(col);
                                }
                                dirty = true;
                                print_status(&game);
                            }
                            PressResult::Miss => {
                                if let Some(ref a) = audio {
                                    a.play_game_over();
                                }
                                dirty = true;
                                flash_game_over(&lamp, &game)?;
                                print_status(&game);
                            }
                            PressResult::Ignored => {}
                        }
                    }
                    GameInput::AnyKey => {}
                },
                State::GameOver => match input {
                    GameInput::Quit => break,
                    GameInput::AnyKey | GameInput::Column(_) => {
                        game.reset();
                        dirty = true;
                        print_status(&game);
                    }
                },
            }
        }

        // Tick timer (only during gameplay)
        if game.state == State::Playing {
            let tick_duration = Duration::from_millis(game.tick_ms());
            if last_tick.elapsed() >= tick_duration {
                let ok = game.tick();
                last_tick = Instant::now();
                dirty = true;
                if !ok {
                    // Game over from missed tile
                    if let Some(ref a) = audio {
                        a.play_game_over();
                    }
                    flash_game_over(&lamp, &game)?;
                    print_status(&game);
                }
            }
        }
    }

    // _guard drops here: re-enables autonomous, disables raw mode
    drop(_guard);

    // Print final score after raw mode is off
    println!("\r\nFinal score: {}\r", game.score);

    Ok(())
}

/// Flash the LEDs red 3 times for game over.
fn flash_game_over(lamp: &LampArray, _game: &Game) -> Result<()> {
    for _ in 0..3 {
        lamp.fill(Color::RED)?;
        std::thread::sleep(Duration::from_millis(150));
        lamp.fill(Color::BLACK)?;
        std::thread::sleep(Duration::from_millis(100));
    }
    lamp.fill(Color::RED)?;
    Ok(())
}

/// Print game status to the terminal (works in raw mode).
fn print_status(game: &Game) {
    let status = match game.state {
        State::Ready => "Press any macropad key to start!".to_string(),
        State::Playing => format!("Score: {}  |  Speed: {}ms", game.score, game.tick_ms()),
        State::GameOver => format!(
            "GAME OVER!  Score: {}  |  Press any key to restart",
            game.score
        ),
    };
    // Move to beginning of line, clear it, print status
    print!("\r\x1b[K{status}");
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
