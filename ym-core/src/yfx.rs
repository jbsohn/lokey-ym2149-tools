use crate::sequence::{SfxFrame, SfxSequence};
use crate::timing::ZX_SPECTRUM_CLOCK;
use crate::traits::{SfxFile, SfxInput};

/// Represents a compiled Lokey YM-2149 sound effect (.yfx) binary container.
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
    type Error = Box<dyn std::error::Error>;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if !bytes.len().is_multiple_of(5) {
            return Err("YFX file size must be a multiple of 5".into());
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
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Self::try_from(bytes)
    }

    /// Compiles an `SfxSequence` into a `YfxFile` 5-byte binary container.
    #[must_use]
    pub fn from_sequence(sequence: &SfxSequence) -> Self {
        let mut compiled_bytes = Vec::with_capacity(sequence.frames.len() * 5);

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

        Self {
            bytes: compiled_bytes,
        }
    }

    /// Returns the number of 5-byte frames in the compiled container.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.bytes.len() / 5
    }
}

impl SfxFile for YfxFile {
    fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    fn to_sequence(&self, name: &str) -> Result<SfxSequence, Box<dyn std::error::Error>> {
        let mut frames = Vec::with_capacity(self.frame_count());
        let mut pp = 0;

        while pp < self.bytes.len() {
            let tone_low = self.bytes[pp];
            let tone_high = self.bytes[pp + 1];
            let volume = self.bytes[pp + 2];
            let control = self.bytes[pp + 3];
            let duration = self.bytes[pp + 4];
            pp += 5;

            let tone = u16::from(tone_low) | (u16::from(tone_high) << 8);
            let tone_enable = (control & 0x01) != 0;
            let noise_enable = (control & 0x02) != 0;
            let noise = (control >> 3) & 0x1F;

            frames.push(SfxFrame::new(
                tone_enable,
                noise_enable,
                tone,
                noise,
                volume,
                duration,
            ));
        }

        Ok(SfxSequence {
            name: name.to_string(),
            source_clock: ZX_SPECTRUM_CLOCK,
            source_hz: 50,
            priority: 1,
            preferred_channels: None,
            loop_start: None,
            frames,
        })
    }
}

impl SfxInput for YfxFile {
    fn to_sequences(&self) -> Result<Vec<SfxSequence>, Box<dyn std::error::Error>> {
        let seq = self.to_sequence("sfx")?;
        Ok(vec![seq])
    }
}
