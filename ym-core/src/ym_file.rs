use crate::sequence::{YmFrame, YmSequence};
use crate::timing::{SystemHz, TimingConfig, ATARI_7800_CLOCK, ATARI_ST_CLOCK};
use crate::traits::SongInput;
use ym2149_common::{ChiptunePlayer, MetadataFields};
use ym2149_ym_replayer::decompress_if_needed;
use ym2149_ym_replayer::load_song;
use ym2149_ym_replayer::parser::{Ym6Parser, YmParser};

/// Represents an Atari ST / Amstrad / Spectrum `.ym` chiptune input file.
///
/// Encapsulates format validation, LHA decompression, header inspection,
/// and raw 14-register frame storage.
#[derive(Debug, Clone, PartialEq)]
pub struct YmFile {
    /// Song name or file stem.
    pub name: String,
    /// Master clock frequency of the original file (typically 2,000,000 Hz for Atari ST).
    pub source_clock: u32,
    /// Native playback frame rate (typically 50 Hz).
    pub source_hz: u16,
    /// Optional frame index to loop to.
    pub loop_frame: Option<usize>,
    /// Uncompressed physical 16-byte raw frame buffers.
    pub raw_frames: Vec<[u8; 16]>,
    /// Count of frames where YM6 digi-drum sample values were detected.
    pub digidrum_frames: usize,
    /// Total byte length after decompression.
    pub uncompressed_len: usize,
}

impl YmFile {
    /// Parses a `.ym` binary file buffer (automatically decompressing LHA if needed).
    ///
    /// # Errors
    /// Returns an error if decompression fails or if the format signature is unsupported.
    pub fn from_bytes(
        name: &str,
        ym_data: &[u8],
        source_clock_override: Option<u32>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let decompressed = decompress_if_needed(ym_data)?;
        let uncompressed_len = decompressed.len();
        let source_clock =
            source_clock_override.unwrap_or_else(|| Self::detect_ym_source_clock(&decompressed));

        // YM2 / YM3 format: interleaved register data (all R0, then all R1, ...) at 50 Hz.
        if decompressed.len() >= 4 {
            let magic = &decompressed[0..4];
            if magic == b"YM2!" || magic == b"YM3!" {
                let data = &decompressed[4..];
                let frame_count = data.len() / 14;
                let mut raw_frames = Vec::with_capacity(frame_count);
                for f in 0..frame_count {
                    let mut raw16 = [0u8; 16];
                    for r in 0..14 {
                        raw16[r] = data[r * frame_count + f];
                    }
                    raw_frames.push(Self::sanitize_raw16(&raw16).0);
                }

                return Ok(Self {
                    name: name.to_string(),
                    source_clock,
                    source_hz: 50,
                    loop_frame: None,
                    raw_frames,
                    digidrum_frames: 0,
                    uncompressed_len,
                });
            }
        }

        let (player, summary) = load_song(&decompressed)?;
        let total_frames = summary.frame_count;

        let loop_frame = player
            .metadata()
            .loop_frame()
            .filter(|&frame| frame < total_frames);

        let parsed_frames = Self::parse_raw_frames(&decompressed)
            .ok_or("Unsupported YM format: only YM4/YM5/YM6 are supported")?;

        let mut digidrum_frames = 0usize;
        let mut raw_frames = Vec::with_capacity(total_frames);
        for raw in parsed_frames.into_iter().take(total_frames) {
            let (sanitized16, has_digidrum) = Self::sanitize_raw16(&raw);
            if has_digidrum {
                digidrum_frames += 1;
            }
            raw_frames.push(sanitized16);
        }

        Ok(Self {
            name: name.to_string(),
            source_clock,
            source_hz: 50,
            loop_frame,
            raw_frames,
            digidrum_frames,
            uncompressed_len,
        })
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
        if let Ok((frames, _)) = YmParser::new().parse_full(decompressed) {
            return Some(frames);
        }

        let ym6 = Ym6Parser {};
        if let Ok((frames, _, _, _)) = ym6.parse_full(decompressed) {
            return Some(frames);
        }

        None
    }

    /// Sanitizes a 16-byte raw YM frame into 14 hardware registers, stripping unused bits
    /// and silencing YM6 digi-drum sample values.
    fn sanitize_raw_frame(raw: &[u8; 16]) -> ([u8; 14], bool) {
        let mut reg_14 = [0u8; 14];
        reg_14.copy_from_slice(&raw[0..14]);
        reg_14[1] &= 0x0F;
        reg_14[3] &= 0x0F;
        reg_14[5] &= 0x0F;

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

    /// Sanitizes a 16-byte raw YM frame in place of its 14 register bytes, preserving the
    /// original R13 byte (0xFF = no envelope retrigger) and zeroing bytes 14-15.
    fn sanitize_raw16(raw: &[u8; 16]) -> ([u8; 16], bool) {
        let (reg_14, has_digidrum) = Self::sanitize_raw_frame(raw);
        let mut sanitized16 = [0u8; 16];
        sanitized16[0..14].copy_from_slice(&reg_14);
        sanitized16[13] = raw[13];
        (sanitized16, has_digidrum)
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
            envelope_shape: Some(env_shape),
            duration: None,
        }
    }
}

impl SongInput for YmFile {
    fn title(&self) -> &str {
        &self.name
    }

    fn source_clock(&self) -> u32 {
        self.source_clock
    }

    fn source_hz(&self) -> u16 {
        self.source_hz
    }

    fn to_sequence(
        &self,
        target_clock_override: Option<u32>,
    ) -> Result<YmSequence, Box<dyn std::error::Error>> {
        let target_clock = target_clock_override.unwrap_or(ATARI_7800_CLOCK).max(1);
        let source_clock = self.source_clock.max(1);
        let ratio = f64::from(target_clock) / f64::from(source_clock);
        let apply_scaling = (ratio - 1.0).abs() > 0.0001;

        let frames: Vec<YmFrame> = self
            .raw_frames
            .iter()
            .map(|raw| {
                let mut reg_14 = [0u8; 14];
                reg_14.copy_from_slice(&raw[0..14]);
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

        Ok(YmSequence {
            name: self.name.clone(),
            timing: TimingConfig {
                master_clock_hz: target_clock,
                frame_rate: SystemHz::Custom(u32::from(self.source_hz)),
            },
            priority: 0,
            loop_start: self.loop_frame,
            frames,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ym3_frames_are_sanitized() {
        const FRAMES: usize = 2;
        let mut regs = [[0u8; 14]; FRAMES];
        regs[0][0] = 0x34;
        regs[0][1] = 0xF1; // stray high nibble in coarse tone A
        regs[0][8] = 0x80; // impossible volume value (bits 5-7 set)
        regs[0][13] = 0x0E;
        regs[1][13] = 0xFF; // no envelope retrigger

        let mut data = b"YM3!".to_vec();
        for r in 0..14 {
            for frame in &regs {
                data.push(frame[r]);
            }
        }

        let ym = YmFile::from_bytes("ym3", &data, None).unwrap();
        assert_eq!(ym.raw_frames.len(), FRAMES);
        assert_eq!(ym.raw_frames[0][1], 0x01);
        assert_eq!(ym.raw_frames[0][8], 0x00);

        let seq = ym.to_sequence(Some(ATARI_ST_CLOCK)).unwrap();
        assert_eq!(seq.frames[0].tone_a, Some(0x0134));
        assert_eq!(seq.frames[0].envelope_shape, Some(0x0E));
        assert_eq!(seq.frames[1].envelope_shape, None);
    }
}
