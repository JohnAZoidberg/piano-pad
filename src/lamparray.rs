use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};

const VID: u16 = 0x32AC;
const PID: u16 = 0x0013;
const USAGE_PAGE: u16 = 0x0059;

const REPORT_CONTROL: u8 = 0x06;
const REPORT_MULTI_UPDATE: u8 = 0x04;

const LAMP_UPDATE_COMPLETE: u8 = 0x01;

/// LED grid mapping: LED_MAP[row][col] = LED index
/// Derived from QMK firmware macropad.c/macropad.h
pub const LED_MAP: [[u8; 4]; 6] = [
    [5, 2, 22, 17],  // Row 0 (top — tiles spawn here)
    [4, 0, 20, 18],  // Row 1
    [7, 1, 21, 16],  // Row 2
    [6, 3, 23, 19],  // Row 3
    [9, 11, 15, 13], // Row 4
    [8, 10, 14, 12], // Row 5 (bottom — hit zone)
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const BLACK: Color = Color::new(0, 0, 0);
    #[allow(dead_code)]
    pub const RED: Color = Color::new(255, 0, 0);
}

pub struct LampArray {
    device: HidDevice,
}

impl LampArray {
    pub fn open() -> Result<Self> {
        let api = HidApi::new().context("Failed to initialize HID API")?;
        for info in api.device_list() {
            if info.vendor_id() == VID
                && info.product_id() == PID
                && info.usage_page() == USAGE_PAGE
            {
                let device = info
                    .open_device(&api)
                    .context("Failed to open macropad HID device")?;
                return Ok(Self { device });
            }
        }
        anyhow::bail!(
            "Macropad not found (VID={VID:#06x}, PID={PID:#06x}, UsagePage={USAGE_PAGE:#06x})"
        );
    }

    /// Take control of LEDs from the device's autonomous mode.
    pub fn disable_autonomous(&self) -> Result<()> {
        let buf = [REPORT_CONTROL, 0x00];
        self.device
            .send_feature_report(&buf)
            .context("Failed to disable autonomous mode")?;
        Ok(())
    }

    /// Release LED control back to the device.
    pub fn enable_autonomous(&self) -> Result<()> {
        let buf = [REPORT_CONTROL, 0x01];
        self.device
            .send_feature_report(&buf)
            .context("Failed to enable autonomous mode")?;
        Ok(())
    }

    /// Render a 6×4 color grid to the macropad LEDs.
    /// Sends 3 LampMultiUpdateReports (8 LEDs each), last one with LampUpdateComplete.
    pub fn render_grid(&self, grid: &[[Color; 4]; 6]) -> Result<()> {
        // Flatten grid into 24 LED colors indexed by LED id
        let mut led_colors = [Color::BLACK; 24];
        for (row_idx, row) in grid.iter().enumerate() {
            for (col_idx, color) in row.iter().enumerate() {
                let led_id = LED_MAP[row_idx][col_idx] as usize;
                led_colors[led_id] = *color;
            }
        }

        // Send in 3 batches of 8
        for batch in 0..3 {
            let start = batch * 8;
            let flags = if batch == 2 {
                LAMP_UPDATE_COMPLETE
            } else {
                0x00
            };
            self.send_multi_update(start, &led_colors[start..start + 8], flags)?;
        }
        Ok(())
    }

    /// Set all 24 LEDs to a single color.
    #[allow(dead_code)]
    pub fn fill(&self, color: Color) -> Result<()> {
        let grid = [[color; 4]; 6];
        self.render_grid(&grid)
    }

    fn send_multi_update(&self, start_id: usize, colors: &[Color], flags: u8) -> Result<()> {
        let count = colors.len() as u8;
        // Report 0x04: [report_id, count, flags, lamp_ids(8×u16le), colors(8×{r,g,b,intensity})]
        // Total: 1 + 1 + 1 + 16 + 32 = 51 bytes
        let mut buf = [0u8; 51];
        buf[0] = REPORT_MULTI_UPDATE;
        buf[1] = count;
        buf[2] = flags;

        // Lamp IDs (u16le each, 8 slots starting at offset 3)
        for i in 0..8 {
            let id = (start_id + i) as u16;
            let offset = 3 + i * 2;
            buf[offset] = id as u8;
            buf[offset + 1] = (id >> 8) as u8;
        }

        // Colors (r, g, b, intensity for each, 8 slots starting at offset 19)
        for (i, color) in colors.iter().enumerate() {
            let offset = 19 + i * 4;
            buf[offset] = color.r;
            buf[offset + 1] = color.g;
            buf[offset + 2] = color.b;
            buf[offset + 3] = 0xFF; // intensity = max
        }

        self.device
            .send_feature_report(&buf)
            .context("Failed to send LampMultiUpdate")?;
        Ok(())
    }
}
