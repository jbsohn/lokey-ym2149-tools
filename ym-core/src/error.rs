use thiserror::Error;

/// Strongly-typed error enum for operations across `ym-core`.
#[derive(Debug, Error)]
pub enum YmError {
    /// An I/O error occurred while reading or writing data.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// UTF-8 string decoding failed.
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// Integer parsing failed.
    #[error("Parse int error: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    /// Underlying YM replayer failed.
    #[error("YM replayer error: {0}")]
    Replayer(#[from] ym2149_ym_replayer::ReplayerError),

    /// The file magic bytes do not match the expected format identifier.
    #[error("Invalid magic header bytes: 0x{:02X}{:02X}", .0[0], .0[1])]
    InvalidMagic([u8; 2]),

    /// The container format version is unsupported.
    #[error("Unsupported YSG version: {0}")]
    UnsupportedVersion(u8),

    /// Header was truncated before the full fixed-size structure was read.
    #[error("File truncated before header completed (expected {expected} bytes, got {actual})")]
    TruncatedHeader { expected: usize, actual: usize },

    /// Track descriptor header or offset table was truncated.
    #[error("{0} track descriptor truncated before offset table")]
    TruncatedTrackDescriptor(&'static str),

    /// Track offset points out of bounds of the file or track table.
    #[error("{0} track offset points out of bounds")]
    TrackOffsetOutOfBounds(&'static str),

    /// Pattern payload bounds are invalid (e.g. end < start, or beyond track length).
    #[error("{0} track has invalid pattern payload bounds")]
    InvalidPatternBounds(&'static str),

    /// Pattern index in sequence table is out of bounds of the unique patterns pool.
    #[error(
        "{track} step {step} pattern index {index} out of bounds ({unique_count} unique patterns)"
    )]
    InvalidPatternIndex {
        track: &'static str,
        step: usize,
        index: u8,
        unique_count: usize,
    },

    /// Opcode stream reached unexpected end of input.
    #[error("Unexpected EOF in {0} stream")]
    UnexpectedEof(&'static str),

    /// Expected a register payload byte following an active opcode.
    #[error("Missing payload byte in stream: {0}")]
    MissingPayloadByte(&'static str),

    /// The number of unique patterns exceeds the 8-bit table capacity.
    #[error("Track exceeds maximum of {max} unique patterns (got {count})")]
    TooManyUniquePatterns { count: usize, max: usize },

    /// Song contains no frames to compile.
    #[error("Cannot compile empty song sequence")]
    EmptySequence,

    /// Pattern frames per chunk must be greater than zero.
    #[error("Pattern frames must be greater than zero")]
    InvalidPatternFrames,

    /// Sequence length (number of patterns) exceeds the 8-bit maximum (255 steps).
    #[error("Song length ({patterns} patterns) exceeds maximum {max} steps")]
    SongTooLong { patterns: usize, max: usize },

    /// Compiled song exceeds the YSG maximum file size limit.
    #[error(
        "Compiled song ({size} bytes) exceeds the YSG {max}-byte limit; try --step 2, a different --pattern-frames, or --max-bytes to truncate"
    )]
    SongBinaryTooLarge { size: usize, max: usize },

    /// Automated candidate search failed to find a valid pattern frame size.
    #[error("Could not find a valid pattern size fitting sequence length limits")]
    NoOptimalPatternFound,

    /// YFX binary length is not a multiple of the 5-byte fixed frame stride.
    #[error("YFX file size ({0} bytes) must be a multiple of 5")]
    InvalidYfxSize(usize),

    /// File extension is not recognized as a supported song or sound effect format.
    #[error("Unsupported file extension '.{0}'")]
    UnsupportedExtension(String),

    /// YM chiptune format is unsupported.
    #[error("Unsupported YM format: {0}")]
    UnsupportedYmFormat(&'static str),

    /// AYFX sound effect bank payload is empty.
    #[error("AYFX sound effect bank is empty")]
    EmptyBank,

    /// AYFX sound effect bank payload is malformed.
    #[error("Malformed AYFX bank: {0}")]
    MalformedBank(String),

    /// AYFX CSV table parsing error.
    #[error("Malformed CSV at line {line}: {message}")]
    MalformedCsv { line: usize, message: String },

    /// The file does not begin with the VGM magic `"Vgm "`.
    #[error("Invalid VGM magic: {:02X} {:02X} {:02X} {:02X}", .0[0], .0[1], .0[2], .0[3])]
    InvalidVgmMagic([u8; 4]),

    /// VGM v1.51+ header has no AY-3-8910 / YM2149 clock, or the clock is zero.
    #[error(
        "VGM file has no AY-3-8910 / YM2149 clock (need v1.51+ with a non-zero clock at offset 0x74)"
    )]
    MissingAy8910Clock,

    /// Command byte is not a documented VGM opcode, so the stream can no longer be aligned.
    #[error("Unsupported VGM command 0x{0:02X}")]
    UnsupportedVgmCommand(u8),

    /// Decompressed stream exceeds safety limit.
    #[error("Decompressed data exceeds {max} bytes safety cap")]
    DecompressedTooLarge { max: usize },

    /// Song contains more playback frames than the safety limit.
    #[error("Song exceeds {max} playback frames safety cap")]
    TooManyFrames { max: usize },

    /// Requested sound effect bank index is out of bounds.
    #[error("Bank index {index} out of bounds ({count} effects in bank)")]
    BankIndexOutOfBounds { index: usize, count: usize },

    /// No sound effects were loaded from input files.
    #[error("No sound effects were loaded")]
    NoSfxLoaded,
}

/// Convenience result alias using `YmError`.
pub type Result<T> = std::result::Result<T, YmError>;
