#![allow(clippy::similar_names)]

use crate::chip::{TiaChannel, TiaChip, TiaSoundType};
use crate::timing::{SystemHz, TimingConfig, TIA_NTSC_AUDIO_CLOCK};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Custom deserializer for AUDC fields that accepts integers (0-15) or named strings ("saw", "noise", etc.).
///
/// # Errors
///
/// Returns a serde deserialization error if the payload is not a valid number or recognized sound type string.
pub fn deserialize_audc_option<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(num)) => {
            if let Some(val) = num.as_u64() {
                Ok(Some((val as u8) & 0x0F))
            } else {
                Err(serde::de::Error::custom("expected integer for AUDC"))
            }
        }
        Some(serde_json::Value::String(s)) => {
            let st = TiaSoundType::from_name(&s);
            Ok(Some(st.audc_value()))
        }
        _ => Err(serde::de::Error::custom(
            "expected number or sound type string for AUDC",
        )),
    }
}

/// Frame representation for authoring full dual-channel TIA music sequences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TiaFrame {
    pub audf0: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_audc_option")]
    pub audc0: Option<u8>,
    pub audv0: Option<u8>,
    pub audf1: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_audc_option")]
    pub audc1: Option<u8>,
    pub audv1: Option<u8>,
    pub duration: Option<u8>,
}

impl TiaFrame {
    /// Applies non-None register values from this frame to `chip`.
    pub fn apply_to_chip(&self, chip: &mut TiaChip) {
        if let Some(f) = self.audf0 {
            chip.set_audf(TiaChannel::Ch0, f);
        }
        if let Some(c) = self.audc0 {
            chip.set_audc(TiaChannel::Ch0, c);
        }
        if let Some(v) = self.audv0 {
            chip.set_audv(TiaChannel::Ch0, v);
        }
        if let Some(f) = self.audf1 {
            chip.set_audf(TiaChannel::Ch1, f);
        }
        if let Some(c) = self.audc1 {
            chip.set_audc(TiaChannel::Ch1, c);
        }
        if let Some(v) = self.audv1 {
            chip.set_audv(TiaChannel::Ch1, v);
        }
    }
}

/// Dual-channel sound sequence manifest container for TIA music assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TiaSequence {
    pub name: String,
    pub timing: TimingConfig,
    pub priority: u8,
    pub loop_start: Option<usize>,
    pub frames: Vec<TiaFrame>,
}

#[derive(Debug)]
struct TsgHeader {
    pattern_size: usize,
    num_unique: usize,
    seq_len: usize,
    loop_pattern: usize,
    frame_rate_hz: u32,
    master_clock_hz: u32,
    last_pat_frames: usize,
}

impl TiaSequence {
    /// Parses a JSON string representation into a `TiaSequence`.
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }

    /// Serializes this sequence into a JSON string.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes a compiled .tsg binary stream into a `TiaSequence`.
    ///
    /// # Errors
    /// Returns an error if header validation fails or payload is truncated.
    pub fn from_tsg(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let header = Self::parse_tsg_header(bytes)?;
        let seq_table_start = 14;
        let offset_table_start = seq_table_start + header.seq_len;
        let pattern_data_start = offset_table_start + header.num_unique * 4;

        if bytes.len() < pattern_data_start {
            return Err("TSG file truncated before pattern data".into());
        }

        let sequence_table = Self::parse_sequence_table(bytes, seq_table_start, header.seq_len)?;
        let offsets = Self::parse_offset_table(bytes, offset_table_start, header.num_unique)?;
        let frames = Self::decode_tsg_pattern_frames(
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

    fn parse_tsg_header(bytes: &[u8]) -> Result<TsgHeader, Box<dyn std::error::Error>> {
        if bytes.len() < 14 {
            return Err("TSG file too small to contain header".into());
        }
        let pattern_size = bytes[0] as usize;
        let num_unique = bytes[1] as usize;
        let seq_len = bytes[2] as usize;
        let loop_pattern = bytes[3] as usize;
        let frame_rate_hz = u32::from(bytes[4]);
        let master_clock_hz = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);
        let last_pat_frames = bytes[9] as usize;

        if pattern_size == 0 || num_unique == 0 || seq_len == 0 {
            return Err("Invalid TSG header parameters".into());
        }

        Ok(TsgHeader {
            pattern_size,
            num_unique,
            seq_len,
            loop_pattern,
            frame_rate_hz,
            master_clock_hz,
            last_pat_frames,
        })
    }

    fn parse_sequence_table(
        bytes: &[u8],
        start: usize,
        len: usize,
    ) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
        if bytes.len() < start + len {
            return Err("TSG truncated in sequence table".into());
        }
        Ok(bytes[start..start + len]
            .iter()
            .map(|&b| b as usize)
            .collect())
    }

    fn parse_offset_table(
        bytes: &[u8],
        start: usize,
        count: usize,
    ) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
        let end = start + count * 4;
        if bytes.len() < end {
            return Err("TSG truncated in offset table".into());
        }
        let mut offsets = Vec::with_capacity(count);
        for chunk in bytes[start..end].chunks_exact(4) {
            let offset = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
            offsets.push(offset);
        }
        Ok(offsets)
    }

    fn decode_tsg_pattern_frames(
        bytes: &[u8],
        data_start: usize,
        sequence_table: &[usize],
        offsets: &[usize],
        header: &TsgHeader,
    ) -> Result<Vec<TiaFrame>, Box<dyn std::error::Error>> {
        let pattern_data = &bytes[data_start..];
        let mut unique_patterns: Vec<Vec<TiaFrame>> = Vec::with_capacity(header.num_unique);

        for pat_idx in 0..header.num_unique {
            let start = offsets[pat_idx];
            let end = if pat_idx + 1 < header.num_unique {
                offsets[pat_idx + 1]
            } else {
                pattern_data.len()
            };

            if start > pattern_data.len() || end > pattern_data.len() || start > end {
                return Err(format!("Invalid pattern offset for pattern {pat_idx}").into());
            }

            let pat_bytes = &pattern_data[start..end];
            let pat_frames = Self::decode_single_pattern(pat_bytes, header.pattern_size)?;
            unique_patterns.push(pat_frames);
        }

        let mut all_frames = Vec::new();
        for (seq_idx, &pat_id) in sequence_table.iter().enumerate() {
            if pat_id >= unique_patterns.len() {
                return Err(format!("Sequence references out-of-bounds pattern {pat_id}").into());
            }
            let is_last_pattern = seq_idx == sequence_table.len() - 1;
            let count = if is_last_pattern && header.last_pat_frames > 0 {
                header.last_pat_frames
            } else {
                header.pattern_size
            };

            let frames = &unique_patterns[pat_id];
            all_frames.extend_from_slice(&frames[..count.min(frames.len())]);
        }

        Ok(all_frames)
    }

    fn decode_single_pattern(
        bytes: &[u8],
        expected_frames: usize,
    ) -> Result<Vec<TiaFrame>, Box<dyn std::error::Error>> {
        let mut frames = Vec::with_capacity(expected_frames);
        let mut idx = 0;

        while frames.len() < expected_frames && idx < bytes.len() {
            let mask = bytes[idx];
            idx += 1;

            if mask == 0x80 {
                // RLE marker: next byte is repeat count
                if idx >= bytes.len() {
                    return Err("Truncated RLE token in TSG".into());
                }
                let rle_count = bytes[idx] as usize;
                idx += 1;
                let last: TiaFrame = frames.last().cloned().unwrap_or_default();
                for _ in 0..rle_count {
                    if frames.len() < expected_frames {
                        frames.push(last.clone());
                    }
                }
                continue;
            }

            let mut frame = TiaFrame::default();
            // Bit 0: AUDF0, Bit 1: AUDC0, Bit 2: AUDV0
            // Bit 3: AUDF1, Bit 4: AUDC1, Bit 5: AUDV1
            if mask & (1 << 0) != 0 {
                if idx >= bytes.len() {
                    return Err("Truncated AUDF0".into());
                }
                frame.audf0 = Some(bytes[idx]);
                idx += 1;
            }
            if mask & (1 << 1) != 0 {
                if idx >= bytes.len() {
                    return Err("Truncated AUDC0".into());
                }
                frame.audc0 = Some(bytes[idx]);
                idx += 1;
            }
            if mask & (1 << 2) != 0 {
                if idx >= bytes.len() {
                    return Err("Truncated AUDV0".into());
                }
                frame.audv0 = Some(bytes[idx]);
                idx += 1;
            }
            if mask & (1 << 3) != 0 {
                if idx >= bytes.len() {
                    return Err("Truncated AUDF1".into());
                }
                frame.audf1 = Some(bytes[idx]);
                idx += 1;
            }
            if mask & (1 << 4) != 0 {
                if idx >= bytes.len() {
                    return Err("Truncated AUDC1".into());
                }
                frame.audc1 = Some(bytes[idx]);
                idx += 1;
            }
            if mask & (1 << 5) != 0 {
                if idx >= bytes.len() {
                    return Err("Truncated AUDV1".into());
                }
                frame.audv1 = Some(bytes[idx]);
                idx += 1;
            }

            frames.push(frame);
        }

        while frames.len() < expected_frames {
            frames.push(TiaFrame::default());
        }

        Ok(frames)
    }

    /// Loads a `TiaSequence` from a file path (.json or .tsg).
    ///
    /// # Errors
    /// Returns an error if reading or parsing the file fails.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "json" => {
                let json_str = std::str::from_utf8(&bytes)?;
                Ok(Self::from_json(json_str)?)
            }
            "tsg" => Self::from_tsg(name, &bytes),
            other => Err(format!(
                "Unsupported TIA song extension '.{other}'. Expected .json or .tsg"
            )
            .into()),
        }
    }
}

/// Single-channel frame representation for TIA sound effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TiaSfxFrame {
    pub audf: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_audc_option")]
    pub audc: Option<u8>,
    pub audv: Option<u8>,
    pub duration: Option<u8>,
}

impl TiaSfxFrame {
    /// Applies non-None register values to the specified channel on `chip`.
    pub fn apply_to_chip(&self, chip: &mut TiaChip, channel: TiaChannel) {
        if let Some(f) = self.audf {
            chip.set_audf(channel, f);
        }
        if let Some(c) = self.audc {
            chip.set_audc(channel, c);
        }
        if let Some(v) = self.audv {
            chip.set_audv(channel, v);
        }
    }
}

fn default_source_clock() -> u32 {
    TIA_NTSC_AUDIO_CLOCK
}

const fn default_source_hz() -> u32 {
    60
}

/// Single-channel Sound Effect manifest for TIA sound effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TiaSfxSequence {
    pub name: String,
    #[serde(default = "default_source_clock")]
    pub source_clock: u32,
    #[serde(default = "default_source_hz")]
    pub source_hz: u32,
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub preferred_channel: Option<TiaChannel>,
    #[serde(default)]
    pub loop_start: Option<usize>,
    pub frames: Vec<TiaSfxFrame>,
}

impl TiaSfxSequence {
    /// Parses a JSON string representation into a `TiaSfxSequence`.
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }

    /// Serializes this SFX sequence into a JSON string.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses a CSV string where each row has `AUDF,AUDC,AUDV` (or with headers).
    ///
    /// # Errors
    /// Returns an error if parsing fails.
    pub fn from_csv(name: &str, csv_data: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut frames = Vec::new();
        for line in csv_data.lines() {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with("audf")
                || line.starts_with("AUDF")
            {
                continue;
            }
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() >= 3 {
                let audf = parts[0].parse::<u8>()?;
                let audc = parts[1].parse::<u8>()?;
                let audv = parts[2].parse::<u8>()?;
                frames.push(TiaSfxFrame {
                    audf: Some(audf),
                    audc: Some(audc),
                    audv: Some(audv),
                    duration: None,
                });
            }
        }

        Ok(Self {
            name: name.to_string(),
            source_clock: TIA_NTSC_AUDIO_CLOCK,
            source_hz: 60,
            priority: 0,
            preferred_channel: None,
            loop_start: None,
            frames,
        })
    }

    /// Parses a raw fixed-width 3-byte binary `.tfx` payload (`[AUDF, AUDC, AUDV]` per frame).
    ///
    /// # Errors
    /// Returns an error if the payload is not a multiple of 3 bytes.
    pub fn from_tfx(name: &str, bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if !bytes.len().is_multiple_of(3) {
            return Err("TFX payload size must be a multiple of 3 bytes".into());
        }

        let mut frames = Vec::with_capacity(bytes.len() / 3);
        for chunk in bytes.chunks_exact(3) {
            frames.push(TiaSfxFrame {
                audf: Some(chunk[0] & 0x1F),
                audc: Some(chunk[1] & 0x0F),
                audv: Some(chunk[2] & 0x0F),
                duration: None,
            });
        }

        Ok(Self {
            name: name.to_string(),
            source_clock: TIA_NTSC_AUDIO_CLOCK,
            source_hz: 60,
            priority: 0,
            preferred_channel: None,
            loop_start: None,
            frames,
        })
    }

    /// Serializes this SFX sequence into a fixed-width 3-byte binary `.tfx` payload.
    #[must_use]
    pub fn to_tfx(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.frames.len() * 3);
        for frame in &self.frames {
            bytes.push(frame.audf.unwrap_or(0) & 0x1F);
            bytes.push(frame.audc.unwrap_or(0) & 0x0F);
            bytes.push(frame.audv.unwrap_or(0) & 0x0F);
        }
        bytes
    }

    /// Loads a `TiaSfxSequence` from a file path (.json, .csv, or .tfx).
    ///
    /// # Errors
    /// Returns an error if reading or parsing fails.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "json" => {
                let json_str = std::str::from_utf8(&bytes)?;
                Ok(Self::from_json(json_str)?)
            }
            "csv" => {
                let csv_str = std::str::from_utf8(&bytes)?;
                Self::from_csv(name, csv_str)
            }
            "tfx" => Self::from_tfx(name, &bytes),
            other => Err(format!(
                "Unsupported TIA SFX extension '.{other}'. Expected .json, .csv, or .tfx"
            )
            .into()),
        }
    }
}
