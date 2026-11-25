use crate::ayfx::AyfxFile;
use crate::error::YmError;
use crate::timing::TimingConfig;
use crate::vgm_file::VgmFile;
use crate::yfx::YfxFile;
use crate::ym_file::YmFile;
use crate::ysg::YsgFile;
use std::path::Path;

/// High-level frame representation for YM-2149 song sequences.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct YmFrame {
    pub tone_a: Option<u16>,
    pub tone_b: Option<u16>,
    pub tone_c: Option<u16>,
    pub noise_period: Option<u8>,
    pub volume_a: Option<u8>,
    pub volume_b: Option<u8>,
    pub volume_c: Option<u8>,
    pub tone_enable_a: Option<bool>,
    pub tone_enable_b: Option<bool>,
    pub tone_enable_c: Option<bool>,
    pub noise_enable_a: Option<bool>,
    pub noise_enable_b: Option<bool>,
    pub noise_enable_c: Option<bool>,
    pub envelope_period: Option<u16>,
    pub envelope_shape: Option<u8>,
}

/// Song sequence container for YM-2149 music assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YmSequence {
    pub name: String,
    pub timing: TimingConfig,
    pub loop_start: Option<usize>,
    pub frames: Vec<YmFrame>,
}

impl YmSequence {
    /// Decodes a `YmSequence` from raw bytes and file extension (`.ym`, `.vgm`, `.vgz`, or `.ysg`).
    ///
    /// # Errors
    /// Returns an error if the extension is unsupported or the file payload is malformed.
    pub fn from_bytes(
        name: &str,
        extension: &str,
        bytes: &[u8],
        clock_override: Option<u32>,
        target_clock_override: Option<u32>,
    ) -> Result<Self, YmError> {
        match extension.to_ascii_lowercase().as_str() {
            "ym" => {
                let ym = YmFile::from_bytes(name, bytes, clock_override)?;
                Ok(ym.to_sequence(target_clock_override))
            }
            "vgm" | "vgz" => {
                let vgm = VgmFile::from_bytes(name, bytes, clock_override)?;
                Ok(vgm.to_sequence(target_clock_override))
            }
            "ysg" => {
                let mut seq = YsgFile::from_bytes(bytes)?.to_sequence(name)?;
                if let Some(target_clock) = target_clock_override {
                    seq.retune_clock(target_clock);
                }
                Ok(seq)
            }
            _ => Err(YmError::UnsupportedExtension(extension.to_string())),
        }
    }

    /// Loads a `YmSequence` from a file path (`.ysg`, `.ym`, `.vgm`, or `.vgz`).
    ///
    /// # Errors
    /// Returns an error if reading the file from disk fails or decoding fails.
    pub fn from_file(
        input: impl AsRef<Path>,
        clock_override: Option<u32>,
        target_clock_override: Option<u32>,
    ) -> Result<Self, YmError> {
        let input = input.as_ref();
        let bytes = std::fs::read(input)?;
        let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");

        Self::from_bytes(name, extension, &bytes, clock_override, target_clock_override)
    }

    /// Retunes all frame periods from `self.timing.master_clock_hz` to `target_clock`.
    pub fn retune_clock(&mut self, target_clock: u32) {
        let target_clock = target_clock.max(1);
        let source_clock = self.timing.master_clock_hz.max(1);
        let ratio = f64::from(target_clock) / f64::from(source_clock);
        if (ratio - 1.0).abs() > 0.0001 {
            for frame in &mut self.frames {
                frame.scale_pitch(ratio);
            }
            self.timing.master_clock_hz = target_clock;
        }
    }

    /// Downsamples this sequence's frames by merging `step`-frame windows.
    pub fn decimate(&mut self, step: usize) {
        if step <= 1 || self.frames.is_empty() {
            return;
        }
        self.frames = YmFrame::decimate_slice(&self.frames, step);
        if let Some(loop_start) = self.loop_start {
            self.loop_start = Some(loop_start / step);
        }
    }
}

impl YmFrame {
    /// Downsamples a slice of `YmFrame`s by picking the peak volume and active tone
    /// for each channel across each `step`-frame window.
    pub fn decimate_slice(frames: &[Self], step: usize) -> Vec<Self> {
        if step <= 1 {
            return frames.to_vec();
        }
        let limit = frames.len();
        let mut decimated = Vec::with_capacity(limit.div_ceil(step));
        let mut i = 0;
        while i < limit {
            let window_end = (i + step).min(limit);
            let mut best_idx_a = i;
            let mut best_vol_a = frames[i].volume_a.unwrap_or(0);
            let mut best_idx_b = i;
            let mut best_vol_b = frames[i].volume_b.unwrap_or(0);
            let mut best_idx_c = i;
            let mut best_vol_c = frames[i].volume_c.unwrap_or(0);

            for (idx, f) in frames.iter().enumerate().take(window_end).skip(i) {
                let v_a = f.volume_a.unwrap_or(0);
                if v_a > best_vol_a {
                    best_vol_a = v_a;
                    best_idx_a = idx;
                }
                let v_b = f.volume_b.unwrap_or(0);
                if v_b > best_vol_b {
                    best_vol_b = v_b;
                    best_idx_b = idx;
                }
                let v_c = f.volume_c.unwrap_or(0);
                if v_c > best_vol_c {
                    best_vol_c = v_c;
                    best_idx_c = idx;
                }
            }

            let dominant_idx = if best_vol_a >= best_vol_b && best_vol_a >= best_vol_c {
                best_idx_a
            } else if best_vol_b >= best_vol_c {
                best_idx_b
            } else {
                best_idx_c
            };

            let mut final_frame = frames[i].clone();
            final_frame.volume_a = frames[best_idx_a].volume_a;
            final_frame.tone_a = frames[best_idx_a].tone_a;
            final_frame.tone_enable_a = frames[best_idx_a].tone_enable_a;
            final_frame.noise_enable_a = frames[best_idx_a].noise_enable_a;

            final_frame.volume_b = frames[best_idx_b].volume_b;
            final_frame.tone_b = frames[best_idx_b].tone_b;
            final_frame.tone_enable_b = frames[best_idx_b].tone_enable_b;
            final_frame.noise_enable_b = frames[best_idx_b].noise_enable_b;

            final_frame.volume_c = frames[best_idx_c].volume_c;
            final_frame.tone_c = frames[best_idx_c].tone_c;
            final_frame.tone_enable_c = frames[best_idx_c].tone_enable_c;
            final_frame.noise_enable_c = frames[best_idx_c].noise_enable_c;

            final_frame.noise_period = frames[dominant_idx].noise_period;
            final_frame.envelope_period = frames[dominant_idx].envelope_period;
            final_frame.envelope_shape = frames[dominant_idx].envelope_shape;

            decimated.push(final_frame);
            i += step;
        }
        decimated
    }

    /// Writes frame register values directly to a YM-2149 chip backend.
    ///
    /// Envelope shape (R13) is written whenever `envelope_shape` is `Some` — including when the
    /// value matches the previous frame, because any write to R13 resets the hardware envelope
    /// phase. `None` means the original data used the 0xFF sentinel (no write this frame).
    pub fn apply_to_chip(
        &self,
        chip: &mut impl ym2149::Ym2149Backend,
        mixer: &mut u8,
        last_env_shape: &mut Option<u8>,
    ) {
        // Tone A (R0, R1)
        if let Some(tone) = self.tone_a {
            chip.write_register(0, (tone & 0xFF) as u8);
            chip.write_register(1, ((tone >> 8) & 0x0F) as u8);
        }
        // Tone B (R2, R3)
        if let Some(tone) = self.tone_b {
            chip.write_register(2, (tone & 0xFF) as u8);
            chip.write_register(3, ((tone >> 8) & 0x0F) as u8);
        }
        // Tone C (R4, R5)
        if let Some(tone) = self.tone_c {
            chip.write_register(4, (tone & 0xFF) as u8);
            chip.write_register(5, ((tone >> 8) & 0x0F) as u8);
        }
        // Noise Period (R6)
        if let Some(noise) = self.noise_period {
            chip.write_register(6, noise & 0x1F);
        }
        // Volume A, B, C (R8, R9, R10)
        if let Some(vol) = self.volume_a {
            chip.write_register(8, vol & 0x1F);
        }
        if let Some(vol) = self.volume_b {
            chip.write_register(9, vol & 0x1F);
        }
        if let Some(vol) = self.volume_c {
            chip.write_register(10, vol & 0x1F);
        }

        // Mixer Enable bits (R7) - 0 is ENABLED, 1 is DISABLED
        if let Some(en) = self.tone_enable_a {
            if en {
                *mixer &= !0x01;
            } else {
                *mixer |= 0x01;
            }
        }
        if let Some(en) = self.tone_enable_b {
            if en {
                *mixer &= !0x02;
            } else {
                *mixer |= 0x02;
            }
        }
        if let Some(en) = self.tone_enable_c {
            if en {
                *mixer &= !0x04;
            } else {
                *mixer |= 0x04;
            }
        }
        if let Some(en) = self.noise_enable_a {
            if en {
                *mixer &= !0x08;
            } else {
                *mixer |= 0x08;
            }
        }
        if let Some(en) = self.noise_enable_b {
            if en {
                *mixer &= !0x10;
            } else {
                *mixer |= 0x10;
            }
        }
        if let Some(en) = self.noise_enable_c {
            if en {
                *mixer &= !0x20;
            } else {
                *mixer |= 0x20;
            }
        }
        chip.write_register(7, *mixer);

        // Envelope Period (R11, R12)
        if let Some(period) = self.envelope_period {
            chip.write_register(11, (period & 0xFF) as u8);
            chip.write_register(12, ((period >> 8) & 0xFF) as u8);
        }

        // Envelope Shape (R13): written whenever envelope_shape is Some, resetting phase.
        if let Some(shape) = self.envelope_shape {
            let shape_val = shape & 0x0F;
            chip.write_register(13, shape_val);
            *last_env_shape = Some(shape_val);
        }
    }

    /// Scales tone, noise, and envelope periods by a clock ratio safely.
    pub fn scale_pitch(&mut self, ratio: f64) {
        let ratio = if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else {
            1.0
        };
        if let Some(t) = self.tone_a {
            self.tone_a = Some((f64::from(t) * ratio).round().clamp(0.0, 4095.0) as u16);
        }
        if let Some(t) = self.tone_b {
            self.tone_b = Some((f64::from(t) * ratio).round().clamp(0.0, 4095.0) as u16);
        }
        if let Some(t) = self.tone_c {
            self.tone_c = Some((f64::from(t) * ratio).round().clamp(0.0, 4095.0) as u16);
        }
        if let Some(n) = self.noise_period {
            self.noise_period = Some((f64::from(n & 0x1F) * ratio).round().clamp(0.0, 31.0) as u8);
        }
        if let Some(e) = self.envelope_period {
            self.envelope_period = Some((f64::from(e) * ratio).round().clamp(0.0, 65535.0) as u16);
        }
    }
}

/// Sound channel selector for routing dynamic SFX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YmChannel {
    A,
    B,
    C,
}

impl YmChannel {
    /// Returns 0-based channel index: 0 for A, 1 for B, 2 for C.
    pub const fn to_index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
        }
    }

    /// Converts a 0-based channel index into a `YmChannel` (values >= 2 map to C).
    pub const fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::A,
            1 => Self::B,
            _ => Self::C,
        }
    }
}

impl From<YmChannel> for usize {
    fn from(ch: YmChannel) -> Self {
        ch.to_index()
    }
}

impl From<usize> for YmChannel {
    fn from(idx: usize) -> Self {
        Self::from_index(idx)
    }
}

/// Single-channel YM-2149 sound effect frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxFrame {
    pub tone_enable: bool,
    pub noise_enable: bool,
    pub tone: u16,
    pub noise: u8,
    pub volume: u8,
    /// Number of playback ticks to hold these register values (0 is treated as 1).
    pub duration: u8,
}

impl Default for SfxFrame {
    fn default() -> Self {
        Self::new(false, false, 0, 0, 0, 1)
    }
}

impl SfxFrame {
    pub const fn new(
        tone_enable: bool,
        noise_enable: bool,
        tone: u16,
        noise: u8,
        volume: u8,
        duration: u8,
    ) -> Self {
        Self {
            tone_enable,
            noise_enable,
            tone,
            noise,
            volume,
            duration,
        }
    }

    /// Number of physical playback ticks this frame occupies (at least 1).
    pub const fn tick_count(self) -> u8 {
        if self.duration == 0 {
            1
        } else {
            self.duration
        }
    }

    /// Writes SFX frame values to a specific YM-2149 audio channel.
    pub fn apply_to_chip(
        self,
        chip: &mut impl ym2149::Ym2149Backend,
        mixer: &mut u8,
        channel: YmChannel,
    ) {
        let tone_reg_low = match channel {
            YmChannel::A => 0,
            YmChannel::B => 2,
            YmChannel::C => 4,
        };
        chip.write_register(tone_reg_low, (self.tone & 0xFF) as u8);
        chip.write_register(tone_reg_low + 1, ((self.tone >> 8) & 0x0F) as u8);

        chip.write_register(6, self.noise & 0x1F);

        let vol_reg = match channel {
            YmChannel::A => 8,
            YmChannel::B => 9,
            YmChannel::C => 10,
        };
        chip.write_register(vol_reg, self.volume & 0x1F);

        let tone_bit = match channel {
            YmChannel::A => 0x01,
            YmChannel::B => 0x02,
            YmChannel::C => 0x04,
        };
        let noise_bit = match channel {
            YmChannel::A => 0x08,
            YmChannel::B => 0x10,
            YmChannel::C => 0x20,
        };

        if self.tone_enable {
            *mixer &= !tone_bit;
        } else {
            *mixer |= tone_bit;
        }
        if self.noise_enable {
            *mixer &= !noise_bit;
        } else {
            *mixer |= noise_bit;
        }
        chip.write_register(7, *mixer);
    }
}

/// Single-channel YM-2149 sound effect sequence (register frames only).
///
/// Playback details (chip clock, frame rate, YM channel) are chosen by the
/// player/driver at playback time rather than stored in the sound asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfxSequence {
    pub name: String,
    pub frames: Vec<SfxFrame>,
}

impl SfxSequence {
    /// Creates a new sound effect sequence.
    pub fn new(name: impl Into<String>, frames: Vec<SfxFrame>) -> Self {
        Self {
            name: name.into(),
            frames,
        }
    }

    /// Decodes one or more `SfxSequence`s from raw bytes and extension (`.csv`, `.afx`, `.afb`, or `.yfx`).
    ///
    /// # Errors
    /// Returns an error if the extension is unsupported or the payload is malformed.
    pub fn from_bytes(name: &str, extension: &str, bytes: &[u8]) -> Result<Vec<Self>, YmError> {
        match extension.to_ascii_lowercase().as_str() {
            "csv" => {
                let text = std::str::from_utf8(bytes)?;
                Ok(AyfxFile::from_csv(name, text)?.sequences)
            }
            "afb" => Ok(AyfxFile::from_bank(bytes)?.sequences),
            "afx" => Ok(AyfxFile::from_effect(name, bytes)?.sequences),
            "yfx" => Ok(vec![YfxFile::from_bytes(bytes)?.to_sequence(name)]),
            _ => Err(YmError::UnsupportedExtension(extension.to_string())),
        }
    }

    /// Loads a single `SfxSequence` from a file path (`.yfx`, `.csv`, `.afx`, or `.afb`).
    ///
    /// # Errors
    /// Returns an error if reading/decoding fails or `bank_index` is out of bounds.
    pub fn from_file(input: impl AsRef<Path>, bank_index: usize) -> Result<Self, YmError> {
        let mut sequences = Self::load_all_from_paths(&[input.as_ref()])?;
        if bank_index >= sequences.len() {
            return Err(YmError::BankIndexOutOfBounds {
                index: bank_index,
                count: sequences.len(),
            });
        }
        Ok(sequences.swap_remove(bank_index))
    }

    /// Loads all `SfxSequence`s from a list of file paths (`.yfx`, `.csv`, `.afx`, or `.afb`).
    ///
    /// # Errors
    /// Returns an error if reading or decoding any file fails, or if no effects are loaded.
    pub fn load_all_from_paths(inputs: &[impl AsRef<Path>]) -> Result<Vec<Self>, YmError> {
        let mut sequences = Vec::new();
        for input in inputs {
            let input = input.as_ref();
            let bytes = std::fs::read(input)?;
            let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("sfx");

            sequences.extend(Self::from_bytes(name, extension, &bytes)?);
        }
        if sequences.is_empty() {
            return Err(YmError::NoSfxLoaded);
        }
        Ok(sequences)
    }
}
