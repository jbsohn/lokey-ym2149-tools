/// Atari 7800 YM-2149 master clock in Hz (1.789773 MHz).
pub const ATARI_7800_CLOCK: u32 = 1_789_773;
/// Atari ST YM-2149 master clock in Hz (2.0 MHz).
pub const ATARI_ST_CLOCK: u32 = 2_000_000;
/// ZX Spectrum AY-3-8912 master clock in Hz (1.7734 MHz).
pub const ZX_SPECTRUM_CLOCK: u32 = 1_773_400;
/// Apple II Mockingboard AY-3-8910 master clock in Hz (~1.020484 MHz).
pub const APPLE2_CLOCK: u32 = 1_020_484;

/// Timing configuration for YM-2149 sound generation and playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimingConfig {
    pub master_clock_hz: u32,
    pub frame_rate: u32,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self::ATARI_7800
    }
}

impl TimingConfig {
    /// Atari 7800 NTSC default (1.789773 MHz clock, 60 Hz frame rate).
    pub const ATARI_7800: Self = Self {
        master_clock_hz: ATARI_7800_CLOCK,
        frame_rate: 60,
    };

    /// Atari ST default (2.0 MHz clock, 50 Hz frame rate).
    pub const ATARI_ST: Self = Self {
        master_clock_hz: ATARI_ST_CLOCK,
        frame_rate: 50,
    };

    /// ZX Spectrum / `AYFXEdit` default (1.7734 MHz clock, 50 Hz frame rate).
    pub const ZX_SPECTRUM: Self = Self {
        master_clock_hz: ZX_SPECTRUM_CLOCK,
        frame_rate: 50,
    };

    /// Apple II Mockingboard default (~1.020484 MHz clock, 50 Hz frame rate).
    pub const APPLE2: Self = Self {
        master_clock_hz: APPLE2_CLOCK,
        frame_rate: 50,
    };

    /// Creates a new timing configuration.
    pub const fn new(master_clock_hz: u32, frame_rate: u32) -> Self {
        Self {
            master_clock_hz,
            frame_rate,
        }
    }

    /// Computes duration of a single frame in milliseconds.
    pub fn frame_duration_ms(self) -> f64 {
        1000.0 / f64::from(self.frame_rate.max(1))
    }

    /// Host PCM samples per playback frame (at least 1).
    pub fn samples_per_frame(self, output_sample_rate: u32) -> usize {
        let hz = self.frame_rate.max(1);
        ((f64::from(output_sample_rate) / f64::from(hz)).round() as usize).max(1)
    }

    /// Computes 6502 PHI2 busy-wait delay-loop constants for hitting this refresh rate
    /// on real Atari 7800 hardware (1.789773 MHz clock).
    pub fn calculate_delay(self) -> (u32, u8) {
        Self::calculate_delay_for_hz(self.frame_rate)
    }

    /// Computes 6502 PHI2 busy-wait delay-loop constants for hitting `hz`
    /// on real Atari 7800 hardware (1.789773 MHz clock): an outer
    /// loop count (`y`, ~1285 cycles/iteration) and a fine-tune inner loop count
    /// (`x`, ~5 cycles/iteration), after subtracting a fixed ~1800-cycle
    /// per-frame processing overhead.
    pub fn calculate_delay_for_hz(hz: u32) -> (u32, u8) {
        let hz_valid = hz.max(1);
        let remaining = (f64::from(ATARI_7800_CLOCK) / f64::from(hz_valid) - 1800.0).max(0.0);
        let y_raw = (remaining / 1285.0).floor();
        let x = ((remaining - y_raw * 1285.0) / 5.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        let y = (y_raw as u32).max(1);
        (y, x)
    }
}
