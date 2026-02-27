use crate::lamparray::Color;
use std::io::Write;

/// Render the 6×4 color grid to a terminal using ANSI 24-bit background colors.
/// Each cell is 3 spaces wide with 1-space gaps. Total: 7 lines (6 grid + 1 status).
pub fn render_terminal_grid(
    w: &mut impl Write,
    grid: &[[Color; 4]; 6],
    status: &str,
) -> std::io::Result<()> {
    for row in grid {
        write!(w, "\r ")?;
        for (i, color) in row.iter().enumerate() {
            if i > 0 {
                write!(w, "\x1b[0m ")?;
            }
            write!(w, "\x1b[48;2;{};{};{}m   ", color.r, color.g, color.b)?;
        }
        write!(w, "\x1b[0m\x1b[K\r\n")?;
    }
    write!(w, "\r\x1b[K{status}")?;
    w.flush()
}
