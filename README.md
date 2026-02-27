# Piano Pad

A beat-synced rhythm game for the Framework Laptop 16 macropad. Tiles scroll
down the 6×4 LED grid in time with an MP3's beats — press the right key when
the tile reaches the bottom two rows.

## How It Works

1. **Beat analysis** — On startup the game decodes the MP3, runs energy-based
   onset detection, and builds a list of beat timestamps.
2. **Song playback** — The actual song plays through your speakers, starting
   after a short delay so the first tiles have time to scroll into view.
3. **Tile scrolling** — Each beat spawns a 2-row-tall tile at the top of the
   grid. Tiles scroll down at a constant speed (one row per 200ms tick).
4. **Hit zone** — The bottom two rows (rows 4 and 5) are the hit zone. When a
   tile reaches this zone you have 400ms (2 ticks) to press the matching
   column key.
5. **Scoring** — Successful hits increment your score. Missed tiles (wrong key
   or tile scrolls past) count as misses. The song keeps playing either way —
   no game-over interruption.
6. **Song complete** — After all beats have scrolled through, the grid turns
   green and your final score is shown.

## Controls

The macropad's bottom two rows of keys map to columns 0-3:

| Row | Col 0 | Col 1 | Col 2 | Col 3 |
|-----|-------|-------|-------|-------|
| 4   | Q     | R     | S     | T     |
| 5   | U     | V     | W     | X     |

Both rows hit the same columns — press whichever row is more comfortable.

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

# Slow it down to half speed for practice
cargo run -- --speed 0.5

# Skip the intro (jump to 1.5s before the first beat)
cargo run -- --skip-intro

# Combine options
cargo run -- --speed 0.75 --skip-intro path/to/song.mp3
```

## Requirements

- Framework Laptop 16 with LED macropad module
- Rust toolchain
- Linux (HID access — may need `udev` rules or root)
