use std::fmt;

/// Strongly-typed error enum for operations across `ym-core`.
#[derive(Debug)]
pub enum YmError {
    /// An I/O error occurred while reading or writing data.
    Io(std::io::Error),
    /// JSON serialization or deserialization failed.
    Json(serde_json::Error),
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
    /// Automated candidate search failed to find a valid pattern frame size.
    NoOptimalPatternFound,
    /// YFX binary length is not a multiple of the 5-byte fixed frame stride.
    InvalidYfxSize(usize),
    /// File extension is not recognized as a supported song or sound effect format.
    UnsupportedExtension(String),
    /// AYFX sound effect bank payload is empty or malformed.
    MalformedBank(String),
    /// Generic or informational error message.
    Generic(String),
}

impl fmt::Display for YmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
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
            Self::NoOptimalPatternFound => {
                write!(f, "Could not find a valid pattern size fitting sequence length limits")
            }
            Self::InvalidYfxSize(size) => {
                write!(f, "YFX file size ({size} bytes) must be a multiple of 5")
            }
            Self::UnsupportedExtension(ext) => {
                write!(f, "Unsupported file extension '.{ext}'")
            }
            Self::MalformedBank(msg) => write!(f, "Malformed AYFX bank: {msg}"),
            Self::Generic(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for YmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
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

impl From<&str> for YmError {
    fn from(s: &str) -> Self {
        Self::Generic(s.to_string())
    }
}

impl From<String> for YmError {
    fn from(s: String) -> Self {
        Self::Generic(s)
    }
}

impl From<Box<dyn std::error::Error>> for YmError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        Self::Generic(err.to_string())
    }
}

/// Convenience result alias using `YmError`.
pub type Result<T> = std::result::Result<T, YmError>;
