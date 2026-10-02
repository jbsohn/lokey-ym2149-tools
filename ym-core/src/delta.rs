use crate::sequence::{SfxSequence, YmSequence};

/// Bit 15 of the 16-bit frame mask — signals an RLE idle-run token.
pub const RLE_FLAG: u16 = 0x8000;

/// Controls how much compression is applied when compiling a song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// Full: delta encoding + pattern deduplication (default).
    Full,
    /// Delta only: delta-encode each frame but use one single pattern (no dedup, no mid-song boundary resets).
    DeltaOnly,
    /// None: write all 14 registers every frame (no delta, no dedup). Raw register stream.
    None,
}

/// Fine-grained feature flags for [`DeltaCompiler::compile_song`].
///
/// Use `CompilerOptions::default()` to get all active techniques enabled.
/// Pass `--no-X` CLI flags to disable individual techniques for debugging.
#[derive(Debug, Clone)]
pub struct CompilerOptions {
    /// Enable pattern deduplication (default: true).
    pub dedup: bool,
    /// Enable idle-frame RLE (default: false until implemented).
    pub rle: bool,
    /// Enable variable-length mask (default: false until implemented).
    pub varlength_mask: bool,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            dedup: true,
            rle: true,
            varlength_mask: false,
        }
    }
}

/// Platform-agnostic delta-mask compiler for YM-2149 register updates.
#[derive(Debug, Default)]
pub struct DeltaCompiler;

/// Result of [`DeltaCompiler::compile_song`]: the compiled YSG payload plus the
/// pattern size it chose, so callers can report or log it as they see fit.
#[derive(Debug, Clone)]
pub struct YmSongDetails {
    pub bytes: Vec<u8>,
    pub pattern_size: usize,
}

impl DeltaCompiler {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Compiles a sound effect sequence into a 5-byte fixed-width frame representation.
    #[must_use]
    pub fn compile_sfx(&self, sequence: &SfxSequence) -> Vec<u8> {
        let mut compiled_bytes = Vec::new();

        let mut active_tone = 0u16;
        let mut active_volume = 0u8;
        let mut active_tone_enable = true;
        let mut active_noise_enable = false;
        let mut active_noise_period = 0u8;

        for frame in &sequence.frames {
            if let Some(t) = frame.tone {
                active_tone = t;
            }
            if let Some(v) = frame.volume {
                active_volume = v;
            }
            if let Some(te) = frame.tone_enable {
                active_tone_enable = te;
            }
            if let Some(ne) = frame.noise_enable {
                active_noise_enable = ne;
            }
            if let Some(n) = frame.noise {
                active_noise_period = n;
            }

            let tone_low = (active_tone & 0xFF) as u8;
            let tone_high = ((active_tone >> 8) & 0x0F) as u8;

            let mut control = 0u8;
            if active_tone_enable {
                control |= 0x01;
            }
            if active_noise_enable {
                control |= 0x02;
            }
            control |= (active_noise_period & 0x1F) << 3;

            let duration = frame.duration.unwrap_or(1);

            compiled_bytes.push(tone_low);
            compiled_bytes.push(tone_high);
            compiled_bytes.push(active_volume & 0x1F);
            compiled_bytes.push(control);
            compiled_bytes.push(duration);
        }

        compiled_bytes
    }

    /// Compiles a music song sequence into a YSG binary payload using the channel-split format.
    ///
    /// # Errors
    /// Returns an error if the song has no frames or cannot be compressed within limits.
    pub fn compile_song(
        &self,
        sequence: &YmSequence,
        _level: CompressionLevel,
        _options: &CompilerOptions,
    ) -> Result<YmSongDetails, Box<dyn std::error::Error>> {
        let details = crate::ysg::compile_ysg_optimal(sequence)?;
        Ok(YmSongDetails {
            bytes: details.bytes,
            pattern_size: details.pattern_frames as usize,
        })
    }
}
