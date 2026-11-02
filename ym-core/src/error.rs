use std::fmt;

/// Strongly-typed error enum for operations across `ym-core`.
#[derive(Debug)]
pub enum YmError {
    /// An I/O error occurred while reading or writing data.
    Io(std::io::Error),
    /// JSON serialization or deserialization failed.
    Json(serde_json::Error),
    /// UTF-8 string decoding failed.
    Utf8(std::str::Utf8Error),
    /// Integer parsing failed.
    ParseInt(std::num::ParseIntError),
    /// Underling YM replayer failed.
    Replayer(ym2149_ym_replayer::ReplayerError),
    /// The file magic bytes do not match the expected format identifier.
    InvalidMagic([u8; 2]),
    /// The container format version is unsupported.
    UnsupportedVersion(u8),
    /// Header was truncated before the full fixed-size structure was read.
    TruncatedHeader { expected: usize, actual: usize },
    /// Track descriptor header or offset table was truncated.
    TruncatedTrackDescriptor(&'static str),
    /// Track offset points out of bounds of the file or track table.
    TrackOffsetOutOfBounds(&'static str),
    /// Pattern payload bounds are invalid (e.g. end < start, or beyond track length).
    InvalidPatternBounds(&'static str),
    /// Pattern index in sequence table is out of bounds of the unique patterns pool.
    InvalidPatternIndex {
        track: &'static str,
        step: usize,
        index: u8,
        unique_count: usize,
    },
    /// Opcode stream reached unexpected end of input.
    UnexpectedEof(&'static str),
    /// Expected a register payload byte following an active opcode.
    MissingPayloadByte(&'static str),
    /// The number of unique patterns exceeds the 8-bit table capacity.
    TooManyUniquePatterns { count: usize, max: usize },
    /// Song contains no frames to compile.
    EmptySequence,
    /// Pattern frames per chunk must be greater than zero.
    InvalidPatternFrames,
    /// Sequence length (number of patterns) exceeds the 8-bit maximum (255 steps).
    SongTooLong { patterns: usize, max: usize },
    /// Compiled song exceeds the YSG maximum file size limit.
    SongBinaryTooLarge { size: usize, max: usize },
    /// Automated candidate search failed to find a valid pattern frame size.
    NoOptimalPatternFound,
    /// YFX binary length is not a multiple of the 5-byte fixed frame stride.
    InvalidYfxSize(usize),
    /// File extension is not recognized as a supported song or sound effect format.
    UnsupportedExtension(String),
    /// YM chiptune format is unsupported.
    UnsupportedYmFormat(&'static str),
    /// AYFX sound effect bank payload is empty.
    EmptyBank,
    /// AYFX sound effect bank payload is malformed.
    MalformedBank(String),
    /// AYFX CSV table parsing error.
    MalformedCsv { line: usize, message: String },
    /// The file does not begin with the VGM magic `"Vgm "`.
    InvalidVgmMagic([u8; 4]),
    /// VGM v1.51+ header has no AY-3-8910 / YM2149 clock, or the clock is zero.
    MissingAy8910Clock,
    /// Command byte is not a documented VGM opcode, so the stream can no longer be aligned.
    UnsupportedVgmCommand(u8),
    /// Decompressed stream exceeds safety limit.
    DecompressedTooLarge { max: usize },
    /// Song contains more playback frames than the safety limit.
    TooManyFrames { max: usize },
    /// Requested sound effect bank index is out of bounds.
    BankIndexOutOfBounds { index: usize, count: usize },
    /// No sound effects were loaded from input files.
    NoSfxLoaded,
    /// Generic or informational error message.
    Generic(String),
}

impl fmt::Display for YmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::Utf8(err) => write!(f, "UTF-8 error: {err}"),
            Self::ParseInt(err) => write!(f, "Parse int error: {err}"),
            Self::Replayer(err) => write!(f, "YM replayer error: {err}"),
            Self::InvalidMagic(magic) => write!(
                f,
                "Invalid magic header bytes: 0x{:02X}{:02X}",
                magic[0], magic[1]
            ),
            Self::UnsupportedVersion(ver) => write!(f, "Unsupported YSG version: {ver}"),
            Self::TruncatedHeader { expected, actual } => write!(
                f,
                "File truncated before header completed (expected {expected} bytes, got {actual})"
            ),
            Self::TruncatedTrackDescriptor(track) => {
                write!(f, "{track} track descriptor truncated before offset table")
            }
            Self::TrackOffsetOutOfBounds(track) => {
                write!(f, "{track} track offset points out of bounds")
            }
            Self::InvalidPatternBounds(track) => {
                write!(f, "{track} track has invalid pattern payload bounds")
            }
            Self::InvalidPatternIndex {
                track,
                step,
                index,
                unique_count,
            } => write!(
                f,
                "{track} step {step} pattern index {index} out of bounds ({unique_count} unique patterns)"
            ),
            Self::UnexpectedEof(stream) => {
                write!(f, "Unexpected EOF in {stream} stream")
            }
            Self::MissingPayloadByte(desc) => {
                write!(f, "Missing payload byte in stream: {desc}")
            }
            Self::TooManyUniquePatterns { count, max } => {
                write!(f, "Track exceeds maximum of {max} unique patterns (got {count})")
            }
            Self::EmptySequence => write!(f, "Cannot compile empty song sequence"),
            Self::InvalidPatternFrames => write!(f, "Pattern frames must be greater than zero"),
            Self::SongTooLong { patterns, max } => write!(
                f,
                "Song length ({patterns} patterns) exceeds maximum {max} steps"
            ),
            Self::SongBinaryTooLarge { size, max } => write!(
                f,
                "Compiled song ({size} bytes) exceeds the YSG {max}-byte limit; try --step 2, a different --pattern-frames, or --max-bytes to truncate"
            ),
            Self::NoOptimalPatternFound => {
                write!(f, "Could not find a valid pattern size fitting sequence length limits")
            }
            Self::InvalidYfxSize(size) => {
                write!(f, "YFX file size ({size} bytes) must be a multiple of 5")
            }
            Self::UnsupportedExtension(ext) => {
                write!(f, "Unsupported file extension '.{ext}'")
            }
            Self::UnsupportedYmFormat(msg) => write!(f, "Unsupported YM format: {msg}"),
            Self::EmptyBank => write!(f, "AYFX sound effect bank is empty"),
            Self::MalformedBank(msg) => write!(f, "Malformed AYFX bank: {msg}"),
            Self::MalformedCsv { line, message } => {
                write!(f, "Malformed CSV at line {line}: {message}")
            }
            Self::InvalidVgmMagic(magic) => write!(
                f,
                "Invalid VGM magic: {:02X} {:02X} {:02X} {:02X}",
                magic[0], magic[1], magic[2], magic[3]
            ),
            Self::MissingAy8910Clock => write!(
                f,
                "VGM file has no AY-3-8910 / YM2149 clock (need v1.51+ with a non-zero clock at offset 0x74)"
            ),
            Self::UnsupportedVgmCommand(opcode) => {
                write!(f, "Unsupported VGM command 0x{opcode:02X}")
            }
            Self::DecompressedTooLarge { max } => {
                write!(f, "Decompressed data exceeds {max} bytes safety cap")
            }
            Self::TooManyFrames { max } => {
                write!(f, "Song exceeds {max} playback frames safety cap")
            }
            Self::BankIndexOutOfBounds { index, count } => {
                write!(f, "Bank index {index} out of bounds ({count} effects in bank)")
            }
            Self::NoSfxLoaded => write!(f, "No sound effects were loaded"),
            Self::Generic(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for YmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Utf8(err) => Some(err),
            Self::ParseInt(err) => Some(err),
            Self::Replayer(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for YmError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for YmError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<std::str::Utf8Error> for YmError {
    fn from(err: std::str::Utf8Error) -> Self {
        Self::Utf8(err)
    }
}

impl From<std::num::ParseIntError> for YmError {
    fn from(err: std::num::ParseIntError) -> Self {
        Self::ParseInt(err)
    }
}

impl From<ym2149_ym_replayer::ReplayerError> for YmError {
    fn from(err: ym2149_ym_replayer::ReplayerError) -> Self {
        Self::Replayer(err)
    }
}

/// Convenience result alias using `YmError`.
pub type Result<T> = std::result::Result<T, YmError>;
