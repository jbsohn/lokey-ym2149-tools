use crate::error::YmError;
use crate::sequence::{SfxFrame, SfxSequence};

/// Represents a compiled Lokey YM-2149 sound effect (`.yfx`) binary container.
///
/// Encapsulates the 5-byte fixed-width frame representation:
/// - Byte 0: Tone period LSB (R0, R2, or R4)
/// - Byte 1: Tone period MSB (R1, R3, or R5)
/// - Byte 2: Volume / Mode (R8, R9, or R10)
/// - Byte 3: Control bitmask (bit 0 = tone enable, bit 1 = noise enable, bits 3..7 = noise period)
/// - Byte 4: Frame duration (ticks)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YfxFile {
    pub bytes: Vec<u8>,
}

impl TryFrom<&[u8]> for YfxFile {
    type Error = YmError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if !bytes.len().is_multiple_of(5) {
            return Err(YmError::InvalidYfxSize(bytes.len()));
        }
        Ok(Self {
            bytes: bytes.to_vec(),
        })
    }
}

impl YfxFile {
    /// Instantiates a `YfxFile` from raw bytes, verifying the length is a multiple of 5.
    ///
    /// # Errors
    /// Returns an error if `bytes.len()` is not a multiple of 5.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, YmError> {
        Self::try_from(bytes)
    }

    /// Compiles an `SfxSequence` into a `YfxFile` 5-byte binary container.
    pub fn from_sequence(sequence: &SfxSequence) -> Self {
        let mut compiled_bytes = Vec::with_capacity(sequence.frames.len() * 5);

        for &frame in &sequence.frames {
            let tone_low = (frame.tone & 0xFF) as u8;
            let tone_high = ((frame.tone >> 8) & 0x0F) as u8;

            let mut control = 0u8;
            if frame.tone_enable {
                control |= 0x01;
            }
            if frame.noise_enable {
                control |= 0x02;
            }
            control |= (frame.noise & 0x1F) << 3;

            compiled_bytes.push(tone_low);
            compiled_bytes.push(tone_high);
            compiled_bytes.push(frame.volume & 0x1F);
            compiled_bytes.push(control);
            compiled_bytes.push(frame.tick_count());
        }

        Self {
            bytes: compiled_bytes,
        }
    }

    /// Returns the number of 5-byte frames in the compiled container.
    pub fn frame_count(&self) -> usize {
        self.bytes.len() / 5
    }

    /// Borrows the compiled `.yfx` bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the compiled `.yfx` bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// Decodes this `.yfx` payload into an [`SfxSequence`] named `name`.
    pub fn to_sequence(&self, name: &str) -> SfxSequence {
        let frames = self
            .bytes
            .chunks_exact(5)
            .map(|chunk| {
                let tone = u16::from(chunk[0]) | (u16::from(chunk[1]) << 8);
                let volume = chunk[2];
                let control = chunk[3];
                let duration = chunk[4];
                let tone_enable = (control & 0x01) != 0;
                let noise_enable = (control & 0x02) != 0;
                let noise = (control >> 3) & 0x1F;
                SfxFrame::new(tone_enable, noise_enable, tone, noise, volume, duration)
            })
            .collect();

        SfxSequence::new(name, frames)
    }
}
