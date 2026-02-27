# Piano Pad

A beat-synced rhythm game for the Framework Laptop 16 macropad. Tiles scroll
down the 6×4 LED grid in time with an MP3's beats — press the matching key
when a tile is near you.

## How It Works

1. **Beat analysis** — On startup the game decodes the MP3 with ffmpeg and
   detects beats. Two analysis modes are available:
   - **Pitch** (default) — FFT sub-band analysis assigns columns by frequency
     band (bass, low-mid, mid-high, high).
   - **Rhythm** (`--rhythm`) — Estimates tempo via autocorrelation, then
     classifies beats by metrical position (downbeat, offbeat, 16th note,
     syncopation).
2. **Song playback** — The actual song plays through your speakers, starting
   after a short delay so the first tiles have time to scroll into view.
3. **Tile scrolling** — Each beat spawns a 2-row-tall tile at the top of the
   grid. Tiles scroll down at a constant speed (one row per 200ms tick).
   Each column has two alternating shades so consecutive tiles are visually
   distinct.
4. **Pressing tiles** — Press the matching column key on any row where you can
   see the tile. There is a one-row grace zone above and below to account for
   timing.
5. **Scoring** — Successful hits increment your score. Missed tiles (wrong key
   or tile scrolls off the bottom) count as misses. The song keeps playing
   either way — no game-over interruption.
6. **Song complete** — After all beats have scrolled through, the grid turns
   green and your final score is shown.

## Controls

All macropad keys (A–X) are mapped to a 6×4 grid:

| Row | Col 0 | Col 1 | Col 2 | Col 3 |
|-----|-------|-------|-------|-------|
| 0   | A     | B     | C     | D     |
| 1   | E     | F     | G     | H     |
| 2   | I     | J     | K     | L     |
| 3   | M     | N     | O     | P     |
| 4   | Q     | R     | S     | T     |
| 5   | U     | V     | W     | X     |

Press the key in the correct column wherever the tile is on the grid.

- **Esc** or **Ctrl+C** — quit
- **Any key** on the Ready/Complete screen — start or restart

## Running

```
# Pass an MP3 directly
cargo run -- path/to/song.mp3

# Or place MP3 files in a songs/ directory (first one alphabetically is used)
mkdir songs
cp song.mp3 songs/
cargo run

# Use rhythm-based column assignment instead of pitch-based
cargo run -- --rhythm

# Slow it down to half speed for practice
cargo run -- --speed 0.5

# Skip the intro (jump to 1.5s before the first beat)
cargo run -- --skip-intro

# Run without the physical macropad (terminal grid only)
cargo run -- --no-pad

# Combine options
cargo run -- --speed 0.75 --skip-intro --rhythm path/to/song.mp3
```

The terminal always shows a colored 6×4 grid simulation using ANSI 24-bit
color. With `--no-pad` you can run and test without the hardware attached.

### Analyze tool

A diagnostic tool for inspecting beat detection results:

```
cargo run --bin analyze -- songs/song.mp3
cargo run --bin analyze -- --rhythm songs/song.mp3
```

### Sync test tool

Simulates the game's tick model without hardware or audio and measures how
closely each tile's arrival at the hit zone matches its beat's song time.
Reports mean/max error, standard deviation, and a per-column breakdown.
Prints `PASS` if all beats land within one tick (200ms — the theoretical
quantization limit), `FAIL` otherwise with details of the worst offenders.

```
cargo run --bin sync_test -- songs/song.mp3
cargo run --bin sync_test -- --rhythm songs/song.mp3
```

## Requirements

- Framework Laptop 16 with LED macropad module (or use `--no-pad` to run without it)
- **ffmpeg** — used for audio decoding (must be in `$PATH`)
- Rust toolchain
- Linux (HID access — may need `udev` rules or root)
