// ============================================================================
// Atari TIA (Television Interface Adapter) Sound Chip Emulation
//
// Adapted from the open-source TIASound JavaScript emulator:
//   Author: Fabio Cardoso (https://github.com/fabiopiratininga)
//   Repository: https://github.com/fabiopiratininga/TIASound
//   License: MIT License - Copyright (c) 2025 Fabio Cardoso
// ============================================================================

#![allow(clippy::similar_names)]

use crate::timing::TIA_NTSC_AUDIO_CLOCK;
use serde::{Deserialize, Serialize};

/// TIA channel index (Channel 0 or Channel 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TiaChannel {
    #[serde(rename = "0", alias = "ch0", alias = "A", alias = "a")]
    Ch0,
    #[serde(rename = "1", alias = "ch1", alias = "B", alias = "b")]
    Ch1,
}

impl TiaChannel {
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            TiaChannel::Ch0 => 0,
            TiaChannel::Ch1 => 1,
        }
    }

    #[must_use]
    pub fn from_index(idx: usize) -> Self {
        if idx == 0 {
            TiaChannel::Ch0
        } else {
            TiaChannel::Ch1
        }
    }
}

/// Named TIA sound waveform types (mapped to AUDC values 0..=15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TiaSoundType {
    Saw,     // AUDC = 1
    Engine,  // AUDC = 3
    Square,  // AUDC = 4
    Bass,    // AUDC = 6
    Pitfall, // AUDC = 7
    Noise,   // AUDC = 8
    Lead,    // AUDC = 12
    Buzz,    // AUDC = 15
    Custom(u8),
}

impl TiaSoundType {
    /// Converts a sound type identifier or name into its AUDC numeric value (0..=15).
    #[must_use]
    pub fn audc_value(self) -> u8 {
        match self {
            TiaSoundType::Saw => 1,
            TiaSoundType::Engine => 3,
            TiaSoundType::Square => 4,
            TiaSoundType::Bass => 6,
            TiaSoundType::Pitfall => 7,
            TiaSoundType::Noise => 8,
            TiaSoundType::Lead => 12,
            TiaSoundType::Buzz => 15,
            TiaSoundType::Custom(val) => val & 0x0F,
        }
    }

    /// Parses a named sound type string or returns Custom(0) if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_lowercase().as_str() {
            "saw" => TiaSoundType::Saw,
            "engine" => TiaSoundType::Engine,
            "square" => TiaSoundType::Square,
            "bass" => TiaSoundType::Bass,
            "pitfall" => TiaSoundType::Pitfall,
            "noise" => TiaSoundType::Noise,
            "lead" => TiaSoundType::Lead,
            "buzz" => TiaSoundType::Buzz,
            _ => {
                if let Ok(val) = name.trim().parse::<u8>() {
                    Self::from_audc(val)
                } else {
                    TiaSoundType::Custom(0)
                }
            }
        }
    }

    /// Converts an AUDC integer (0..=15) to its canonical named type or Custom.
    #[must_use]
    pub fn from_audc(audc: u8) -> Self {
        match audc & 0x0F {
            1 => TiaSoundType::Saw,
            3 => TiaSoundType::Engine,
            4 => TiaSoundType::Square,
            6 => TiaSoundType::Bass,
            7 => TiaSoundType::Pitfall,
            8 => TiaSoundType::Noise,
            12 => TiaSoundType::Lead,
            15 => TiaSoundType::Buzz,
            other => TiaSoundType::Custom(other),
        }
    }

    /// Returns the descriptive name for this sound type.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            TiaSoundType::Saw => "saw",
            TiaSoundType::Engine => "engine",
            TiaSoundType::Square => "square",
            TiaSoundType::Bass => "bass",
            TiaSoundType::Pitfall => "pitfall",
            TiaSoundType::Noise => "noise",
            TiaSoundType::Lead => "lead",
            TiaSoundType::Buzz => "buzz",
            TiaSoundType::Custom(_) => "custom",
        }
    }
}

// Polynomial run-length tables (P0 through P7) matching original TIA hardware bit shifts
static P0: &[i16] = &[1, -1];
static P1: &[i16] = &[1, 1, -1];
static P2: &[i16] = &[16, 15, -1];
static P3: &[i16] = &[1, 2, 2, 1, 1, 1, 4, 3, -1];
static P4: &[i16] = &[1, 2, 1, 1, 2, 2, 5, 4, 2, 1, 3, 1, 1, 1, 1, 4, -1];
static P5: &[i16] = &[
    1, 4, 1, 3, 2, 4, 1, 2, 3, 2, 1, 1, 1, 1, 1, 1, 2, 4, 2, 1, 4, 1, 1, 2, 2, 1, 3, 2, 1, 3, 1, 1,
    1, 4, 1, 1, 1, 1, 2, 1, 1, 2, 6, 1, 2, 2, 1, 2, 1, 2, 1, 1, 2, 1, 6, 2, 1, 2, 2, 1, 1, 1, 1, 2,
    2, 2, 2, 7, 2, 3, 2, 2, 1, 1, 1, 3, 2, 1, 1, 2, 1, 1, 7, 1, 1, 3, 1, 1, 2, 3, 3, 1, 1, 1, 2, 2,
    1, 1, 2, 2, 4, 3, 5, 1, 3, 1, 1, 5, 2, 1, 1, 1, 2, 1, 2, 1, 3, 1, 2, 5, 1, 1, 2, 1, 1, 1, 5, 1,
    1, 1, 1, 1, 1, 1, 1, 6, 1, 1, 1, 2, 1, 1, 1, 1, 4, 2, 1, 1, 3, 1, 3, 6, 3, 2, 3, 1, 1, 2, 1, 2,
    4, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1, 2, 1, 4, 2, 2, 3, 4, 1, 1, 4, 1, 2, 1, 2, 2, 2, 1, 1, 4, 3, 1,
    4, 4, 9, 5, 4, 1, 5, 3, 1, 1, 3, 2, 2, 2, 1, 5, 1, 2, 1, 1, 1, 2, 3, 1, 2, 1, 1, 3, 4, 2, 5, 2,
    2, 1, 2, 3, 1, 1, 1, 1, 1, 2, 1, 3, 3, 3, 2, 1, 2, 1, 1, 1, 1, 1, 3, 3, 1, 2, 2, 3, 1, 3, 1, 8,
    -1,
];
static P6: &[i16] = &[5, 6, 4, 5, 10, 5, 3, 7, 4, 10, 6, 3, 6, 4, 9, 6, -1];
static P7: &[i16] = &[
    2, 3, 2, 1, 4, 1, 6, 10, 2, 4, 2, 1, 1, 4, 5, 9, 3, 3, 4, 1, 1, 1, 8, 5, 5, 5, 4, 1, 1, 1, 8,
    4, 2, 8, 3, 3, 1, 1, 7, 4, 2, 7, 5, 1, 3, 1, 7, 4, 1, 4, 8, 2, 1, 3, 4, 7, 1, 3, 7, 3, 2, 1, 6,
    6, 2, 2, 4, 5, 3, 2, 6, 6, 1, 3, 3, 2, 5, 3, 7, 3, 4, 3, 2, 2, 2, 5, 9, 3, 1, 5, 3, 1, 2, 2,
    11, 5, 1, 5, 3, 1, 1, 2, 12, 5, 1, 2, 5, 2, 1, 1, 12, 6, 1, 2, 5, 1, 2, 1, 10, 6, 3, 2, 2, 4,
    1, 2, 6, 10, -1,
];

/// Polynomial array mapped by AUDC register (0..=15).
pub static POLYS: &[&[i16]; 16] = &[
    P0, P3, P3, P7, P1, P1, P2, P4, P5, P4, P2, P0, P1, P1, P2, P6,
];

/// Frequency divisors mapped by AUDC register (0..=15).
pub static DIVISORS: &[u32; 16] = &[1, 1, 15, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 3, 1];

/// Internal emulation state of a single TIA sound channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiaChannelState {
    pub audv: u8,
    pub audc: u8,
    pub audf: u8,
    pub offset: usize,
    pub count: usize,
    pub last: u8,
    pub f: u32,
    pub rate_accum: u32,
}

impl Default for TiaChannelState {
    fn default() -> Self {
        Self::new()
    }
}

impl TiaChannelState {
    /// Creates a reset channel state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            audv: 0,
            audc: 0,
            audf: 0,
            offset: 0,
            count: 0,
            last: 1,
            f: 0,
            rate_accum: 0,
        }
    }

    /// Resets internal synthesis counters while maintaining register values.
    pub fn reset_counters(&mut self) {
        self.offset = 0;
        self.count = 0;
        self.last = 1;
        self.f = 0;
        self.rate_accum = 0;
    }

    /// Resets all registers and counters to zero.
    pub fn reset_all(&mut self) {
        self.audv = 0;
        self.audc = 0;
        self.audf = 0;
        self.reset_counters();
    }

    /// Sets the AUDC (Audio Control / Waveform) register (0..=15).
    pub fn set_audc(&mut self, val: u8) {
        self.reset_counters();
        self.audc = val.min(15);
    }

    /// Sets the AUDF (Audio Frequency) register (0..=31).
    pub fn set_audf(&mut self, val: u8) {
        self.reset_counters();
        self.audf = val.min(31);
    }

    /// Sets the AUDV (Audio Volume) register (0..=15).
    pub fn set_audv(&mut self, val: u8) {
        self.audv = val.min(15);
    }

    /// Sets all 3 registers at once.
    pub fn set_registers(&mut self, audf: u8, audc: u8, audv: u8) {
        self.reset_counters();
        self.audf = audf.min(31);
        self.audc = audc.min(15);
        self.audv = audv.min(15);
    }

    /// Advances the channel by one TIA audio clock tick (31.44 kHz).
    pub fn step_tia_clock(&mut self) {
        let divisor = DIVISORS[(self.audc & 0x0F) as usize] * (u32::from(self.audf & 0x1F) + 1);
        self.f += 1;
        if self.f >= divisor {
            let poly = POLYS[(self.audc & 0x0F) as usize];
            self.f = 0;
            self.count += 1;
            let target_len = poly[self.offset];
            if target_len > 0 && self.count >= target_len as usize {
                self.offset += 1;
                self.count = 0;
                if poly[self.offset] == -1 {
                    self.offset = 0;
                }
            }
            self.last = u8::from(self.offset.is_multiple_of(2));
        }
    }

    /// Returns the current raw floating-point amplitude (0.0 to 0.5) based on current state and AUDV.
    #[must_use]
    pub fn current_sample_level(&self) -> f32 {
        let vol = f32::from(self.audv & 0x0F) / 30.0;
        f32::from(self.last) * vol
    }

    /// Generates one resampled output sample by clocking the TIA according to the rate ratio.
    pub fn step_and_render_sample(&mut self, tia_rate: u32, output_sample_rate: u32) -> f32 {
        self.rate_accum += tia_rate;
        while self.rate_accum >= output_sample_rate {
            self.step_tia_clock();
            self.rate_accum -= output_sample_rate;
        }
        self.current_sample_level()
    }
}

/// Dual-channel Atari TIA sound chip emulator.
#[derive(Debug, Clone)]
pub struct TiaChip {
    pub channels: [TiaChannelState; 2],
    pub tia_clock_hz: u32,
    pub output_sample_rate: u32,
    pub muted: [bool; 2],
}

impl Default for TiaChip {
    fn default() -> Self {
        Self::new(TIA_NTSC_AUDIO_CLOCK, 48_000)
    }
}

impl TiaChip {
    /// Creates a new TIA sound chip emulator instance with specified clocks.
    #[must_use]
    pub fn new(tia_clock_hz: u32, output_sample_rate: u32) -> Self {
        Self {
            channels: [TiaChannelState::new(), TiaChannelState::new()],
            tia_clock_hz: tia_clock_hz.max(1),
            output_sample_rate: output_sample_rate.max(1),
            muted: [false, false],
        }
    }

    /// Resets both channels and mute flags.
    pub fn reset(&mut self) {
        self.channels[0].reset_all();
        self.channels[1].reset_all();
        self.muted = [false, false];
    }

    /// Sets AUDC (Audio Control / Waveform) for a channel.
    pub fn set_audc(&mut self, ch: TiaChannel, val: u8) {
        self.channels[ch.index()].set_audc(val);
    }

    /// Sets AUDF (Audio Frequency) for a channel.
    pub fn set_audf(&mut self, ch: TiaChannel, val: u8) {
        self.channels[ch.index()].set_audf(val);
    }

    /// Sets AUDV (Audio Volume) for a channel.
    pub fn set_audv(&mut self, ch: TiaChannel, val: u8) {
        self.channels[ch.index()].set_audv(val);
    }

    /// Configures all 3 registers for a channel.
    pub fn set_channel(&mut self, ch: TiaChannel, audf: u8, audc: u8, audv: u8) {
        self.channels[ch.index()].set_registers(audf, audc, audv);
    }

    /// Configures channel parameters using a named waveform type.
    pub fn set_channel_by_name(&mut self, ch: TiaChannel, audf: u8, sound_type: &str, audv: u8) {
        let audc = TiaSoundType::from_name(sound_type).audc_value();
        self.set_channel(ch, audf, audc, audv);
    }

    /// Sets mute state for a channel.
    pub fn set_muted(&mut self, ch: TiaChannel, muted: bool) {
        self.muted[ch.index()] = muted;
    }

    /// Ticks both channels by one TIA clock cycle (unresampled).
    pub fn clock(&mut self) {
        self.channels[0].step_tia_clock();
        self.channels[1].step_tia_clock();
    }

    /// Produces one mono mixed sample resampled to `output_sample_rate`.
    pub fn get_sample(&mut self) -> f32 {
        let s0 = if self.muted[0] {
            self.channels[0].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate);
            0.0
        } else {
            self.channels[0].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate)
        };

        let s1 = if self.muted[1] {
            self.channels[1].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate);
            0.0
        } else {
            self.channels[1].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate)
        };

        s0 + s1
    }

    /// Produces one stereo sample pair (channel 0 left, channel 1 right) resampled to `output_sample_rate`.
    pub fn get_stereo_sample(&mut self) -> (f32, f32) {
        let s0 = if self.muted[0] {
            self.channels[0].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate);
            0.0
        } else {
            self.channels[0].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate)
        };

        let s1 = if self.muted[1] {
            self.channels[1].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate);
            0.0
        } else {
            self.channels[1].step_and_render_sample(self.tia_clock_hz, self.output_sample_rate)
        };

        (s0, s1)
    }

    /// Renders PCM float samples into an interleaved output buffer.
    pub fn render_samples(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 1 {
            for sample in buffer.iter_mut() {
                *sample = self.get_sample();
            }
        } else if channels == 2 {
            let mut i = 0;
            while i < buffer.len() {
                let (l, r) = self.get_stereo_sample();
                buffer[i] = l;
                if i + 1 < buffer.len() {
                    buffer[i + 1] = r;
                }
                i += 2;
            }
        } else {
            let mut i = 0;
            while i < buffer.len() {
                let mono = self.get_sample();
                for c in 0..channels {
                    if i + c < buffer.len() {
                        buffer[i + c] = mono;
                    }
                }
                i += channels;
            }
        }
    }
}
