use crate::timing::{SystemHz, TimingConfig, ATARI_ST_CLOCK, ZX_SPECTRUM_CLOCK};
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

#[allow(dead_code)]
struct YsgHeader {
    pattern_size: usize,
    num_unique: usize,
    seq_len: usize,
    loop_pattern: usize,
    frame_rate_hz: u32,
    master_clock_hz: u32,
    last_pat_frames: usize,
    features: u8,
}

impl YmSequence {
    /// Deserializes a compiled .ysg binary stream into a `YmSequence`.
    ///
    /// # Errors
    ///
    /// Returns an error if header validation fails, sequence table offsets are invalid,
    /// or the byte payload is truncated before pattern data ends.
    pub fn from_ysg(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let header = Self::parse_ysg_header(bytes)?;

        let seq_table_start = 14;
        let offset_table_start = seq_table_start + header.seq_len;
        let pattern_data_start = offset_table_start + header.num_unique * 4;

        if bytes.len() < pattern_data_start {
            return Err("YSG file truncated before pattern data".into());
        }

        let sequence_table = Self::parse_sequence_table(bytes, seq_table_start, header.seq_len)?;
        let offsets = Self::parse_offset_table(bytes, offset_table_start, header.num_unique)?;
        let frames = Self::decode_ysg_pattern_frames(
            bytes,
            pattern_data_start,
            &sequence_table,
            &offsets,
            &header,
        )?;

        let loop_start = if header.loop_pattern == 255 {
            None
        } else {
            Some(header.loop_pattern * header.pattern_size)
        };

        Ok(Self {
            name: name.to_string(),
            timing: TimingConfig {
                master_clock_hz: header.master_clock_hz,
                frame_rate: SystemHz::Custom(header.frame_rate_hz),
            },
            priority: 0,
            loop_start,
            frames,
        })
    }

    /// Parses the 13-byte header from a YSG binary stream.
    fn parse_ysg_header(bytes: &[u8]) -> Result<YsgHeader, Box<dyn std::error::Error>> {
        if bytes.len() < 14 {
            return Err("YSG file too small to contain header".into());
        }
        let pattern_size = bytes[0] as usize;
        let num_unique = bytes[1] as usize;
        let seq_len = bytes[2] as usize;
        let loop_pattern = bytes[3] as usize;
        let frame_rate_hz = u32::from_le_bytes(bytes[4..8].try_into()?);
        let master_clock_hz = u32::from_le_bytes(bytes[8..12].try_into()?);
        let last_pat_frames = bytes[12] as usize;
        let features = bytes[13];
        Ok(YsgHeader {
            pattern_size,
            num_unique,
            seq_len,
            loop_pattern,
            frame_rate_hz,
            master_clock_hz,
            last_pat_frames,
            features,
        })
    }

    /// Reads the sequence pattern index table.
    fn parse_sequence_table(
        bytes: &[u8],
        start: usize,
        seq_len: usize,
    ) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
        if bytes.len() < start + seq_len {
            return Err("YSG file truncated before sequence table end".into());
        }
        let mut sequence_table = Vec::with_capacity(seq_len);
        for i in 0..seq_len {
            sequence_table.push(bytes[start + i] as usize);
        }
        Ok(sequence_table)
    }

    /// Reads pattern byte offsets from the YSG header.
    fn parse_offset_table(
        bytes: &[u8],
        start: usize,
        num_unique: usize,
    ) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
        if bytes.len() < start + num_unique * 4 {
            return Err("YSG file truncated before offset table end".into());
        }
        let mut offsets = Vec::with_capacity(num_unique);
        for i in 0..num_unique {
            let ptr = start + i * 4;
            let offset = u32::from_le_bytes(bytes[ptr..ptr + 4].try_into()?);
            offsets.push(offset as usize);
        }
        Ok(offsets)
    }

    /// Decodes pattern register streams into frames by iterating the sequence table.
    fn decode_ysg_pattern_frames(
        bytes: &[u8],
        pattern_data_start: usize,
        sequence_table: &[usize],
        offsets: &[usize],
        header: &YsgHeader,
    ) -> Result<Vec<YmFrame>, Box<dyn std::error::Error>> {
        let mut frames = Vec::new();
        let last_entry = sequence_table.len().saturating_sub(1);

        for (entry_idx, &pattern_idx) in sequence_table.iter().enumerate() {
            if pattern_idx >= header.num_unique {
                return Err(format!(
                    "Sequence index {} out of range (max {})",
                    pattern_idx, header.num_unique
                )
                .into());
            }
            let start_ptr = pattern_data_start + offsets[pattern_idx];
            if start_ptr >= bytes.len() {
                return Err("YSG pattern offset out of bounds".into());
            }
            let frames_to_decode = if entry_idx == last_entry && header.last_pat_frames > 0 {
                header.last_pat_frames
            } else {
                header.pattern_size
            };
            frames.extend(Self::decode_ysg_pattern(
                bytes,
                start_ptr,
                frames_to_decode,
                header.features,
            )?);
        }

        Ok(frames)
    }

    fn is_rle_token(mask: u16, rle_enabled: bool) -> bool {
        rle_enabled && (mask & crate::delta::RLE_FLAG) != 0
    }

    /// Decodes one pattern's delta-encoded register stream into frames.
    /// Handles RLE tokens (mask bit 15 set) when features bit 0 is set.
    fn decode_ysg_pattern(
        bytes: &[u8],
        start_ptr: usize,
        pattern_size: usize,
        features: u8,
    ) -> Result<Vec<YmFrame>, Box<dyn std::error::Error>> {
        let rle_enabled = (features & 0x01) != 0;
        let mut frames = Vec::with_capacity(pattern_size);
        let mut pp = start_ptr;
        let mut registers = [0u8; 14];

        while frames.len() < pattern_size {
            if pp + 1 >= bytes.len() {
                return Err("Unexpected EOF in YSG pattern data".into());
            }
            let mask = u16::from(bytes[pp]) | (u16::from(bytes[pp + 1]) << 8);
            pp += 2;

            if Self::is_rle_token(mask, rle_enabled) {
                if pp >= bytes.len() {
                    return Err("Unexpected EOF in YSG RLE count byte".into());
                }
                let n = bytes[pp] as usize;
                pp += 1;
                let emit = (n + 1).min(pattern_size - frames.len());
                let frame = {
                    let mut f = Self::registers_to_frame(&registers);
                    f.envelope_shape = None;
                    f
                };
                for _ in 0..emit {
                    frames.push(frame.clone());
                }
                continue;
            }

            let r13_written = (mask & (1 << 13)) != 0;

            for (reg, slot) in registers.iter_mut().enumerate() {
                if (mask & (1 << reg)) != 0 {
                    if pp >= bytes.len() {
                        return Err("Unexpected EOF in YSG pattern register payload".into());
                    }
                    *slot = bytes[pp];
                    pp += 1;
                }
            }

            let mut frame = Self::registers_to_frame(&registers);
            if !r13_written {
                frame.envelope_shape = None;
            }
            frames.push(frame);
        }

        Ok(frames)
    }

    /// Sanitizes a 16-byte raw YM frame into 14 hardware registers, stripping unused bits
    /// and detecting YM6 digi-drum sample values. Returns the register array and whether
    /// any digi-drum data was found and silenced.
    fn sanitize_raw_frame(raw: &[u8; 16]) -> ([u8; 14], bool) {
        let mut reg_14 = [0u8; 14];
        reg_14.copy_from_slice(&raw[0..14]);
        reg_14[1] &= 0x0F; // R1 bits 4-7 unused
        reg_14[3] &= 0x0F; // R3 bits 4-7 unused
        reg_14[5] &= 0x0F; // R5 bits 4-7 unused
                           // YM6 digi-drum frames store PCM sample values (0-255) in R8-R10 rather than
                           // hardware volume values (0-31). Bits 5-7 set is physically impossible on the
                           // chip — silence those channels to prevent false envelope-mode triggering.
        let has_digidrum = reg_14[8] > 0x1F || reg_14[9] > 0x1F || reg_14[10] > 0x1F;
        if reg_14[8] > 0x1F {
            reg_14[8] = 0;
        }
        if reg_14[9] > 0x1F {
            reg_14[9] = 0;
        }
        if reg_14[10] > 0x1F {
            reg_14[10] = 0;
        }
        (reg_14, has_digidrum)
    }

    /// Converts 14 YM-2149 hardware registers to a `YmFrame`.
    fn registers_to_frame(registers: &[u8; 14]) -> YmFrame {
        let tone_a = u16::from(registers[0]) | (u16::from(registers[1]) << 8);
        let tone_b = u16::from(registers[2]) | (u16::from(registers[3]) << 8);
        let tone_c = u16::from(registers[4]) | (u16::from(registers[5]) << 8);
        let noise_period = registers[6];
        let mixer = registers[7];
        let volume_a = registers[8];
        let volume_b = registers[9];
        let volume_c = registers[10];
        let env_period = u16::from(registers[11]) | (u16::from(registers[12]) << 8);
        let env_shape = registers[13];

        YmFrame {
            tone_a: Some(tone_a),
            tone_b: Some(tone_b),
            tone_c: Some(tone_c),
            noise_period: Some(noise_period),
            volume_a: Some(volume_a),
            volume_b: Some(volume_b),
            volume_c: Some(volume_c),
            tone_enable_a: Some((mixer & 0x01) == 0),
            tone_enable_b: Some((mixer & 0x02) == 0),
            tone_enable_c: Some((mixer & 0x04) == 0),
            noise_enable_a: Some((mixer & 0x08) == 0),
            noise_enable_b: Some((mixer & 0x10) == 0),
            noise_enable_c: Some((mixer & 0x20) == 0),
            envelope_period: Some(env_period),
            envelope_shape: Some(env_shape & 0x0F),
            duration: Some(1),
        }
    }

    /// Decodes raw .ym chiptune data into a `YmSequence`.
    /// Returns the sequence and the number of frames where digi-drum sample values
    /// were detected and silenced (YM6 only). Callers should warn the user when > 0.
    ///
    /// # Errors
    ///
    /// Returns an error if LHA decompression fails or the YM register stream format is invalid.
    pub fn from_ym_data(
        name: &str,
        ym_data: &[u8],
        source_clock_override: Option<u32>,
    ) -> Result<(Self, usize), Box<dyn std::error::Error>> {
        use ym2149_common::{ChiptunePlayer, MetadataFields};
        use ym2149_ym_replayer::decompress_if_needed;
        use ym2149_ym_replayer::load_song;

        let decompressed = decompress_if_needed(ym_data)?;
        let source_clock =
            source_clock_override.unwrap_or_else(|| Self::detect_ym_source_clock(&decompressed));

        let target_clock = 1_789_773u32;
        let ratio = f64::from(target_clock) / f64::from(source_clock);
        let apply_scaling = (ratio - 1.0).abs() > 0.0001;

        // YM2 / YM3 format: interleaved register data (all R0 values, then all R1, ...) at 50 Hz.
        // Same interleaved layout as YM5 but without metadata. Chip clock assumed 2 MHz (Atari ST).
        if decompressed.len() >= 4 {
            let magic = &decompressed[0..4];
            if magic == b"YM2!" || magic == b"YM3!" {
                let data = &decompressed[4..];
                let frame_count = data.len() / 14;
                let mut frames = Vec::with_capacity(frame_count);
                for f in 0..frame_count {
                    let mut raw16 = [0u8; 16];
                    for r in 0..14 {
                        raw16[r] = data[r * frame_count + f];
                    }
                    let (reg_14, _) = Self::sanitize_raw_frame(&raw16);
                    let mut frame = Self::registers_to_frame(&reg_14);
                    if apply_scaling {
                        frame.scale_pitch(ratio);
                    }
                    frames.push(frame);
                }
                return Ok((
                    Self {
                        name: name.to_string(),
                        timing: TimingConfig {
                            master_clock_hz: target_clock,
                            frame_rate: SystemHz::Hz50,
                        },
                        priority: 0,
                        loop_start: None,
                        frames,
                    },
                    0,
                ));
            }
        }

        let (player, summary) = load_song(&decompressed)?;
        let total_frames = summary.frame_count;

        let loop_start = player
            .metadata()
            .loop_frame()
            .filter(|&frame| frame < total_frames);

        let raw_frames = Self::parse_raw_frames(&decompressed)
            .ok_or("Unsupported YM format: only YM4/YM5/YM6 are supported")?;

        let mut digidrum_frames = 0usize;
        let frames = raw_frames
            .into_iter()
            .take(total_frames)
            .map(|raw| {
                let (reg_14, has_digidrum) = Self::sanitize_raw_frame(&raw);
                if has_digidrum {
                    digidrum_frames += 1;
                }
                let mut frame = Self::registers_to_frame(&reg_14);
                frame.envelope_shape = if raw[13] == 0xFF {
                    None
                } else {
                    Some(raw[13] & 0x0F)
                };
                if apply_scaling {
                    frame.scale_pitch(ratio);
                }
                frame
            })
            .collect();

        Ok((
            Self {
                name: name.to_string(),
                timing: TimingConfig {
                    master_clock_hz: target_clock,
                    frame_rate: SystemHz::Hz50,
                },
                priority: 0,
                loop_start,
                frames,
            },
            digidrum_frames,
        ))
    }

    /// Detects the target YM clock frequency from chiptune header.
    fn detect_ym_source_clock(decompressed: &[u8]) -> u32 {
        if decompressed.len() >= 26
            && (&decompressed[0..4] == b"YM5!" || &decompressed[0..4] == b"YM6!")
        {
            let clock = u32::from_be_bytes([
                decompressed[22],
                decompressed[23],
                decompressed[24],
                decompressed[25],
            ]);
            if clock > 0 {
                clock
            } else {
                ATARI_ST_CLOCK
            }
        } else {
            ATARI_ST_CLOCK
        }
    }

    fn parse_raw_frames(decompressed: &[u8]) -> Option<Vec<[u8; 16]>> {
        use ym2149_ym_replayer::parser::{Ym6Parser, YmParser};

        if let Ok((frames, _)) = YmParser::new().parse_full(decompressed) {
            return Some(frames);
        }

        let ym6 = Ym6Parser {};
        if let Ok((frames, _, _, _)) = ym6.parse_full(decompressed) {
            return Some(frames);
        }

        None
    }

    /// Byte length of `ym_data` after decompression (e.g. from LHA-compressed
    /// `.ym` files), before any lokey-ym-tools recompilation. Useful for reporting
    /// how much smaller a compiled `.ysg` is than the source register stream.
    ///
    /// # Errors
    ///
    /// Returns an error if LHA decompression of `ym_data` fails.
    pub fn ym_decompressed_len(ym_data: &[u8]) -> Result<usize, Box<dyn std::error::Error>> {
        use ym2149_ym_replayer::decompress_if_needed;
        Ok(decompress_if_needed(ym_data)?.len())
    }

    /// Loads a `YmSequence` from a file path (.ysg, .ym, or .json).
    ///
    /// # Errors
    ///
    /// Returns an error if reading the target file from disk fails, or if decoding the
    /// sequence format fails.
    pub fn load_from_path(
        input: &std::path::Path,
        clock_override: Option<u32>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");

        match extension {
            "ysg" => {
                let bytes = std::fs::read(input)?;
                Self::from_ysg(name, &bytes)
            }
            "json" => {
                let content = std::fs::read_to_string(input)?;
                Ok(serde_json::from_str(&content)?)
            }
            "ym" => {
                let bytes = std::fs::read(input)?;
                let (seq, _) = Self::from_ym_data(name, &bytes, clock_override)?;
                Ok(seq)
            }
            _ => Err(format!(
                "Unsupported song file extension '.{extension}'. Expected .ysg, .ym, or .json"
            )
            .into()),
        }
    }
}

impl YmFrame {
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
    fn scale_pitch(&mut self, ratio: f64) {
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
    ///
    /// Returns an error if CSV parsing fails or mandatory column fields are missing.
    pub fn from_ayfx_csv(name: &str, content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut frames = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() < 5 {
                return Err(format!(
                    "Line {}: expected at least 5 columns, found {}",
                    line_num + 1,
                    parts.len()
                )
                .into());
            }

            let t = parts[0].parse::<i32>()? != 0;
            let n = parts[1].parse::<i32>()? != 0;

            let parse_val = |s: &str| -> Result<u16, Box<dyn std::error::Error>> {
                if let Some(hex) = s.strip_prefix("0x") {
                    Ok(u16::from_str_radix(hex, 16)?)
                } else if let Some(hex) = s.strip_prefix("0X") {
                    Ok(u16::from_str_radix(hex, 16)?)
                } else {
                    Ok(s.parse::<u16>()?)
                }
            };

            let tone = parse_val(parts[2])?;
            let noise = (parse_val(parts[3])? & 0x1F) as u8;
            let volume = (parse_val(parts[4])? & 0x1F) as u8;

            frames.push(SfxFrame::new(t, n, tone, noise, volume, 1));
        }

        Ok(Self {
            name: name.to_string(),
            source_clock: ZX_SPECTRUM_CLOCK,
            source_hz: 50,
            priority: 1,
            preferred_channels: None,
            loop_start: None,
            frames,
        })
    }

    /// Parses an AYFX bank binary into a list of `SfxSequences`.
    ///
    /// # Errors
    ///
    /// Returns an error if the binary payload is empty or offset table pointers are truncated.
    pub fn from_ayfx_bank(bank_data: &[u8]) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        if bank_data.is_empty() {
            return Err("Empty bank data".into());
        }

        let num_effects = bank_data[0] as usize;
        let mut sequences = Vec::new();

        for i in 0..num_effects {
            let offset_ptr = 1 + i * 2;
            if offset_ptr + 1 >= bank_data.len() {
                break;
            }
            let offset_val = (u16::from(bank_data[offset_ptr])
                | (u16::from(bank_data[offset_ptr + 1]) << 8))
                as usize;
            let start_idx = 2 + i * 2 + offset_val;
            if start_idx >= bank_data.len() {
                continue;
            }

            let max_len = Self::calculate_ayfx_effect_max_len(bank_data, i, num_effects, start_idx);
            let end_limit = (start_idx + max_len).min(bank_data.len());
            if start_idx >= end_limit {
                continue;
            }

            let (frames, consumed) = Self::decode_ayfx_frames(&bank_data[start_idx..end_limit]);
            let name = Self::parse_ayfx_effect_name(bank_data, start_idx + consumed, end_limit, i);

            sequences.push(SfxSequence {
                name,
                source_clock: ZX_SPECTRUM_CLOCK,
                source_hz: 50,
                priority: 1,
                preferred_channels: None,
                loop_start: None,
                frames,
            });
        }

        Ok(sequences)
    }

    /// Computes maximum byte length of an AYFX effect in a bank.
    fn calculate_ayfx_effect_max_len(
        bank_data: &[u8],
        i: usize,
        num_effects: usize,
        start_idx: usize,
    ) -> usize {
        if start_idx >= bank_data.len() {
            return 0;
        }
        if i < num_effects - 1 {
            let next_ptr = 3 + i * 2;
            if next_ptr + 1 < bank_data.len() {
                let next_offset_val = (u16::from(bank_data[next_ptr])
                    | (u16::from(bank_data[next_ptr + 1]) << 8))
                    as usize;
                let next_start_idx = 4 + i * 2 + next_offset_val;
                if next_start_idx <= bank_data.len() {
                    if let Some(diff) = next_start_idx.checked_sub(start_idx) {
                        if diff > 0 {
                            return diff;
                        }
                    }
                }
            }
        }
        bank_data.len().saturating_sub(start_idx)
    }

    /// Parses optional null-terminated effect name from AYFX block.
    fn parse_ayfx_effect_name(
        bank_data: &[u8],
        mut pp: usize,
        end_limit: usize,
        fallback_idx: usize,
    ) -> String {
        if pp < end_limit {
            let mut name_bytes = Vec::new();
            while pp < end_limit && bank_data[pp] != 0 {
                name_bytes.push(bank_data[pp]);
                pp += 1;
            }
            if !name_bytes.is_empty() {
                if let Ok(decoded_name) = String::from_utf8(name_bytes) {
                    return decoded_name;
                }
            }
        }
        format!("sfx_{}", fallback_idx + 1)
    }

    /// Parses a single AYFX effect binary into an `SfxSequence`.
    ///
    /// # Errors
    ///
    /// Returns an error if frame decoding fails.
    pub fn from_ayfx_effect(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let (frames, _) = Self::decode_ayfx_frames(bytes);

        Ok(Self {
            name: name.to_string(),
            source_clock: ZX_SPECTRUM_CLOCK,
            source_hz: 50,
            priority: 1,
            preferred_channels: None,
            loop_start: None,
            frames,
        })
    }

    /// Decodes AYFX frame bitstream data.
    fn decode_ayfx_frames(bytes: &[u8]) -> (Vec<SfxFrame>, usize) {
        let mut frames = Vec::new();
        let mut pp = 0;
        let mut tone = 0u16;
        let mut noise = 0u8;
        let end_limit = bytes.len();

        while pp < end_limit {
            let it = bytes[pp];
            pp += 1;

            if (it & (1 << 5)) != 0 {
                if pp + 1 >= end_limit {
                    break;
                }
                tone = (u16::from(bytes[pp]) | (u16::from(bytes[pp + 1]) << 8)) & 0xFFF;
                pp += 2;
            }
            if (it & (1 << 6)) != 0 {
                if pp >= end_limit {
                    break;
                }
                let n_val = bytes[pp];
                pp += 1;

                if it == 0xD0 && n_val >= 0x20 {
                    break;
                }
                noise = n_val & 0x1F;
            }

            let vol = it & 0x0F;
            let t_enable = (it & (1 << 4)) == 0;
            let n_enable = (it & (1 << 7)) == 0;

            frames.push(SfxFrame::new(t_enable, n_enable, tone, noise, vol, 1));
        }

        (frames, pp)
    }

    /// Parses compiled .yfx binary data into an `SfxSequence`.
    ///
    /// # Errors
    ///
    /// Returns an error if the binary size is not a multiple of 5 bytes.
    pub fn from_yfx(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if !bytes.len().is_multiple_of(5) {
            return Err("YFX file size must be a multiple of 5".into());
        }

        let frames = Self::decode_yfx_frames(bytes);

        Ok(Self {
            name: name.to_string(),
            source_clock: ZX_SPECTRUM_CLOCK,
            source_hz: 50,
            priority: 1,
            preferred_channels: None,
            loop_start: None,
            frames,
        })
    }

    /// Decodes 5-byte fixed-length YFX frame chunks.
    fn decode_yfx_frames(bytes: &[u8]) -> Vec<SfxFrame> {
        let mut frames = Vec::new();
        let mut pp = 0;

        while pp < bytes.len() {
            let tone_low = bytes[pp];
            let tone_high = bytes[pp + 1];
            let volume = bytes[pp + 2];
            let control = bytes[pp + 3];
            let duration = bytes[pp + 4];
            pp += 5;

            let tone = u16::from(tone_low) | (u16::from(tone_high) << 8);
            let tone_enable = (control & 0x01) != 0;
            let noise_enable = (control & 0x02) != 0;
            let noise = (control >> 3) & 0x1F;

            frames.push(SfxFrame::new(
                tone_enable,
                noise_enable,
                tone,
                noise,
                volume,
                duration,
            ));
        }

        frames
    }

    /// Loads a single `SfxSequence` from a file path (.yfx, .json, .csv, .afx, or .afb).
    ///
    /// # Errors
    ///
    /// Returns an error if reading the file fails or the file extension is unsupported.
    pub fn load_from_path(
        input: &std::path::Path,
        bank_index: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("sfx");

        match extension {
            "csv" => {
                let content = std::fs::read_to_string(input)?;
                Self::from_ayfx_csv(name, &content)
            }
            "afb" => {
                let bytes = std::fs::read(input)?;
                let bank = Self::from_ayfx_bank(&bytes)?;
                if bank.is_empty() {
                    return Err("AYFX bank contains no sound effects".into());
                }
                let idx = bank_index.min(bank.len() - 1);
                Ok(bank[idx].clone())
            }
            "afx" => {
                let bytes = std::fs::read(input)?;
                Self::from_ayfx_effect(name, &bytes)
            }
            "yfx" => {
                let bytes = std::fs::read(input)?;
                Self::from_yfx(name, &bytes)
            }
            "json" => {
                let content = std::fs::read_to_string(input)?;
                Ok(serde_json::from_str(&content)?)
            }
            _ => Err(format!(
                "Unsupported SFX file extension '.{extension}'. Expected .yfx, .json, .csv, .afx, or .afb")
                .into()),
        }
    }

    /// Loads all `SfxSequences` from a list of file paths.
    ///
    /// # Errors
    ///
    /// Returns an error if reading any target file fails or decoding any sequence fails.
    pub fn load_all_from_paths(
        inputs: &[std::path::PathBuf],
    ) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        let mut sequences = Vec::new();
        for input in inputs {
            let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("sfx");

            match extension {
                "csv" => {
                    let content = std::fs::read_to_string(input)?;
                    sequences.push(Self::from_ayfx_csv(name, &content)?);
                }
                "afb" => {
                    let bytes = std::fs::read(input)?;
                    let bank = Self::from_ayfx_bank(&bytes)?;
                    sequences.extend(bank);
                }
                "afx" => {
                    let bytes = std::fs::read(input)?;
                    sequences.push(Self::from_ayfx_effect(name, &bytes)?);
                }
                "yfx" => {
                    let bytes = std::fs::read(input)?;
                    sequences.push(Self::from_yfx(name, &bytes)?);
                }
                "json" => {
                    let content = std::fs::read_to_string(input)?;
                    sequences.push(serde_json::from_str(&content)?);
                }
                _ => {
                    return Err(format!(
                        "Unsupported SFX extension '.{extension}'. Expected .yfx, .json, .csv, .afx, or .afb")
                        .into())
                }
            }
        }
        if sequences.is_empty() {
            return Err("No sound effects were loaded.".into());
        }
        Ok(sequences)
    }
}
