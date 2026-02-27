use crate::lamparray::Color;
use rand::Rng;

const COLS: usize = 4;
const ROWS: usize = 6;
const HIT_ROW: usize = ROWS - 1;

const INITIAL_TICK_MS: u64 = 650;
const MIN_TICK_MS: u64 = 200;
const TICK_DECREASE_PER_SCORE: u64 = 5;

/// Row colors: dim blue → cyan → green → yellow → orange → bright white
const ROW_COLORS: [Color; ROWS] = [
    Color::new(0, 0, 80),      // Row 0: dim blue (spawn)
    Color::new(0, 120, 120),   // Row 1: cyan
    Color::new(0, 180, 0),     // Row 2: green
    Color::new(200, 200, 0),   // Row 3: yellow
    Color::new(255, 120, 0),   // Row 4: orange
    Color::new(255, 255, 255), // Row 5: bright white (hit zone)
];

#[derive(Clone)]
struct Tile {
    col: usize,
    row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    Playing,
    GameOver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressResult {
    Hit,
    Miss,
    Ignored,
}

pub struct Game {
    pub state: State,
    pub score: u32,
    tiles: Vec<Tile>,
    rng: rand::rngs::ThreadRng,
}

impl Game {
    pub fn new() -> Self {
        Self {
            state: State::Ready,
            score: 0,
            tiles: Vec::new(),
            rng: rand::thread_rng(),
        }
    }

    /// Start a new game.
    pub fn start(&mut self) {
        self.state = State::Playing;
        self.score = 0;
        self.tiles.clear();
        // Spawn one tile immediately so the board isn't empty
        self.spawn_tile();
    }

    /// Reset to ready screen.
    pub fn reset(&mut self) {
        self.state = State::Ready;
        self.score = 0;
        self.tiles.clear();
    }

    /// Current tick interval in milliseconds.
    pub fn tick_ms(&self) -> u64 {
        let decrease = self.score as u64 * TICK_DECREASE_PER_SCORE;
        INITIAL_TICK_MS.saturating_sub(decrease).max(MIN_TICK_MS)
    }

    /// Advance one game tick: move tiles down, spawn new tile.
    /// Returns false if a tile fell off the bottom (game over).
    pub fn tick(&mut self) -> bool {
        if self.state != State::Playing {
            return true;
        }

        // Move all tiles down
        for tile in &mut self.tiles {
            tile.row += 1;
        }

        // Check if any tile fell off the bottom
        if self.tiles.iter().any(|t| t.row > HIT_ROW) {
            self.state = State::GameOver;
            return false;
        }

        // Spawn a new tile at the top
        self.spawn_tile();
        true
    }

    /// Handle a column press. Returns the result.
    pub fn press_column(&mut self, col: usize) -> PressResult {
        if self.state != State::Playing {
            return PressResult::Ignored;
        }

        // Check if there's a tile at the hit row
        let hit_row_tile = self.tiles.iter().position(|t| t.row == HIT_ROW);

        match hit_row_tile {
            Some(idx) if self.tiles[idx].col == col => {
                // Correct hit
                self.tiles.remove(idx);
                self.score += 1;
                PressResult::Hit
            }
            Some(_) => {
                // Wrong column — game over
                self.state = State::GameOver;
                PressResult::Miss
            }
            None => {
                // No tile at hit row — lenient, just ignore
                PressResult::Ignored
            }
        }
    }

    fn spawn_tile(&mut self) {
        let col = self.rng.gen_range(0..COLS);
        self.tiles.push(Tile { col, row: 0 });
    }

    /// Render the current game state as a 6×4 color grid.
    pub fn render(&self) -> [[Color; 4]; 6] {
        match self.state {
            State::Ready => self.render_ready(),
            State::Playing => self.render_playing(),
            State::GameOver => self.render_game_over(),
        }
    }

    fn render_ready(&self) -> [[Color; 4]; 6] {
        let mut grid = [[Color::BLACK; 4]; 6];
        // Light up bottom row to indicate "press to start"
        for col in 0..COLS {
            grid[HIT_ROW][col] = Color::new(0, 80, 0);
        }
        grid
    }

    fn render_playing(&self) -> [[Color; 4]; 6] {
        let mut grid = [[Color::BLACK; 4]; 6];
        for tile in &self.tiles {
            if tile.row < ROWS {
                grid[tile.row][tile.col] = ROW_COLORS[tile.row];
            }
        }
        grid
    }

    fn render_game_over(&self) -> [[Color; 4]; 6] {
        [[Color::RED; 4]; 6]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_game_is_ready() {
        let game = Game::new();
        assert_eq!(game.state, State::Ready);
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_start_game() {
        let mut game = Game::new();
        game.start();
        assert_eq!(game.state, State::Playing);
        assert_eq!(game.score, 0);
        // Should have spawned one tile
        assert_eq!(game.tiles.len(), 1);
        assert_eq!(game.tiles[0].row, 0);
    }

    #[test]
    fn test_tick_moves_tiles_down() {
        let mut game = Game::new();
        game.start();
        let initial_col = game.tiles[0].col;

        game.tick();
        // Original tile moved to row 1, new tile spawned at row 0
        assert_eq!(game.tiles.len(), 2);
        assert!(game
            .tiles
            .iter()
            .any(|t| t.row == 1 && t.col == initial_col));
        assert!(game.tiles.iter().any(|t| t.row == 0));
    }

    #[test]
    fn test_tile_falls_off_is_game_over() {
        let mut game = Game::new();
        game.state = State::Playing;
        game.tiles.push(Tile {
            col: 0,
            row: HIT_ROW,
        });

        let ok = game.tick();
        assert!(!ok);
        assert_eq!(game.state, State::GameOver);
    }

    #[test]
    fn test_correct_hit() {
        let mut game = Game::new();
        game.state = State::Playing;
        game.tiles.push(Tile {
            col: 2,
            row: HIT_ROW,
        });

        let result = game.press_column(2);
        assert_eq!(result, PressResult::Hit);
        assert_eq!(game.score, 1);
        assert!(game.tiles.is_empty());
    }

    #[test]
    fn test_wrong_column_is_miss() {
        let mut game = Game::new();
        game.state = State::Playing;
        game.tiles.push(Tile {
            col: 2,
            row: HIT_ROW,
        });

        let result = game.press_column(0);
        assert_eq!(result, PressResult::Miss);
        assert_eq!(game.state, State::GameOver);
    }

    #[test]
    fn test_no_tile_at_hit_row_is_ignored() {
        let mut game = Game::new();
        game.state = State::Playing;
        game.tiles.push(Tile { col: 0, row: 2 });

        let result = game.press_column(0);
        assert_eq!(result, PressResult::Ignored);
    }

    #[test]
    fn test_speed_scaling() {
        let mut game = Game::new();
        assert_eq!(game.tick_ms(), 650);

        game.score = 10;
        assert_eq!(game.tick_ms(), 600);

        game.score = 90;
        assert_eq!(game.tick_ms(), 200); // floor

        game.score = 200;
        assert_eq!(game.tick_ms(), 200); // still floor
    }

    #[test]
    fn test_render_ready_bottom_row_lit() {
        let game = Game::new();
        let grid = game.render();
        // Bottom row should be lit
        for col in 0..4 {
            assert_ne!(grid[HIT_ROW][col], Color::BLACK);
        }
        // Other rows should be black
        for row in 0..HIT_ROW {
            for col in 0..4 {
                assert_eq!(grid[row][col], Color::BLACK);
            }
        }
    }

    #[test]
    fn test_render_playing_shows_tiles() {
        let mut game = Game::new();
        game.state = State::Playing;
        game.tiles.push(Tile { col: 1, row: 3 });

        let grid = game.render();
        assert_eq!(grid[3][1], ROW_COLORS[3]);
        assert_eq!(grid[3][0], Color::BLACK);
    }

    #[test]
    fn test_render_game_over_all_red() {
        let mut game = Game::new();
        game.state = State::GameOver;
        let grid = game.render();
        for row in &grid {
            for color in row {
                assert_eq!(*color, Color::RED);
            }
        }
    }

    #[test]
    fn test_led_map_validity() {
        use crate::lamparray::LED_MAP;
        let mut seen = [false; 24];
        for row in &LED_MAP {
            for &led_id in row {
                assert!((led_id as usize) < 24, "LED id {led_id} out of range");
                assert!(!seen[led_id as usize], "Duplicate LED id {led_id}");
                seen[led_id as usize] = true;
            }
        }
        // All 24 LEDs should be mapped
        for (i, &s) in seen.iter().enumerate() {
            assert!(s, "LED {i} not mapped");
        }
    }

    #[test]
    fn test_reset() {
        let mut game = Game::new();
        game.start();
        game.score = 42;
        game.reset();
        assert_eq!(game.state, State::Ready);
        assert_eq!(game.score, 0);
        assert!(game.tiles.is_empty());
    }
}
