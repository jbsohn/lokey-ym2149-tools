use crate::ayfx::AyfxFile;
use crate::error::YmError;
use crate::sequence::{SfxSequence, YmSequence};
use crate::traits::{SfxInput, SongFile, SongInput};
use crate::yfx::YfxFile;
use crate::ym_file::YmFile;
use crate::ysg::YsgFile;

/// Unified container for any supported sound effect input format.
///
/// Encapsulates format detection and byte-level decoding across:
/// - AYFX CSV text dumps (.csv)
/// - AYFX binary sound effect banks (.afb)
/// - AYFX single binary effects (.afx)
/// - Lokey YFX 5-byte fixed streams (.yfx)
/// - Structured JSON sequence manifests (.json)
#[derive(Debug, Clone, PartialEq)]
pub enum SfxContainer {
    /// Sound effect loaded from an AYFX bank, single effect, or CSV table.
    Ayfx(AyfxFile),
    /// Compiled 5-byte fixed-stride YFX stream.
    Yfx(YfxFile),
    /// Canonical JSON sound effect manifest.
    Json(SfxSequence),
}

impl SfxContainer {
    /// Decodes a sound effect container from raw in-memory bytes and an extension identifier,
    /// completely decoupled from filesystem I/O.
    ///
    /// # Errors
    /// Returns an error if the extension is not supported or if the binary or text payload
    /// is malformed.
    pub fn from_bytes(name: &str, extension: &str, bytes: &[u8]) -> Result<Self, YmError> {
        match extension.to_ascii_lowercase().as_str() {
            "csv" => {
                let text = std::str::from_utf8(bytes).map_err(|e| {
                    YmError::Generic(format!("Invalid UTF-8 in CSV sound effect: {e}"))
                })?;
                Ok(Self::Ayfx(AyfxFile::from_csv(name, text)?))
            }
            "afb" => Ok(Self::Ayfx(AyfxFile::from_bank(bytes)?)),
            "afx" => Ok(Self::Ayfx(AyfxFile::from_effect(name, bytes)?)),
            "yfx" => Ok(Self::Yfx(YfxFile::try_from(bytes)?)),
            "json" => Ok(Self::Json(serde_json::from_slice(bytes)?)),
            _ => Err(YmError::UnsupportedExtension(extension.to_string())),
        }
    }
}

impl SfxInput for SfxContainer {
    fn to_sequences(&self) -> Result<Vec<SfxSequence>, Box<dyn std::error::Error>> {
        match self {
            Self::Ayfx(f) => f.to_sequences(),
            Self::Yfx(f) => f.to_sequences(),
            Self::Json(s) => Ok(vec![s.clone()]),
        }
    }
}

/// Unified container for any supported song input format.
///
/// Encapsulates format detection and byte-level decoding across:
/// - Raw Arnaud Carré YM chiptune streams (.ym)
/// - Compiled 20-byte relocatable 4-track YSG files (.ysg)
/// - Canonical JSON song sequence manifests (.json)
#[derive(Debug, Clone, PartialEq)]
pub enum SongContainer {
    /// Raw uncompressed or LHA-compressed YM chiptune dump.
    Ym(YmFile),
    /// Compiled 4-track decoupled YSG song container.
    Ysg(YsgFile),
    /// Canonical JSON song sequence manifest.
    Json(YmSequence),
}

impl SongContainer {
    /// Decodes a song container from raw in-memory bytes and an extension identifier,
    /// completely decoupled from filesystem I/O.
    ///
    /// # Errors
    /// Returns an error if the extension is not supported or if the file payload is corrupted.
    pub fn from_bytes(
        name: &str,
        extension: &str,
        bytes: &[u8],
        clock_override: Option<u32>,
    ) -> Result<Self, YmError> {
        match extension.to_ascii_lowercase().as_str() {
            "ym" => Ok(Self::Ym(YmFile::from_bytes(name, bytes, clock_override)?)),
            "ysg" => Ok(Self::Ysg(YsgFile::try_from(bytes)?)),
            "json" => Ok(Self::Json(serde_json::from_slice(bytes)?)),
            _ => Err(YmError::UnsupportedExtension(extension.to_string())),
        }
    }

    /// Converts or decompiles the container into the universal `YmSequence` IR.
    ///
    /// # Errors
    /// Returns an error if pitch retuning or stream decompression fails.
    pub fn to_sequence(
        &self,
        name: &str,
        target_clock_override: Option<u32>,
    ) -> Result<YmSequence, Box<dyn std::error::Error>> {
        match self {
            Self::Ym(f) => f.to_sequence(target_clock_override),
            Self::Ysg(f) => f.to_sequence(name),
            Self::Json(s) => Ok(s.clone()),
        }
    }
}

impl SongInput for SongContainer {
    fn title(&self) -> &str {
        match self {
            Self::Ym(f) => f.title(),
            Self::Ysg(_) => "song",
            Self::Json(s) => &s.name,
        }
    }

    fn source_clock(&self) -> u32 {
        match self {
            Self::Ym(f) => f.source_clock(),
            Self::Ysg(f) => f.master_clock_hz(),
            Self::Json(s) => s.timing.master_clock_hz,
        }
    }

    fn source_hz(&self) -> u16 {
        match self {
            Self::Ym(f) => f.source_hz(),
            Self::Ysg(f) => f.frame_rate_hz(),
            Self::Json(s) => s.timing.frame_rate.hz_value() as u16,
        }
    }

    fn to_sequence(
        &self,
        target_clock: Option<u32>,
    ) -> Result<YmSequence, Box<dyn std::error::Error>> {
        self.to_sequence(self.title(), target_clock)
    }
}
