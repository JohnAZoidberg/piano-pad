use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameInput {
    /// Player pressed a column key (0-3), mapped from bottom-row keys U/V/W/X
    Column(usize),
    /// Quit the game
    Quit,
    /// Any other key (used for "press to start" / "press to restart")
    AnyKey,
}

/// Map a crossterm KeyEvent to a GameInput.
fn map_key(event: KeyEvent) -> Option<GameInput> {
    // Ctrl+C always quits
    if event.modifiers.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('c') {
        return Some(GameInput::Quit);
    }

    match event.code {
        KeyCode::Esc => Some(GameInput::Quit),
        // Bottom-row macropad keys: U=col0, V=col1, W=col2, X=col3
        KeyCode::Char('u' | 'U') => Some(GameInput::Column(0)),
        KeyCode::Char('v' | 'V') => Some(GameInput::Column(1)),
        KeyCode::Char('w' | 'W') => Some(GameInput::Column(2)),
        KeyCode::Char('x' | 'X') => Some(GameInput::Column(3)),
        _ => Some(GameInput::AnyKey),
    }
}

/// Poll for input with a timeout. Returns None if no input within the timeout.
pub fn poll_input(timeout: Duration) -> Option<GameInput> {
    if event::poll(timeout).ok()? {
        if let Event::Key(key_event) = event::read().ok()? {
            return map_key(key_event);
        }
    }
    None
}
