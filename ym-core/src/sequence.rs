use crate::timing::TimingConfig;
use serde::{Deserialize, Serialize};

/// High-level frame representation for YM-2149 sound sequence authoring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
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
    pub duration: Option<u8>,
}

/// Sound sequence manifest container for YM-2149 assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YmSequence {
    pub name: String,
    pub timing: TimingConfig,
    pub priority: u8,
    pub loop_start: Option<usize>,
    pub frames: Vec<YmFrame>,
}

impl YmSequence {
    /// Deserializes a compiled .ysg binary stream into a `YmSequence`.
    ///
    /// # Errors
    /// Returns an error if header validation fails or pattern payloads are corrupted.
    pub fn from_ysg(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        use crate::traits::SongFile;
        use crate::ysg::YsgFile;

        let file = YsgFile::from_bytes(bytes)?;
        file.to_sequence(name)
    }

    /// Loads a `YmSequence` from a file path (.ysg, .ym, or .json).
    ///
    /// # Errors
    /// Returns an error if reading the target file from disk fails, or if decoding the
    /// sequence format fails.
    pub fn load_from_path(
        input: &std::path::Path,
        clock_override: Option<u32>,
        target_clock_override: Option<u32>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use crate::container::SongContainer;

        let bytes = std::fs::read(input)?;
        let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");

        let container = SongContainer::from_bytes(name, extension, &bytes, clock_override)?;
        container.to_sequence(name, target_clock_override)
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
    #[must_use]
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

        // Envelope Shape (R13): only write when the value changes.
        // Same-value writes retrigger the envelope phase, causing audible clicks when the
        // composer repeats R13 to mark phrase boundaries rather than to intentionally reset.
        if let Some(shape) = self.envelope_shape {
            let shape_val = shape & 0x0F;
            if *last_env_shape != Some(shape_val) {
                chip.write_register(13, shape_val);
                *last_env_shape = Some(shape_val);
            }
        }
    }

    /// Scales tone and noise pitch periods by clock ratio safely.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum YmChannel {
    A,
    B,
    C,
}

impl YmChannel {
    /// Returns 0-based channel index: 0 for A, 1 for B, 2 for C.
    #[must_use]
    pub const fn to_index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
        }
    }

    /// Converts a 0-based channel index into a `YmChannel` (values >= 2 map to C).
    #[must_use]
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

/// Single-channel Sound Effect Frame, matching the validation schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SfxFrame {
    pub tone_enable: Option<bool>,
    pub noise_enable: Option<bool>,
    pub tone: Option<u16>,
    pub noise: Option<u8>,
    pub volume: Option<u8>,
    pub duration: Option<u8>,
}

impl SfxFrame {
    #[must_use]
    pub fn new(
        tone_enable: bool,
        noise_enable: bool,
        tone: u16,
        noise: u8,
        volume: u8,
        duration: u8,
    ) -> Self {
        Self {
            tone_enable: Some(tone_enable),
            noise_enable: Some(noise_enable),
            tone: Some(tone),
            noise: Some(noise),
            volume: Some(volume),
            duration: Some(duration),
        }
    }
}

/// Channel-agnostic Sound Effect manifest matching sfx-schema.json.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SfxSequence {
    pub name: String,
    pub source_clock: u32,
    pub source_hz: u32,
    pub priority: u8,
    pub preferred_channels: Option<Vec<YmChannel>>,
    pub loop_start: Option<usize>,
    pub frames: Vec<SfxFrame>,
}

impl SfxFrame {
    /// Writes SFX frame values to a specific YM-2149 audio channel.
    pub fn apply_to_chip(
        &self,
        chip: &mut impl ym2149::Ym2149Backend,
        mixer: &mut u8,
        channel: YmChannel,
    ) {
        let tone_reg_low = match channel {
            YmChannel::A => 0,
            YmChannel::B => 2,
            YmChannel::C => 4,
        };
        let tone_reg_high = tone_reg_low + 1;

        if let Some(t) = self.tone {
            chip.write_register(tone_reg_low, (t & 0xFF) as u8);
            chip.write_register(tone_reg_high, ((t >> 8) & 0x0F) as u8);
        }

        if let Some(n) = self.noise {
            chip.write_register(6, n & 0x1F);
        }

        let vol_reg = match channel {
            YmChannel::A => 8,
            YmChannel::B => 9,
            YmChannel::C => 10,
        };

        if let Some(v) = self.volume {
            chip.write_register(vol_reg, v & 0x1F);
        }

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

        if let Some(en) = self.tone_enable {
            if en {
                *mixer &= !tone_bit;
            } else {
                *mixer |= tone_bit;
            }
        }
        if let Some(en) = self.noise_enable {
            if en {
                *mixer &= !noise_bit;
            } else {
                *mixer |= noise_bit;
            }
        }
        chip.write_register(7, *mixer);
    }
}

impl SfxSequence {
    /// Parses an AYFX CSV text export into an `SfxSequence`.
    ///
    /// # Errors
    /// Returns an error if CSV parsing fails or mandatory column fields are missing.
    pub fn from_ayfx_csv(name: &str, content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        use crate::ayfx::AyfxFile;
        use crate::traits::SfxInput;

        let file = AyfxFile::from_csv(name, content)?;
        let mut seqs = file.to_sequences()?;
        seqs.pop().ok_or_else(|| "No SFX in CSV".into())
    }

    /// Parses an AYFX bank binary into a list of `SfxSequences`.
    ///
    /// # Errors
    /// Returns an error if the binary payload is empty or offset table pointers are truncated.
    pub fn from_ayfx_bank(bank_data: &[u8]) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        use crate::ayfx::AyfxFile;
        use crate::traits::SfxInput;

        let file = AyfxFile::from_bank(bank_data)?;
        file.to_sequences()
    }

    /// Parses a single AYFX effect binary into an `SfxSequence`.
    ///
    /// # Errors
    /// Returns an error if frame decoding fails.
    pub fn from_ayfx_effect(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        use crate::ayfx::AyfxFile;
        use crate::traits::SfxInput;

        let file = AyfxFile::from_effect(name, bytes)?;
        let mut seqs = file.to_sequences()?;
        seqs.pop().ok_or_else(|| "No SFX in effect file".into())
    }

    /// Parses compiled .yfx binary data into an `SfxSequence`.
    ///
    /// # Errors
    /// Returns an error if the binary size is not a multiple of 5 bytes.
    pub fn from_yfx(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        use crate::traits::SfxFile;
        use crate::yfx::YfxFile;

        let file = YfxFile::from_bytes(bytes)?;
        file.to_sequence(name)
    }

    /// Loads a single `SfxSequence` from a file path (.yfx, .json, .csv, .afx, or .afb).
    ///
    /// # Errors
    /// Returns an error if reading the file fails or the file extension is unsupported.
    pub fn load_from_path(
        input: &std::path::Path,
        bank_index: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let sequences = Self::load_all_from_paths(&[input.to_path_buf()])?;
        let idx = bank_index.min(sequences.len() - 1);
        Ok(sequences[idx].clone())
    }

    /// Loads all `SfxSequences` from a list of file paths.
    ///
    /// # Errors
    /// Returns an error if reading any target file fails or decoding any sequence fails.
    pub fn load_all_from_paths(
        inputs: &[std::path::PathBuf],
    ) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        use crate::container::SfxContainer;
        use crate::traits::SfxInput;

        let mut sequences = Vec::new();
        for input in inputs {
            let bytes = std::fs::read(input)?;
            let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("sfx");

            let container = SfxContainer::from_bytes(name, extension, &bytes)?;
            sequences.extend(container.to_sequences()?);
        }
        if sequences.is_empty() {
            return Err("No sound effects were loaded.".into());
        }
        Ok(sequences)
    }
}
