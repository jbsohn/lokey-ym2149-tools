use crate::sequence::{SfxFrame, SfxSequence};
use crate::timing::ZX_SPECTRUM_CLOCK;
use crate::traits::SfxInput;

/// Represents an AYFX sound effect source (.afx single effect, .afb bank, or .csv export).
///
/// Implements `SfxInput` to produce canonical `SfxSequence` instances.
#[derive(Debug, Clone, PartialEq)]
pub struct AyfxFile {
    pub sequences: Vec<SfxSequence>,
}

impl AyfxFile {
    /// Parses a single AYFX effect binary (.afx) into an `AyfxFile`.
    ///
    /// # Errors
    /// Returns an error if frame decoding fails.
    pub fn from_effect(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let (frames, _) = Self::decode_ayfx_frames(bytes);
        Ok(Self {
            sequences: vec![SfxSequence {
                name: name.to_string(),
                source_clock: ZX_SPECTRUM_CLOCK,
                source_hz: 50,
                priority: 1,
                preferred_channels: None,
                loop_start: None,
                frames,
            }],
        })
    }

    /// Parses an AYFX bank binary (.afb) containing multiple sound effects.
    ///
    /// # Errors
    /// Returns an error if the binary payload is empty or offset table pointers are truncated.
    pub fn from_bank(bank_data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if bank_data.is_empty() {
            return Err("Empty bank data".into());
        }

        let num_effects = bank_data[0] as usize;
        let mut sequences = Vec::with_capacity(num_effects);

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

        Ok(Self { sequences })
    }

    /// Parses an AYFX CSV table (.csv) into an `AyfxFile`.
    ///
    /// # Errors
    /// Returns an error if CSV column values cannot be parsed.
    pub fn from_csv(name: &str, content: &str) -> Result<Self, Box<dyn std::error::Error>> {
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
                if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
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
            sequences: vec![SfxSequence {
                name: name.to_string(),
                source_clock: ZX_SPECTRUM_CLOCK,
                source_hz: 50,
                priority: 1,
                preferred_channels: None,
                loop_start: None,
                frames,
            }],
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
}

impl SfxInput for AyfxFile {
    fn to_sequences(&self) -> Result<Vec<SfxSequence>, Box<dyn std::error::Error>> {
        Ok(self.sequences.clone())
    }
}
