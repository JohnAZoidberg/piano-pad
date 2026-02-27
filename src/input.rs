use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameInput {
    /// Player pressed a column key (0-3)
    Column(usize),
    /// Quit the game
    Quit,
    /// Any other key (used for "press to start" / "press to restart")
    AnyKey,
}

/// Map a crossterm KeyEvent to a GameInput.
/// Macropad ABC layout: A-D=row0, E-H=row1, I-L=row2, M-P=row3, Q-T=row4, U-X=row5
/// All rows map to columns 0-3.
fn map_key(event: KeyEvent) -> Option<GameInput> {
    // Ctrl+C always quits
    if event.modifiers.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('c') {
        return Some(GameInput::Quit);
    }

    match event.code {
        KeyCode::Esc => Some(GameInput::Quit),
        KeyCode::Char(c) => {
            let c = c.to_ascii_lowercase();
            if ('a'..='x').contains(&c) {
                let col = (c as usize - 'a' as usize) % 4;
                Some(GameInput::Column(col))
            } else {
                Some(GameInput::AnyKey)
            }
        }
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
