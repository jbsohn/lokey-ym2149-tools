use serde::{Deserialize, Serialize};

/// TIA NTSC audio clock rate in Hz (color clock / 114: ~31,440 Hz).
pub const TIA_NTSC_AUDIO_CLOCK: u32 = 31_440;

/// TIA PAL audio clock rate in Hz (~31,250 Hz).
pub const TIA_PAL_AUDIO_CLOCK: u32 = 31_250;

/// Atari 2600 NTSC 6507 CPU clock (3.579545 MHz / 3 = 1.193182 MHz).
pub const ATARI_2600_NTSC_CLOCK: u32 = 1_193_182;

/// Atari 7800 Maria/6502 master clock (1.789773 MHz).
pub const ATARI_7800_CLOCK: u32 = 1_789_773;

/// Supported refresh rate selection (50 Hz or 60 Hz).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HzOption {
    Hz50,
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

/// Supported playback refresh rates for TIA sound sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SystemHz {
    #[default]
    Hz60,
    Hz50,
    Custom(u32),
}

impl SystemHz {
    /// Returns numerical refresh rate in Hz.
    #[must_use]
    pub fn hz_value(&self) -> u32 {
        match self {
            SystemHz::Hz60 => 60,
            SystemHz::Hz50 => 50,
            SystemHz::Custom(hz) => *hz,
        }
    }

    /// Computes duration of a single frame in milliseconds.
    #[must_use]
    pub fn frame_duration_ms(&self) -> f64 {
        1000.0 / f64::from(self.hz_value().max(1))
    }
}

/// Computes 6502 delay-loop constants for hitting a target playback rate
/// on Atari 7800 / 2600 hardware.
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

/// Timing configuration for TIA sound generation and playback.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingConfig {
    pub master_clock_hz: u32,
    pub frame_rate: SystemHz,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            master_clock_hz: TIA_NTSC_AUDIO_CLOCK,
            frame_rate: SystemHz::Hz60,
        }
    }
}
