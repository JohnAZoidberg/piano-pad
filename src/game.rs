use crate::beats::Beat;
use crate::lamparray::Color;

const COLS: usize = 4;
const ROWS: usize = 6;

pub const TICK_MS: u64 = 200;
const SCROLL_TICKS: usize = 4; // tiles travel from row 0 to row 4 (2-row tile fills rows 4-5)
pub const SCROLL_DELAY_MS: u64 = TICK_MS * SCROLL_TICKS as u64;

/// Two shades per column so consecutive tiles are visually distinct.
const COL_SHADES: [[Color; 2]; COLS] = [
    [Color::new(0, 80, 255), Color::new(80, 160, 255)],   // Col 0: blue / light blue
    [Color::new(0, 220, 0), Color::new(100, 255, 100)],    // Col 1: green / light green
    [Color::new(255, 200, 0), Color::new(255, 255, 100)],  // Col 2: yellow / pale yellow
    [Color::new(255, 0, 80), Color::new(255, 100, 160)],   // Col 3: pink / light pink
];

#[derive(Clone)]
struct Tile {
    col: usize,
    row: usize,
    shade: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    Playing,
    SongComplete,
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
    pub misses: u32,
    pub total_beats: usize,
    tiles: Vec<Tile>,
    beats: Vec<Beat>,
    next_beat_idx: usize,
    elapsed_ticks: usize,
    col_shade_counter: [u8; COLS],
}

impl Game {
    pub fn new(beats: Vec<Beat>) -> Self {
        let total_beats = beats.len();
        Self {
            state: State::Ready,
            score: 0,
            misses: 0,
            total_beats,
            tiles: Vec::new(),
            beats,
            next_beat_idx: 0,
            elapsed_ticks: 0,
            col_shade_counter: [0; COLS],
        }
    }

    /// Start a new game.
    pub fn start(&mut self) {
        self.state = State::Playing;
        self.score = 0;
        self.misses = 0;
        self.tiles.clear();
        self.next_beat_idx = 0;
        self.elapsed_ticks = 0;
        self.col_shade_counter = [0; COLS];
    }

    /// Reset to ready screen.
    pub fn reset(&mut self) {
        self.state = State::Ready;
        self.score = 0;
        self.misses = 0;
        self.tiles.clear();
        self.next_beat_idx = 0;
        self.elapsed_ticks = 0;
        self.col_shade_counter = [0; COLS];
    }

    /// Advance one game tick. Returns the number of tiles that fell off (missed).
    pub fn tick(&mut self) -> u32 {
        if self.state != State::Playing {
            return 0;
        }

        // 1. Move all tiles down one row
        for tile in &mut self.tiles {
            tile.row += 1;
        }

        // 2. Remove tiles that fell past the grid (missed beats)
        let before = self.tiles.len();
        self.tiles.retain(|t| t.row + 1 < ROWS);
        let dropped = (before - self.tiles.len()) as u32;
        self.misses += dropped;

        // 3. Spawn tiles whose beat.time <= elapsed_ticks * TICK_MS / 1000
        let elapsed_secs = self.elapsed_ticks as f64 * TICK_MS as f64 / 1000.0;
        while self.next_beat_idx < self.beats.len() {
            if self.beats[self.next_beat_idx].time <= elapsed_secs {
                let beat = &self.beats[self.next_beat_idx];
                let col = beat.col;
                let shade = self.col_shade_counter[col] % 2;
                self.col_shade_counter[col] = self.col_shade_counter[col].wrapping_add(1);
                self.tiles.push(Tile { col, row: 0, shade });
                self.next_beat_idx += 1;
            } else {
                break;
            }
        }

        // 4. Increment elapsed_ticks
        self.elapsed_ticks += 1;

        // 5. If all beats spawned and no tiles left → SongComplete
        if self.next_beat_idx >= self.beats.len() && self.tiles.is_empty() {
            self.state = State::SongComplete;
        }

        dropped
    }

    /// Handle a key press at (row, col). Matches any tile whose visual span overlaps the
    /// pressed row, with a one-row grace zone above and below to account for timing between
    /// render and keypress.
    pub fn press(&mut self, row: usize, col: usize) -> PressResult {
        if self.state != State::Playing {
            return PressResult::Ignored;
        }

        // Tile occupies tile.row and tile.row+1. Accept presses one row above or below
        // that span to account for tick timing (tile may have moved since last render).
        // So: accept if pressed_row is within tile.row-1 ..= tile.row+2
        if let Some(idx) = self
            .tiles
            .iter()
            .position(|t| t.col == col && row + 1 >= t.row && row <= t.row + 2)
        {
            self.tiles.remove(idx);
            self.score += 1;
            PressResult::Hit
        } else if self
            .tiles
            .iter()
            .any(|t| row + 1 >= t.row && row <= t.row + 2)
        {
            // A tile is nearby at this row but wrong column → miss
            self.misses += 1;
            PressResult::Miss
        } else {
            // No tiles near this row → lenient ignore
            PressResult::Ignored
        }
    }

    /// Render the current game state as a 6×4 color grid.
    pub fn render(&self) -> [[Color; 4]; 6] {
        match self.state {
            State::Ready => self.render_ready(),
            State::Playing => self.render_playing(),
            State::SongComplete => self.render_song_complete(),
        }
    }

    fn render_ready(&self) -> [[Color; 4]; 6] {
        let mut grid = [[Color::BLACK; 4]; 6];
        // Light up bottom two rows in dim column colors (use shade 0)
        for (col, shades) in COL_SHADES.iter().enumerate() {
            let c = shades[0];
            let dim = Color::new(c.r / 4, c.g / 4, c.b / 4);
            grid[4][col] = dim;
            grid[5][col] = dim;
        }
        grid
    }

    fn render_playing(&self) -> [[Color; 4]; 6] {
        let mut grid = [[Color::BLACK; 4]; 6];
        for tile in &self.tiles {
            let color = COL_SHADES[tile.col][tile.shade as usize];
            // Each tile is 2 rows tall: top at tile.row, bottom at tile.row+1
            if tile.row < ROWS {
                grid[tile.row][tile.col] = color;
            }
            let bottom = tile.row + 1;
            if bottom < ROWS {
                grid[bottom][tile.col] = color;
            }
        }
        grid
    }

    fn render_song_complete(&self) -> [[Color; 4]; 6] {
        [[Color::new(0, 255, 0); 4]; 6]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_beats(times: &[f64]) -> Vec<Beat> {
        times
            .iter()
            .enumerate()
            .map(|(i, &time)| Beat {
                time,
                col: i % COLS,
            })
            .collect()
    }

    #[test]
    fn test_new_game_is_ready() {
        let game = Game::new(vec![]);
        assert_eq!(game.state, State::Ready);
        assert_eq!(game.score, 0);
        assert_eq!(game.misses, 0);
    }

    #[test]
    fn test_start_game() {
        let beats = make_beats(&[0.0, 0.5, 1.0]);
        let mut game = Game::new(beats);
        game.start();
        assert_eq!(game.state, State::Playing);
        assert_eq!(game.score, 0);
        assert_eq!(game.next_beat_idx, 0);
        assert_eq!(game.elapsed_ticks, 0);
    }

    #[test]
    fn test_tick_spawns_beat_at_time_zero() {
        let beats = make_beats(&[0.0]);
        let mut game = Game::new(beats);
        game.start();

        // First tick: moves tiles (none yet), spawns beat at time 0.0, increments elapsed
        game.tick();
        assert_eq!(game.tiles.len(), 1);
        assert_eq!(game.tiles[0].row, 0);
        assert_eq!(game.next_beat_idx, 1);
    }

    #[test]
    fn test_missed_tile_drops_off_and_counts() {
        // A tile at row 4 (occupies 4,5) moves to row 5 (occupies 5,6) → removed, miss counted
        // Use a far-future beat so the game doesn't immediately complete
        let beats = make_beats(&[999.0]);
        let mut game = Game::new(beats);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 0, row: 4, shade: 0 });

        let dropped = game.tick();
        assert_eq!(dropped, 1);
        assert!(game.tiles.is_empty());
        assert_eq!(game.misses, 1);
        assert_eq!(game.state, State::Playing); // game continues
    }

    #[test]
    fn test_press_hit_at_row_3() {
        // Tile at row 3 occupies rows 3,4. Pressing row 3, col 2 should hit.
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 2, row: 3, shade: 0 });

        let result = game.press(3, 2);
        assert_eq!(result, PressResult::Hit);
        assert_eq!(game.score, 1);
        assert!(game.tiles.is_empty());
    }

    #[test]
    fn test_press_hit_at_bottom_of_tile() {
        // Tile at row 3 occupies rows 3,4. Pressing row 4, col 2 should also hit.
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 2, row: 3, shade: 0 });

        let result = game.press(4, 2);
        assert_eq!(result, PressResult::Hit);
        assert_eq!(game.score, 1);
    }

    #[test]
    fn test_press_hit_at_row_0() {
        // Tile at row 0 occupies rows 0,1. Pressing row 1, col 1 should hit.
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 1, row: 0, shade: 0 });

        let result = game.press(1, 1);
        assert_eq!(result, PressResult::Hit);
        assert_eq!(game.score, 1);
    }

    #[test]
    fn test_press_wrong_column_is_miss() {
        // Tile at row 2 (occupies 2,3). Wrong column at same row → miss.
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 2, row: 2, shade: 0 });

        let result = game.press(2, 0);
        assert_eq!(result, PressResult::Miss);
        assert_eq!(game.misses, 1);
        assert_eq!(game.state, State::Playing);
    }

    #[test]
    fn test_press_grace_zone_above() {
        // Tile at row 2 (occupies 2,3). Grace zone extends to row 1.
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 1, row: 2, shade: 0 });

        let result = game.press(1, 1);
        assert_eq!(result, PressResult::Hit);
    }

    #[test]
    fn test_press_grace_zone_below() {
        // Tile at row 2 (occupies 2,3). Grace zone extends to row 4.
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 1, row: 2, shade: 0 });

        let result = game.press(4, 1);
        assert_eq!(result, PressResult::Hit);
    }

    #[test]
    fn test_press_no_tile_at_row_is_ignored() {
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 0, row: 1, shade: 0 });

        // Tile at row 1 (occupies 1,2), grace zone 0-3. Press at row 5 is outside.
        let result = game.press(5, 0);
        assert_eq!(result, PressResult::Ignored);
    }

    #[test]
    fn test_song_complete() {
        // Single beat at time 0
        let beats = make_beats(&[0.0]);
        let mut game = Game::new(beats);
        game.start();

        // Tick 1: spawn beat at row 0 (occupies 0,1)
        game.tick();
        assert_eq!(game.tiles.len(), 1);

        // Move to row 3 (occupies 3,4 — partially in hit zone)
        game.tick(); // row 1
        game.tick(); // row 2
        game.tick(); // row 3

        // Hit it (tile is at row 3, occupying rows 3-4)
        let col = game.tiles[0].col;
        let row = game.tiles[0].row;
        let result = game.press(row, col);
        assert_eq!(result, PressResult::Hit);

        // Next tick should detect song complete (all beats spawned, no tiles)
        game.tick();
        assert_eq!(game.state, State::SongComplete);
    }

    #[test]
    fn test_song_complete_with_misses() {
        // Beat at time 0 — let it fall off, song should still complete
        let beats = make_beats(&[0.0]);
        let mut game = Game::new(beats);
        game.start();

        // Tick through until tile falls off
        for _ in 0..6 {
            game.tick();
        }

        assert_eq!(game.misses, 1);
        assert_eq!(game.state, State::SongComplete);
    }

    #[test]
    fn test_render_song_complete_all_green() {
        let mut game = Game::new(vec![]);
        game.state = State::SongComplete;
        let grid = game.render();
        for row in &grid {
            for color in row {
                assert_eq!(*color, Color::new(0, 255, 0));
            }
        }
    }

    #[test]
    fn test_render_ready_hit_zone_lit() {
        let game = Game::new(vec![]);
        let grid = game.render();
        // Rows 4 and 5 should be lit
        for col in 0..4 {
            assert_ne!(grid[4][col], Color::BLACK);
            assert_ne!(grid[5][col], Color::BLACK);
        }
        // Other rows should be black
        for row in 0..4 {
            for col in 0..4 {
                assert_eq!(grid[row][col], Color::BLACK);
            }
        }
    }

    #[test]
    fn test_render_playing_shows_2row_tiles() {
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 1, row: 2, shade: 0 });

        let grid = game.render();
        // Tile occupies row 2 and row 3, both in column 1's shade 0
        assert_eq!(grid[2][1], COL_SHADES[1][0]);
        assert_eq!(grid[3][1], COL_SHADES[1][0]);
        // Adjacent columns should be black
        assert_eq!(grid[2][0], Color::BLACK);
        assert_eq!(grid[3][0], Color::BLACK);
        // Row above should be black
        assert_eq!(grid[1][1], Color::BLACK);
    }

    #[test]
    fn test_render_alternating_shades() {
        let mut game = Game::new(vec![]);
        game.state = State::Playing;
        game.tiles.push(Tile { col: 0, row: 0, shade: 0 });
        game.tiles.push(Tile { col: 0, row: 3, shade: 1 });

        let grid = game.render();
        // First tile (shade 0) at rows 0,1
        assert_eq!(grid[0][0], COL_SHADES[0][0]);
        assert_eq!(grid[1][0], COL_SHADES[0][0]);
        // Second tile (shade 1) at rows 3,4
        assert_eq!(grid[3][0], COL_SHADES[0][1]);
        assert_eq!(grid[4][0], COL_SHADES[0][1]);
        // The two shades should be different
        assert_ne!(COL_SHADES[0][0], COL_SHADES[0][1]);
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
        let beats = make_beats(&[0.0, 0.5]);
        let mut game = Game::new(beats);
        game.start();
        game.score = 42;
        game.misses = 3;
        game.reset();
        assert_eq!(game.state, State::Ready);
        assert_eq!(game.score, 0);
        assert_eq!(game.misses, 0);
        assert!(game.tiles.is_empty());
        assert_eq!(game.next_beat_idx, 0);
        assert_eq!(game.elapsed_ticks, 0);
    }
}
