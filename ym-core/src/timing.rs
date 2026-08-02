use serde::{Deserialize, Serialize};

pub const ATARI_7800_CLOCK: u32 = 1_789_773;
pub const ATARI_ST_CLOCK: u32 = 2_000_000;
pub const ZX_SPECTRUM_CLOCK: u32 = 1_773_400;

/// Shared CLI value enum for Hz selection, usable by both ym-sfx and ym-song.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum HzOption {
    #[value(name = "50")]
    Hz50,
    #[value(name = "60")]
    Hz60,
}

impl From<HzOption> for SystemHz {
    fn from(opt: HzOption) -> Self {
        match opt {
            HzOption::Hz50 => SystemHz::Hz50,
            HzOption::Hz60 => SystemHz::Hz60,
        }
    }
}

/// Supported playback refresh rates for YM-2149 sound sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SystemHz {
    #[default]
    Hz50,
    Hz60,
    Custom(u32),
}

impl SystemHz {
    /// Returns numerical refresh rate in Hz.
    #[must_use]
    pub fn hz_value(&self) -> u32 {
        match self {
            SystemHz::Hz50 => 50,
            SystemHz::Hz60 => 60,
            SystemHz::Custom(hz) => *hz,
        }
    }

    /// Computes duration of a single frame in milliseconds.
    #[must_use]
    pub fn frame_duration_ms(&self) -> f64 {
        1000.0 / f64::from(self.hz_value().max(1))
    }
}

/// Computes 6502 PHI2 busy-wait delay-loop constants for hitting a target
/// playback rate on real Atari 7800 hardware (1.789773 MHz clock): an outer
/// loop count (`y`, ~1285 cycles/iteration) and a fine-tune inner loop count
/// (`x`, ~5 cycles/iteration), after subtracting a fixed ~1800-cycle
/// per-frame processing overhead. Ported from the original C# player-tuning
/// tool's `CalculateDelay`.
#[must_use]
pub fn calculate_delay(hz: u32) -> (u32, u8) {
    let hz_valid = hz.max(1);
    let remaining = (f64::from(ATARI_7800_CLOCK) / f64::from(hz_valid) - 1800.0).max(0.0);
    let y_raw = (remaining / 1285.0).floor();
    let x = ((remaining - y_raw * 1285.0) / 5.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    let y = (y_raw as u32).max(1);
    (y, x)
}

/// Timing configuration for YM-2149 sound generation and playback.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingConfig {
    pub master_clock_hz: u32,
    pub frame_rate: SystemHz,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            master_clock_hz: ATARI_7800_CLOCK, // Atari 7800 YM-2149 clock (1.789773 MHz)
            frame_rate: SystemHz::Hz60,
        }
    }
}
