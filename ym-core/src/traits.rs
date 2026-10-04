use crate::sequence::{SfxSequence, YmSequence};

/// Interface for input chiptune formats (.ym, .json, future formats).
///
/// Any source format that can produce metadata and unroll into the
/// canonical `YmSequence` IR implements this trait.
pub trait SongInput {
    /// Human-readable title or song identifier.
    fn title(&self) -> &str;

    /// Native master clock frequency of the chiptune (e.g. 2,000,000 Hz for Atari ST).
    fn source_clock(&self) -> u32;

    /// Native playback frame rate in Hz (e.g. 50 Hz or 60 Hz).
    fn source_hz(&self) -> u16;

    /// Unrolls and normalizes the song into the universal `YmSequence` IR.
    /// If `target_clock` is provided and differs from `source_clock`,
    /// tone periods are automatically retuned to preserve pitch.
    ///
    /// # Errors
    /// Returns an error if conversion or pitch retuning fails.
    fn to_sequence(
        &self,
        target_clock: Option<u32>,
    ) -> Result<YmSequence, Box<dyn std::error::Error>>;
}

/// Interface for compiled 8-bit replayable song files (.ysg, future targets).
///
/// Any concrete binary song container that can serialize to bytes,
/// report hardware clock requirements, and decompile back to `YmSequence`
/// implements this trait.
pub trait SongFile {
    /// Serializes the structured container into its on-disk binary byte representation.
    fn to_bytes(&self) -> Vec<u8>;

    /// Native playback frame rate in Hz (e.g. 50, 60, or 25).
    fn frame_rate_hz(&self) -> u16;

    /// Target hardware master clock in Hz (e.g. 1,789,773 Hz for Atari 7800).
    fn master_clock_hz(&self) -> u32;

    /// Decompiles the compressed patterns and tracks back into the universal `YmSequence` IR.
    ///
    /// # Errors
    /// Returns an error if pattern streams or track descriptors are corrupted.
    fn to_sequence(&self, name: &str) -> Result<YmSequence, Box<dyn std::error::Error>>;
}

/// Interface for sound effect input formats (.afx, .afb, .csv, .json, future formats).
///
/// Any source format that can produce one or more sound effect sequences
/// unrolls into the canonical `SfxSequence` IR via this trait.
pub trait SfxInput {
    /// Unrolls and converts the sound effect source into one or more `SfxSequence` objects.
    ///
    /// # Errors
    /// Returns an error if parsing or frame decoding fails.
    fn to_sequences(&self) -> Result<Vec<SfxSequence>, Box<dyn std::error::Error>>;
}

/// Interface for compiled 8-bit replayable sound effect files (.yfx, future targets).
///
/// Any concrete binary sound effect container that can serialize to bytes
/// and decode back to `SfxSequence` implements this trait.
pub trait SfxFile {
    /// Serializes the sound effect container into its on-disk binary byte representation.
    fn to_bytes(&self) -> Vec<u8>;

    /// Decompiles the sound effect container back into an `SfxSequence`.
    ///
    /// # Errors
    /// Returns an error if the container payload is malformed or corrupted.
    fn to_sequence(&self, name: &str) -> Result<SfxSequence, Box<dyn std::error::Error>>;
}
