use crate::sequence::{YmFrame, YmSequence};
use crate::timing::{SystemHz, TimingConfig};
use crate::traits::SongFile;

/// The fixed size in bytes of the YSG container header.
pub const YSG_HEADER_SIZE: usize = 20;

/// Magic bytes identifying a YSG binary stream ("YS").
pub const YSG_MAGIC: [u8; 2] = *b"YS";

/// Current YSG container version.
pub const YSG_VERSION: u8 = 0x01;

/// Largest YSG container the format can address: header track offsets and per-track
/// pattern offsets are 16-bit, so every byte of the file must sit below 64 KB.
pub const YSG_MAX_FILE_SIZE: usize = 0xFFFF;

/// Sentinel sequence table value indicating an entirely silent/empty pattern.
pub const SENTINEL_EMPTY_PATTERN: u8 = 0xFF;

/// Header structure for a YSG (YM Song) binary stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YsgHeader {
    pub pattern_frames: u8,
    pub seq_len: u8,
    pub loop_step: u8,
    pub frame_rate_hz: u16,
    pub master_clock_hz: u32,
    pub offset_track_a: u16,
    pub offset_track_b: u16,
    pub offset_track_c: u16,
    pub offset_track_global: u16,
}

impl YsgHeader {
    /// Serializes the header into its 20-byte binary representation.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; YSG_HEADER_SIZE] {
        let mut bytes = [0u8; YSG_HEADER_SIZE];
        bytes[0] = YSG_MAGIC[0];
        bytes[1] = YSG_MAGIC[1];
        bytes[2] = YSG_VERSION;
        bytes[3] = self.pattern_frames;
        bytes[4] = self.seq_len;
        bytes[5] = self.loop_step;

        let hz_bytes = self.frame_rate_hz.to_le_bytes();
        bytes[6] = hz_bytes[0];
        bytes[7] = hz_bytes[1];

        let clock_bytes = self.master_clock_hz.to_le_bytes();
        bytes[8..12].copy_from_slice(&clock_bytes);

        let off_a = self.offset_track_a.to_le_bytes();
        bytes[12] = off_a[0];
        bytes[13] = off_a[1];

        let off_b = self.offset_track_b.to_le_bytes();
        bytes[14] = off_b[0];
        bytes[15] = off_b[1];

        let off_c = self.offset_track_c.to_le_bytes();
        bytes[16] = off_c[0];
        bytes[17] = off_c[1];

        let off_glob = self.offset_track_global.to_le_bytes();
        bytes[18] = off_glob[0];
        bytes[19] = off_glob[1];

        bytes
    }
}

impl TryFrom<&[u8]> for YsgHeader {
    type Error = Box<dyn std::error::Error>;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() < YSG_HEADER_SIZE {
            return Err("YSG file truncated before header completed".into());
        }
        if bytes[0..2] != YSG_MAGIC {
            return Err(
                format!("Invalid YSG magic: expected 'YS', found {:?}", &bytes[0..2]).into(),
            );
        }
        if bytes[2] != YSG_VERSION {
            return Err(format!("Unsupported YSG version: {}", bytes[2]).into());
        }

        let pattern_frames = bytes[3];
        let seq_len = bytes[4];
        let loop_step = bytes[5];
        let frame_rate_hz = u16::from_le_bytes([bytes[6], bytes[7]]);
        let master_clock_hz = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let offset_track_a = u16::from_le_bytes([bytes[12], bytes[13]]);
        let offset_track_b = u16::from_le_bytes([bytes[14], bytes[15]]);
        let offset_track_c = u16::from_le_bytes([bytes[16], bytes[17]]);
        let offset_track_global = u16::from_le_bytes([bytes[18], bytes[19]]);

        Ok(Self {
            pattern_frames,
            seq_len,
            loop_step,
            frame_rate_hz,
            master_clock_hz,
            offset_track_a,
            offset_track_b,
            offset_track_c,
            offset_track_global,
        })
    }
}

impl YsgHeader {
    /// Deserializes a header from a byte slice.
    ///
    /// # Errors
    /// Returns an error if the slice is too short, magic doesn't match, or version is unsupported.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Self::try_from(bytes)
    }
}

/// Intermediate per-frame state for a single voice (Channel A, B, or C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VoiceFrame {
    pub period: u16,
    pub volume: u8,
    pub envelope_mode: bool,
}

impl VoiceFrame {
    /// Encodes a slice of voice frames for a pattern chunk into compressed opcode bytes.
    #[must_use]
    pub fn encode_pattern(frames: &[Self]) -> Vec<u8> {
        encode_voice_pattern(frames)
    }

    /// Decodes a stream of voice opcode bytes into intermediate voice frames.
    ///
    /// # Errors
    /// Returns an error if the opcode stream is malformed or truncated.
    pub fn decode_stream(
        bytes: &[u8],
        frame_count: usize,
    ) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        decode_voice_stream(bytes, frame_count)
    }
}

/// Intermediate per-frame state for the global PSG subsystem (Noise, Mixer, Envelope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalFrame {
    pub noise_period: u8,
    pub mixer: u8,
    pub envelope_period: u16,
    pub envelope_shape: Option<u8>,
}

impl GlobalFrame {
    /// Encodes a slice of global frames for a pattern chunk into compressed opcode bytes.
    #[must_use]
    pub fn encode_pattern(frames: &[Self]) -> Vec<u8> {
        encode_global_pattern(frames)
    }

    /// Decodes a stream of global opcode bytes into intermediate global frames.
    ///
    /// # Errors
    /// Returns an error if the opcode stream is malformed or truncated.
    pub fn decode_stream(
        bytes: &[u8],
        frame_count: usize,
    ) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        decode_global_stream(bytes, frame_count)
    }
}

impl Default for GlobalFrame {
    fn default() -> Self {
        Self {
            noise_period: 0,
            mixer: 0x3F, // All channels muted by default
            envelope_period: 0,
            envelope_shape: None,
        }
    }
}

/// Decomposes a slice of high-level [`YmFrame`] records into 4 parallel tracks.
#[must_use]
pub fn decompose_frames(
    frames: &[YmFrame],
) -> (
    Vec<VoiceFrame>,
    Vec<VoiceFrame>,
    Vec<VoiceFrame>,
    Vec<GlobalFrame>,
) {
    let mut track_a = Vec::with_capacity(frames.len());
    let mut track_b = Vec::with_capacity(frames.len());
    let mut track_c = Vec::with_capacity(frames.len());
    let mut track_global = Vec::with_capacity(frames.len());

    let mut state_a = VoiceFrame::default();
    let mut state_b = VoiceFrame::default();
    let mut state_c = VoiceFrame::default();
    let mut state_glob = GlobalFrame::default();

    for frame in frames {
        // Channel A
        if let Some(t) = frame.tone_a {
            state_a.period = t & 0x0FFF;
        }
        if let Some(v) = frame.volume_a {
            state_a.volume = v & 0x0F;
            state_a.envelope_mode = (v & 0x10) != 0;
        }

        // Channel B
        if let Some(t) = frame.tone_b {
            state_b.period = t & 0x0FFF;
        }
        if let Some(v) = frame.volume_b {
            state_b.volume = v & 0x0F;
            state_b.envelope_mode = (v & 0x10) != 0;
        }

        // Channel C
        if let Some(t) = frame.tone_c {
            state_c.period = t & 0x0FFF;
        }
        if let Some(v) = frame.volume_c {
            state_c.volume = v & 0x0F;
            state_c.envelope_mode = (v & 0x10) != 0;
        }

        // Global / Shared
        if let Some(np) = frame.noise_period {
            state_glob.noise_period = np & 0x1F;
        }
        if let Some(ep) = frame.envelope_period {
            state_glob.envelope_period = ep;
        }
        state_glob.envelope_shape = frame.envelope_shape;

        // Mixer R7 reconstruction (active-low bits: 0 = enabled, 1 = disabled)
        // Bit 0: Tone A, Bit 1: Tone B, Bit 2: Tone C
        // Bit 3: Noise A, Bit 4: Noise B, Bit 5: Noise C
        let mut mixer = state_glob.mixer;
        if let Some(te) = frame.tone_enable_a {
            if te {
                mixer &= !0x01;
            } else {
                mixer |= 0x01;
            }
        }
        if let Some(te) = frame.tone_enable_b {
            if te {
                mixer &= !0x02;
            } else {
                mixer |= 0x02;
            }
        }
        if let Some(te) = frame.tone_enable_c {
            if te {
                mixer &= !0x04;
            } else {
                mixer |= 0x04;
            }
        }
        if let Some(ne) = frame.noise_enable_a {
            if ne {
                mixer &= !0x08;
            } else {
                mixer |= 0x08;
            }
        }
        if let Some(ne) = frame.noise_enable_b {
            if ne {
                mixer &= !0x10;
            } else {
                mixer |= 0x10;
            }
        }
        if let Some(ne) = frame.noise_enable_c {
            if ne {
                mixer &= !0x20;
            } else {
                mixer |= 0x20;
            }
        }
        state_glob.mixer = mixer;

        track_a.push(state_a);
        track_b.push(state_b);
        track_c.push(state_c);
        track_global.push(state_glob);
    }

    (track_a, track_b, track_c, track_global)
}

/// Encodes a single voice pattern chunk of `pattern_frames` length into compressed bytes.
///
/// Ensures the Pattern Boundary Invariant:
/// - If frame 0 is silent, emits an explicit opcode to mute the voice (volume 0),
///   preventing notes from the prior pattern from leaking.
/// - The first sounding frame forces both Period LSB and MSB to be written,
///   guaranteeing pitch is fully initialized regardless of prior pattern state.
/// - Wait tokens never exceed the pattern chunk boundary.
#[must_use]
pub fn encode_voice_pattern(frames: &[VoiceFrame]) -> Vec<u8> {
    let mut output = Vec::new();
    let pat_len = frames.len();
    if pat_len == 0 {
        return output;
    }

    let first_sounding_idx = frames.iter().position(|f| f.volume > 0 || f.envelope_mode);

    let mut i = 0;
    let mut prev_frame = VoiceFrame::default();
    let mut pitch_initialized = false;

    if let Some(sound_idx) = first_sounding_idx {
        if sound_idx > 0 {
            // Frame 0: emit active mute opcode (Bit 7=1, L=0, M=0, E=0, volume=0)
            output.push(0x80);
            prev_frame.volume = 0;
            prev_frame.envelope_mode = false;
            i = 1;

            let silent_wait = sound_idx - 1;
            if silent_wait > 0 {
                let mut rem = silent_wait;
                while rem > 0 {
                    let run = rem.min(127);
                    output.push(run as u8);
                    rem -= run;
                }
                i = sound_idx;
            }
        }
    } else {
        // Entire pattern is silent
        output.push(0x80);
        let silent_wait = pat_len - 1;
        let mut rem = silent_wait;
        while rem > 0 {
            let run = rem.min(127);
            output.push(run as u8);
            rem -= run;
        }
        return output;
    }

    while i < pat_len {
        let current = frames[i];

        if pitch_initialized && current == prev_frame {
            let mut run = 1usize;
            while i + run < pat_len && frames[i + run] == prev_frame && run < 127 {
                run += 1;
            }
            output.push(run as u8);
            i += run;
            continue;
        }

        let (lsb_changed, msb_changed) =
            if !pitch_initialized && (current.volume > 0 || current.envelope_mode) {
                pitch_initialized = true;
                (true, true)
            } else if current.volume == 0 && !current.envelope_mode {
                // Channel is silent: pitch does not need to be updated while volume is 0
                (false, false)
            } else {
                let lsb = (current.period & 0xFF) != (prev_frame.period & 0xFF);
                let msb = ((current.period >> 8) & 0x0F) != ((prev_frame.period >> 8) & 0x0F);
                (lsb, msb)
            };

        let mut opcode = 0x80u8; // Bit 7 = 1 (active)
        if lsb_changed {
            opcode |= 0x40; // Bit 6 = L
        }
        if msb_changed {
            opcode |= 0x20; // Bit 5 = M
        }
        if current.envelope_mode {
            opcode |= 0x10; // Bit 4 = E
        } else {
            opcode |= current.volume & 0x0F; // Bits 3..0 = vvvv
        }

        output.push(opcode);
        if lsb_changed {
            output.push((current.period & 0xFF) as u8);
        }
        if msb_changed {
            output.push(((current.period >> 8) & 0x0F) as u8);
        }

        if current.volume > 0 || current.envelope_mode {
            prev_frame = current;
        } else {
            prev_frame.volume = 0;
            prev_frame.envelope_mode = false;
        }
        i += 1;
    }

    output
}

/// Decodes a compressed voice stream back into per-frame [`VoiceFrame`] representations.
///
/// # Errors
/// Returns an error if the byte stream is truncated or malformed.
pub fn decode_voice_stream(
    bytes: &[u8],
    frame_count: usize,
) -> Result<Vec<VoiceFrame>, Box<dyn std::error::Error>> {
    let mut frames = Vec::with_capacity(frame_count);
    let mut current = VoiceFrame::default();
    let mut pp = 0;

    while frames.len() < frame_count {
        if pp >= bytes.len() {
            return Err("Unexpected EOF in voice stream".into());
        }

        let b = bytes[pp];
        pp += 1;

        if (b & 0x80) == 0 {
            // Wait run: b frames repeat current state
            let run = (b & 0x7F) as usize;
            for _ in 0..run {
                if frames.len() < frame_count {
                    frames.push(current);
                }
            }
            continue;
        }

        // Active opcode: 1LMEvvvv
        let lsb_changed = (b & 0x40) != 0;
        let msb_changed = (b & 0x20) != 0;
        let env_mode = (b & 0x10) != 0;

        current.envelope_mode = env_mode;
        if env_mode {
            current.volume = 0x10; // 0x10 indicates envelope mode to chip amplitude register
        } else {
            current.volume = b & 0x0F;
        }

        if lsb_changed {
            if pp >= bytes.len() {
                return Err("Missing LSB period payload byte in voice stream".into());
            }
            let lsb = u16::from(bytes[pp]);
            pp += 1;
            current.period = (current.period & 0x0F00) | lsb;
        }

        if msb_changed {
            if pp >= bytes.len() {
                return Err("Missing MSB period payload byte in voice stream".into());
            }
            let msb = u16::from(bytes[pp] & 0x0F);
            pp += 1;
            current.period = (current.period & 0x00FF) | (msb << 8);
        }

        frames.push(current);
    }

    Ok(frames)
}

/// Encodes a single global pattern chunk of `pattern_frames` length into compressed bytes.
///
/// Ensures the Pattern Boundary Invariant:
/// - Frame 0 always explicitly writes R7 (Mixer Control) to establish
///   tone/noise enable routing for the pattern.
/// - If noise or hardware envelope are active in this pattern, their initial
///   parameters are explicitly established.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn encode_global_pattern(frames: &[GlobalFrame]) -> Vec<u8> {
    let mut output = Vec::new();
    let pat_len = frames.len();
    if pat_len == 0 {
        return output;
    }

    let f0 = frames[0];
    let noise_active_f0 = (f0.mixer & 0x38) != 0x38;
    let env_active_f0 = f0.envelope_period > 0 || f0.envelope_shape.is_some();

    let mut noise_initialized = noise_active_f0;
    let mut env_initialized = env_active_f0;

    let r6_write = noise_active_f0;
    let r7_write = true; // Always establish mixer at pattern boundary
    let r11_write = env_active_f0;
    let r12_write = env_active_f0;
    let r13_retrigger = f0.envelope_shape.is_some();

    let mut mask0 = 0x80u8;
    if r6_write {
        mask0 |= 0x01;
    }
    if r7_write {
        mask0 |= 0x02;
    }
    if r11_write {
        mask0 |= 0x04;
    }
    if r12_write {
        mask0 |= 0x08;
    }
    if r13_retrigger {
        mask0 |= 0x10;
    }

    output.push(mask0);
    if r6_write {
        output.push(f0.noise_period & 0x1F);
    }
    if r7_write {
        output.push(f0.mixer);
    }
    if r11_write {
        output.push((f0.envelope_period & 0xFF) as u8);
    }
    if r12_write {
        output.push(((f0.envelope_period >> 8) & 0xFF) as u8);
    }
    if let Some(shape) = f0.envelope_shape {
        output.push(shape & 0x0F);
    }

    let mut prev_frame = f0;
    prev_frame.envelope_shape = None;
    let mut i = 1;

    while i < pat_len {
        let current = frames[i];
        let noise_active = (current.mixer & 0x38) != 0x38;
        let env_active = current.envelope_period > 0 || current.envelope_shape.is_some();

        let force_noise = noise_active && !noise_initialized;
        let force_env = env_active && !env_initialized;

        let is_idle = !force_noise
            && !force_env
            && current.noise_period == prev_frame.noise_period
            && current.mixer == prev_frame.mixer
            && current.envelope_period == prev_frame.envelope_period
            && current.envelope_shape.is_none();

        if is_idle {
            let mut run = 1usize;
            while i + run < pat_len {
                let next = frames[i + run];
                let next_noise_active = (next.mixer & 0x38) != 0x38;
                let next_env_active = next.envelope_period > 0 || next.envelope_shape.is_some();
                let next_force_noise = next_noise_active && !noise_initialized;
                let next_force_env = next_env_active && !env_initialized;

                if !next_force_noise
                    && !next_force_env
                    && next.noise_period == prev_frame.noise_period
                    && next.mixer == prev_frame.mixer
                    && next.envelope_period == prev_frame.envelope_period
                    && next.envelope_shape.is_none()
                    && run < 127
                {
                    run += 1;
                } else {
                    break;
                }
            }
            output.push(run as u8);
            i += run;
            continue;
        }

        let r6_changed = force_noise || (current.noise_period != prev_frame.noise_period);
        let r7_changed = current.mixer != prev_frame.mixer;
        let r11_changed =
            force_env || ((current.envelope_period & 0xFF) != (prev_frame.envelope_period & 0xFF));
        let r12_changed = force_env
            || (((current.envelope_period >> 8) & 0xFF)
                != ((prev_frame.envelope_period >> 8) & 0xFF));
        let r13_retrigger = current.envelope_shape.is_some();

        if noise_active {
            noise_initialized = true;
        }
        if env_active {
            env_initialized = true;
        }

        let mut mask = 0x80u8;
        if r6_changed {
            mask |= 0x01;
        }
        if r7_changed {
            mask |= 0x02;
        }
        if r11_changed {
            mask |= 0x04;
        }
        if r12_changed {
            mask |= 0x08;
        }
        if r13_retrigger {
            mask |= 0x10;
        }

        output.push(mask);
        if r6_changed {
            output.push(current.noise_period & 0x1F);
        }
        if r7_changed {
            output.push(current.mixer);
        }
        if r11_changed {
            output.push((current.envelope_period & 0xFF) as u8);
        }
        if r12_changed {
            output.push(((current.envelope_period >> 8) & 0xFF) as u8);
        }
        if let Some(shape) = current.envelope_shape {
            output.push(shape & 0x0F);
        }

        prev_frame = current;
        prev_frame.envelope_shape = None;
        i += 1;
    }

    output
}

/// Decodes a compressed global stream back into per-frame [`GlobalFrame`] representations.
///
/// # Errors
/// Returns an error if the byte stream is truncated or malformed.
pub fn decode_global_stream(
    bytes: &[u8],
    frame_count: usize,
) -> Result<Vec<GlobalFrame>, Box<dyn std::error::Error>> {
    let mut frames = Vec::with_capacity(frame_count);
    let mut current = GlobalFrame::default();
    let mut pp = 0;

    while frames.len() < frame_count {
        if pp >= bytes.len() {
            return Err("Unexpected EOF in global stream".into());
        }

        let b = bytes[pp];
        pp += 1;

        if (b & 0x80) == 0 {
            // Wait run: current state repeats for b frames (no R13 retrigger)
            let run = (b & 0x7F) as usize;
            current.envelope_shape = None;
            for _ in 0..run {
                if frames.len() < frame_count {
                    frames.push(current);
                }
            }
            continue;
        }

        // Active mask: 1-SRNM76
        let r6_changed = (b & 0x01) != 0;
        let r7_changed = (b & 0x02) != 0;
        let r11_changed = (b & 0x04) != 0;
        let r12_changed = (b & 0x08) != 0;
        let r13_retrigger = (b & 0x10) != 0;

        if r6_changed {
            if pp >= bytes.len() {
                return Err("Missing R6 payload in global stream".into());
            }
            current.noise_period = bytes[pp] & 0x1F;
            pp += 1;
        }
        if r7_changed {
            if pp >= bytes.len() {
                return Err("Missing R7 payload in global stream".into());
            }
            current.mixer = bytes[pp];
            pp += 1;
        }
        if r11_changed {
            if pp >= bytes.len() {
                return Err("Missing R11 payload in global stream".into());
            }
            let lsb = u16::from(bytes[pp]);
            pp += 1;
            current.envelope_period = (current.envelope_period & 0xFF00) | lsb;
        }
        if r12_changed {
            if pp >= bytes.len() {
                return Err("Missing R12 payload in global stream".into());
            }
            let msb = u16::from(bytes[pp]);
            pp += 1;
            current.envelope_period = (current.envelope_period & 0x00FF) | (msb << 8);
        }
        if r13_retrigger {
            if pp >= bytes.len() {
                return Err("Missing R13 payload in global stream".into());
            }
            current.envelope_shape = Some(bytes[pp] & 0x0F);
            pp += 1;
        } else {
            current.envelope_shape = None;
        }

        frames.push(current);
    }

    Ok(frames)
}

/// A serialized descriptor for a single track (Voice A, B, C, or Global).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackDescriptor {
    pub unique_patterns: Vec<Vec<u8>>,
    pub sequence_table: Vec<u8>,
}

impl TrackDescriptor {
    /// Appends an empty/silent pattern sentinel (0xFF) to the sequence table.
    pub fn push_empty_pattern(&mut self) {
        self.sequence_table.push(SENTINEL_EMPTY_PATTERN);
    }

    /// Appends a pattern payload to the track, deduplicating against existing unique patterns.
    ///
    /// # Errors
    /// Returns an error if the track exceeds the 255 unique pattern limit (0xFF is reserved for silent patterns).
    pub fn push_pattern(&mut self, pat_bytes: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(idx) = self.unique_patterns.iter().position(|p| p == &pat_bytes) {
            self.sequence_table.push(idx as u8);
        } else {
            if self.unique_patterns.len() >= 0xFF {
                return Err("Track exceeds maximum of 255 unique patterns".into());
            }
            let idx = self.unique_patterns.len() as u8;
            self.unique_patterns.push(pat_bytes);
            self.sequence_table.push(idx);
        }
        Ok(())
    }

    /// Returns the serialized size of this track descriptor without building it.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        1 + self.sequence_table.len()
            + self.unique_patterns.len() * 2
            + self.unique_patterns.iter().map(Vec::len).sum::<usize>()
    }

    /// Serializes this track descriptor into bytes matching the YSG specification.
    ///
    /// # Panics
    /// Panics if a pattern offset does not fit in 16 bits (track larger than 64 KB).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let p = self.unique_patterns.len();
        let seq_len = self.sequence_table.len();
        let mut bytes = Vec::new();

        // Offset $00: Unique Patterns (u8)
        bytes.push(p as u8);

        // Offset $01: Sequence Table (seq_len bytes)
        bytes.extend_from_slice(&self.sequence_table);

        // Offset table: P * 2 bytes
        let mut curr_offset = 1 + seq_len + (p * 2);
        for pat in &self.unique_patterns {
            let off =
                u16::try_from(curr_offset).expect("YSG pattern offset exceeds 16-bit format limit");
            bytes.extend_from_slice(&off.to_le_bytes());
            curr_offset += pat.len();
        }

        // Pattern Byte Payloads
        for pat in &self.unique_patterns {
            bytes.extend_from_slice(pat);
        }

        bytes
    }

    /// Deserializes a track descriptor from a byte slice.
    ///
    /// # Errors
    /// Returns an error if the slice is truncated or offsets are out of bounds.
    pub fn from_bytes(bytes: &[u8], seq_len: usize) -> Result<Self, Box<dyn std::error::Error>> {
        if bytes.len() < 1 + seq_len {
            return Err("Track descriptor truncated before sequence table".into());
        }
        let p = bytes[0] as usize;
        let sequence_table = bytes[1..=seq_len].to_vec();

        let offset_table_start = 1 + seq_len;
        let offset_table_end = offset_table_start + p * 2;
        if bytes.len() < offset_table_end {
            return Err("Track descriptor truncated before pattern offset table".into());
        }

        let mut offsets = Vec::with_capacity(p);
        for i in 0..p {
            let idx = offset_table_start + i * 2;
            let off = u16::from_le_bytes([bytes[idx], bytes[idx + 1]]) as usize;
            offsets.push(off);
        }

        let mut unique_patterns = Vec::with_capacity(p);
        for i in 0..p {
            let start = offsets[i];
            if start < offset_table_end || start > bytes.len() {
                return Err("Pattern offset out of bounds".into());
            }
            let end = if i + 1 < p {
                offsets[i + 1]
            } else {
                bytes.len()
            };
            if end < start || end > bytes.len() {
                return Err("Invalid pattern payload bounds".into());
            }
            unique_patterns.push(bytes[start..end].to_vec());
        }

        Ok(Self {
            unique_patterns,
            sequence_table,
        })
    }
}

/// Detailed compilation metrics and output for a YSG song compilation.
#[derive(Debug, Clone)]
pub struct YsgSongDetails {
    pub bytes: Vec<u8>,
    pub pattern_frames: u8,
    pub seq_len: u8,
    pub unique_patterns_a: usize,
    pub unique_patterns_b: usize,
    pub unique_patterns_c: usize,
    pub unique_patterns_global: usize,
    pub track_a_bytes: usize,
    pub track_b_bytes: usize,
    pub track_c_bytes: usize,
    pub track_global_bytes: usize,
}

impl TrackDescriptor {
    /// Builds a track descriptor from voice frames, chunked by `pattern_frames` and deduplicated.
    ///
    /// # Errors
    /// Returns an error if the track exceeds 254 unique patterns.
    pub fn from_voice_frames(
        frames: &[VoiceFrame],
        pattern_frames: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut track = Self::default();

        for chunk in frames.chunks(pattern_frames) {
            let is_silent = chunk.iter().all(|f| f.volume == 0 && !f.envelope_mode);
            if is_silent {
                track.push_empty_pattern();
            } else {
                track.push_pattern(VoiceFrame::encode_pattern(chunk))?;
            }
        }

        Ok(track)
    }

    /// Builds a track descriptor from global frames, chunked by `pattern_frames` and deduplicated.
    ///
    /// # Errors
    /// Returns an error if the track exceeds 254 unique patterns.
    pub fn from_global_frames(
        frames: &[GlobalFrame],
        pattern_frames: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut track = Self::default();

        for chunk in frames.chunks(pattern_frames) {
            track.push_pattern(GlobalFrame::encode_pattern(chunk))?;
        }

        Ok(track)
    }
}

/// Computes the absolute header offsets of tracks A, B, C and Global from the byte
/// lengths of the first three tracks.
///
/// # Panics
/// Panics if an offset does not fit in 16 bits. [`YsgFile::from_sequence`] rejects
/// songs larger than [`YSG_MAX_FILE_SIZE`], so this only fires for a hand-built
/// `YsgFile` that violates the format limit.
fn track_offsets(len_a: usize, len_b: usize, len_c: usize) -> [u16; 4] {
    let to_u16 =
        |off: usize| u16::try_from(off).expect("YSG track offset exceeds 16-bit format limit");
    let off_a = YSG_HEADER_SIZE;
    let off_b = off_a + len_a;
    let off_c = off_b + len_b;
    let off_glob = off_c + len_c;
    [
        to_u16(off_a),
        to_u16(off_b),
        to_u16(off_c),
        to_u16(off_glob),
    ]
}

/// Represents a compiled, relocatable YSG (YM Song) binary container.
///
/// Encapsulates the 20-byte container header and the four decoupled track descriptors
/// (Voices A, B, C, and Global).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YsgFile {
    pub header: YsgHeader,
    pub track_a: TrackDescriptor,
    pub track_b: TrackDescriptor,
    pub track_c: TrackDescriptor,
    pub track_global: TrackDescriptor,
}

impl TryFrom<&[u8]> for YsgFile {
    type Error = Box<dyn std::error::Error>;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let header = YsgHeader::from_bytes(bytes)?;
        let seq_len = header.seq_len as usize;

        let off_a = header.offset_track_a as usize;
        let off_b = header.offset_track_b as usize;
        let off_c = header.offset_track_c as usize;
        let off_glob = header.offset_track_global as usize;

        if off_a > off_b || off_b > off_c || off_c > off_glob || off_glob > bytes.len() {
            return Err("Invalid track offsets in YSG header".into());
        }

        let track_a = TrackDescriptor::from_bytes(&bytes[off_a..off_b], seq_len)?;
        let track_b = TrackDescriptor::from_bytes(&bytes[off_b..off_c], seq_len)?;
        let track_c = TrackDescriptor::from_bytes(&bytes[off_c..off_glob], seq_len)?;
        let track_global = TrackDescriptor::from_bytes(&bytes[off_glob..bytes.len()], seq_len)?;

        Ok(Self {
            header,
            track_a,
            track_b,
            track_c,
            track_global,
        })
    }
}

impl YsgFile {
    /// Deserializes and validates a YSG binary container from raw bytes.
    ///
    /// # Errors
    /// Returns an error if the container is truncated, magic is invalid, or track offsets
    /// and descriptors are malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Self::try_from(bytes)
    }

    /// Compiles an uncompressed [`YmSequence`] into a structured `YsgFile` container.
    ///
    /// # Errors
    /// Returns an error if the sequence is empty, exceeds 255 pattern steps, or unique
    /// pattern limits are exceeded.
    #[allow(clippy::similar_names)]
    pub fn from_sequence(
        song: &YmSequence,
        pattern_frames: u8,
    ) -> Result<(Self, YsgSongDetails), Box<dyn std::error::Error>> {
        if pattern_frames == 0 {
            return Err("Pattern frames must be greater than zero".into());
        }
        if song.frames.is_empty() {
            return Err("Cannot compile empty song sequence".into());
        }

        let pat_len = pattern_frames as usize;
        let padded_len = song.frames.len().next_multiple_of(pat_len);
        let mut padded_frames = song.frames.clone();
        padded_frames.resize(
            padded_len,
            YmFrame {
                volume_a: Some(0),
                volume_b: Some(0),
                volume_c: Some(0),
                ..Default::default()
            },
        );

        let seq_len = padded_len / pat_len;
        if seq_len > 255 {
            return Err(
                format!("Song length ({seq_len} patterns) exceeds maximum 255 steps").into(),
            );
        }

        let (track_a_frames, track_b_frames, track_c_frames, track_glob_frames) =
            decompose_frames(&padded_frames);

        let track_a = TrackDescriptor::from_voice_frames(&track_a_frames, pat_len)?;
        let track_b = TrackDescriptor::from_voice_frames(&track_b_frames, pat_len)?;
        let track_c = TrackDescriptor::from_voice_frames(&track_c_frames, pat_len)?;
        let track_global = TrackDescriptor::from_global_frames(&track_glob_frames, pat_len)?;

        let total_size = YSG_HEADER_SIZE
            + track_a.byte_len()
            + track_b.byte_len()
            + track_c.byte_len()
            + track_global.byte_len();
        if total_size > YSG_MAX_FILE_SIZE {
            return Err(format!(
                "Compiled song ({total_size} bytes) exceeds the YSG {YSG_MAX_FILE_SIZE}-byte limit; \
                 try --step 2, a different --pattern-frames, or --max-bytes to truncate"
            )
            .into());
        }

        let bytes_a = track_a.to_bytes();
        let bytes_b = track_b.to_bytes();
        let bytes_c = track_c.to_bytes();
        let bytes_glob = track_global.to_bytes();
        let [off_a, off_b, off_c, off_glob] =
            track_offsets(bytes_a.len(), bytes_b.len(), bytes_c.len());

        let loop_step = match song.loop_start {
            Some(frame) => {
                let step = frame / pat_len;
                if step < seq_len && step < 255 {
                    step as u8
                } else {
                    255
                }
            }
            None => 255,
        };

        let header = YsgHeader {
            pattern_frames,
            seq_len: seq_len as u8,
            loop_step,
            frame_rate_hz: song.timing.frame_rate.hz_value() as u16,
            master_clock_hz: song.timing.master_clock_hz,
            offset_track_a: off_a,
            offset_track_b: off_b,
            offset_track_c: off_c,
            offset_track_global: off_glob,
        };

        let file = Self {
            header,
            track_a,
            track_b,
            track_c,
            track_global,
        };

        let details = YsgSongDetails {
            bytes: file.to_bytes(),
            pattern_frames,
            seq_len: seq_len as u8,
            unique_patterns_a: file.track_a.unique_patterns.len(),
            unique_patterns_b: file.track_b.unique_patterns.len(),
            unique_patterns_c: file.track_c.unique_patterns.len(),
            unique_patterns_global: file.track_global.unique_patterns.len(),
            track_a_bytes: bytes_a.len(),
            track_b_bytes: bytes_b.len(),
            track_c_bytes: bytes_c.len(),
            track_global_bytes: bytes_glob.len(),
        };

        Ok((file, details))
    }

    /// Automatically finds the optimal pattern frame size that yields the smallest
    /// total compressed YSG binary size, returning the constructed `YsgFile` and details.
    ///
    /// # Errors
    /// Returns an error if no valid pattern frame size can compress the song.
    pub fn from_sequence_optimal(
        song: &YmSequence,
    ) -> Result<(Self, YsgSongDetails), Box<dyn std::error::Error>> {
        let mut best: Option<(Self, YsgSongDetails)> = None;

        for &pf in CANDIDATE_PATTERN_FRAMES {
            let pat_len = pf as usize;
            let padded_len = song.frames.len().next_multiple_of(pat_len);
            let seq_len = padded_len / pat_len;
            if seq_len > 255 {
                continue;
            }

            if let Ok((file, details)) = Self::from_sequence(song, pf) {
                if let Some((_, ref current_best_details)) = best {
                    if details.bytes.len() < current_best_details.bytes.len() {
                        best = Some((file, details));
                    }
                } else {
                    best = Some((file, details));
                }
            }
        }

        best.ok_or_else(|| {
            "Could not find a valid pattern size fitting sequence length limits".into()
        })
    }

    /// Compiles a [`YmSequence`] into a [`YsgSongDetails`] payload using the specified pattern size.
    ///
    /// # Errors
    /// Returns an error if compression fails or pattern size limits are exceeded.
    pub fn compile(
        song: &YmSequence,
        pattern_frames: u8,
    ) -> Result<YsgSongDetails, Box<dyn std::error::Error>> {
        Self::from_sequence(song, pattern_frames).map(|(_, details)| details)
    }

    /// Compiles a [`YmSequence`] into a [`YsgSongDetails`] payload, finding the optimal pattern size.
    ///
    /// # Errors
    /// Returns an error if no valid pattern size can compress the song.
    pub fn compile_optimal(
        song: &YmSequence,
    ) -> Result<YsgSongDetails, Box<dyn std::error::Error>> {
        Self::from_sequence_optimal(song).map(|(_, details)| details)
    }

    /// Decompiles a raw `.ysg` binary byte slice into a high-level [`YmSequence`].
    ///
    /// # Errors
    /// Returns an error if the container header or track descriptors are malformed.
    pub fn decompile(name: &str, bytes: &[u8]) -> Result<YmSequence, Box<dyn std::error::Error>> {
        let file = Self::from_bytes(bytes)?;
        file.to_sequence(name)
    }
}

impl SongFile for YsgFile {
    fn to_bytes(&self) -> Vec<u8> {
        let bytes_a = self.track_a.to_bytes();
        let bytes_b = self.track_b.to_bytes();
        let bytes_c = self.track_c.to_bytes();
        let bytes_glob = self.track_global.to_bytes();

        let [off_a, off_b, off_c, off_glob] =
            track_offsets(bytes_a.len(), bytes_b.len(), bytes_c.len());

        let mut header = self.header.clone();
        header.offset_track_a = off_a;
        header.offset_track_b = off_b;
        header.offset_track_c = off_c;
        header.offset_track_global = off_glob;

        let mut out = Vec::with_capacity(
            YSG_HEADER_SIZE + bytes_a.len() + bytes_b.len() + bytes_c.len() + bytes_glob.len(),
        );
        out.extend_from_slice(&header.to_bytes());
        out.extend_from_slice(&bytes_a);
        out.extend_from_slice(&bytes_b);
        out.extend_from_slice(&bytes_c);
        out.extend_from_slice(&bytes_glob);
        out
    }

    fn frame_rate_hz(&self) -> u16 {
        self.header.frame_rate_hz
    }

    fn master_clock_hz(&self) -> u32 {
        self.header.master_clock_hz
    }

    #[allow(clippy::similar_names, clippy::too_many_lines)]
    fn to_sequence(&self, name: &str) -> Result<YmSequence, Box<dyn std::error::Error>> {
        let seq_len = self.header.seq_len as usize;
        let pat_len = self.header.pattern_frames as usize;
        let total_frames = seq_len * pat_len;

        let mut voice_a = Vec::with_capacity(total_frames);
        let mut voice_b = Vec::with_capacity(total_frames);
        let mut voice_c = Vec::with_capacity(total_frames);
        let mut global_frames = Vec::with_capacity(total_frames);

        for s in 0..seq_len {
            // Track A
            let pat_a = self.track_a.sequence_table[s];
            if pat_a == SENTINEL_EMPTY_PATTERN {
                voice_a.resize(voice_a.len() + pat_len, VoiceFrame::default());
            } else {
                let pat_bytes = self
                    .track_a
                    .unique_patterns
                    .get(pat_a as usize)
                    .ok_or_else(|| {
                        format!("Track A step {s} pattern index {pat_a} out of bounds")
                    })?;
                voice_a.extend(decode_voice_stream(pat_bytes, pat_len)?);
            }

            // Track B
            let pat_b = self.track_b.sequence_table[s];
            if pat_b == SENTINEL_EMPTY_PATTERN {
                voice_b.resize(voice_b.len() + pat_len, VoiceFrame::default());
            } else {
                let pat_bytes = self
                    .track_b
                    .unique_patterns
                    .get(pat_b as usize)
                    .ok_or_else(|| {
                        format!("Track B step {s} pattern index {pat_b} out of bounds")
                    })?;
                voice_b.extend(decode_voice_stream(pat_bytes, pat_len)?);
            }

            // Track C
            let pat_c = self.track_c.sequence_table[s];
            if pat_c == SENTINEL_EMPTY_PATTERN {
                voice_c.resize(voice_c.len() + pat_len, VoiceFrame::default());
            } else {
                let pat_bytes = self
                    .track_c
                    .unique_patterns
                    .get(pat_c as usize)
                    .ok_or_else(|| {
                        format!("Track C step {s} pattern index {pat_c} out of bounds")
                    })?;
                voice_c.extend(decode_voice_stream(pat_bytes, pat_len)?);
            }

            // Track Global
            let pat_glob = self.track_global.sequence_table[s];
            if pat_glob == SENTINEL_EMPTY_PATTERN {
                global_frames.resize(global_frames.len() + pat_len, GlobalFrame::default());
            } else {
                let pat_bytes = self
                    .track_global
                    .unique_patterns
                    .get(pat_glob as usize)
                    .ok_or_else(|| {
                        format!("Track Global step {s} pattern index {pat_glob} out of bounds")
                    })?;
                global_frames.extend(decode_global_stream(pat_bytes, pat_len)?);
            }
        }

        let mut frames = Vec::with_capacity(total_frames);
        for i in 0..total_frames {
            let va = voice_a[i];
            let vb = voice_b[i];
            let vc = voice_c[i];
            let glob = global_frames[i];

            frames.push(YmFrame {
                tone_a: Some(va.period),
                tone_b: Some(vb.period),
                tone_c: Some(vc.period),
                noise_period: Some(glob.noise_period),
                volume_a: Some(if va.envelope_mode { 0x10 } else { va.volume }),
                volume_b: Some(if vb.envelope_mode { 0x10 } else { vb.volume }),
                volume_c: Some(if vc.envelope_mode { 0x10 } else { vc.volume }),
                tone_enable_a: Some((glob.mixer & 0x01) == 0),
                tone_enable_b: Some((glob.mixer & 0x02) == 0),
                tone_enable_c: Some((glob.mixer & 0x04) == 0),
                noise_enable_a: Some((glob.mixer & 0x08) == 0),
                noise_enable_b: Some((glob.mixer & 0x10) == 0),
                noise_enable_c: Some((glob.mixer & 0x20) == 0),
                envelope_period: Some(glob.envelope_period),
                envelope_shape: glob.envelope_shape,
                duration: None,
            });
        }

        let loop_start = if self.header.loop_step == 255 {
            None
        } else {
            Some(self.header.loop_step as usize * pat_len)
        };

        Ok(YmSequence {
            name: name.to_string(),
            timing: TimingConfig {
                master_clock_hz: self.header.master_clock_hz,
                frame_rate: SystemHz::Custom(u32::from(self.header.frame_rate_hz)),
            },
            priority: 0,
            loop_start,
            frames,
        })
    }
}

/// Compiles an uncompressed [`YmSequence`] into a YSG song stream using `pattern_frames` chunking.
///
/// # Errors
/// Returns an error if the song has no frames, sequence length exceeds 255 steps,
/// or unique pattern pools exceed capacity.
pub fn compile_ysg(
    song: &YmSequence,
    pattern_frames: u8,
) -> Result<YsgSongDetails, Box<dyn std::error::Error>> {
    YsgFile::compile(song, pattern_frames)
}

/// Candidate pattern frame sizes to evaluate for optimal compression.
pub const CANDIDATE_PATTERN_FRAMES: &[u8] = &[16, 24, 32, 48, 64, 96, 128];

/// Automatically finds the optimal pattern frame size that yields the smallest
/// total compressed YSG binary size.
///
/// # Errors
/// Returns an error if no valid pattern frame size can compress the song.
pub fn compile_ysg_optimal(
    song: &YmSequence,
) -> Result<YsgSongDetails, Box<dyn std::error::Error>> {
    YsgFile::compile_optimal(song)
}

/// Decodes a compiled `.ysg` binary stream into a high-level [`YmSequence`].
///
/// # Errors
/// Returns an error if header validation fails, track descriptors are corrupted,
/// or opcode stream payload bytes are malformed.
pub fn decompile_ysg(name: &str, bytes: &[u8]) -> Result<YmSequence, Box<dyn std::error::Error>> {
    YsgFile::decompile(name, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::YmSongRenderer;
    use crate::traits::SongInput;

    fn assert_round_trip_frames_match(expected: &[YmFrame], actual: &[YmFrame]) {
        for (idx, (orig, dec)) in expected.iter().zip(actual.iter()).enumerate() {
            let check_channel = |orig_vol: Option<u8>,
                                 dec_vol: Option<u8>,
                                 orig_tone: Option<u16>,
                                 dec_tone: Option<u16>,
                                 name: &str| {
                let v_orig = orig_vol.unwrap_or(0);
                let v_dec = dec_vol.unwrap_or(0);
                assert_eq!(v_orig, v_dec, "Volume {name} mismatch at frame {idx}");
                if v_orig > 0 {
                    assert_eq!(
                        orig_tone.unwrap_or(0),
                        dec_tone.unwrap_or(0),
                        "Tone {name} mismatch at frame {idx}"
                    );
                }
            };

            check_channel(orig.volume_a, dec.volume_a, orig.tone_a, dec.tone_a, "A");
            check_channel(orig.volume_b, dec.volume_b, orig.tone_b, dec.tone_b, "B");
            check_channel(orig.volume_c, dec.volume_c, orig.tone_c, dec.tone_c, "C");

            let noise_active = orig.noise_enable_a.unwrap_or(false)
                || orig.noise_enable_b.unwrap_or(false)
                || orig.noise_enable_c.unwrap_or(false);
            if noise_active {
                assert_eq!(
                    orig.noise_period.unwrap_or(0),
                    dec.noise_period.unwrap_or(0),
                    "Noise period mismatch at frame {idx}"
                );
            }
            assert_eq!(
                orig.tone_enable_a, dec.tone_enable_a,
                "Tone enable A mismatch at frame {idx}"
            );
            assert_eq!(
                orig.tone_enable_b, dec.tone_enable_b,
                "Tone enable B mismatch at frame {idx}"
            );
            assert_eq!(
                orig.tone_enable_c, dec.tone_enable_c,
                "Tone enable C mismatch at frame {idx}"
            );
            assert_eq!(
                orig.noise_enable_a, dec.noise_enable_a,
                "Noise enable A mismatch at frame {idx}"
            );
            assert_eq!(
                orig.noise_enable_b, dec.noise_enable_b,
                "Noise enable B mismatch at frame {idx}"
            );
            assert_eq!(
                orig.noise_enable_c, dec.noise_enable_c,
                "Noise enable C mismatch at frame {idx}"
            );
            assert_eq!(
                orig.envelope_period.unwrap_or(0),
                dec.envelope_period.unwrap_or(0),
                "Env period mismatch at frame {idx}"
            );
            assert_eq!(
                orig.envelope_shape, dec.envelope_shape,
                "Env shape mismatch at frame {idx}"
            );
        }
    }

    #[test]
    fn test_ysg_header_round_trip() {
        let header = YsgHeader {
            pattern_frames: 48,
            seq_len: 128,
            loop_step: 16,
            frame_rate_hz: 50,
            master_clock_hz: 1_789_773,
            offset_track_a: 0x0040,
            offset_track_b: 0x0180,
            offset_track_c: 0x02C0,
            offset_track_global: 0x0400,
        };

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), YSG_HEADER_SIZE);
        assert_eq!(&bytes[0..2], b"YS");
        assert_eq!(bytes[2], 0x01);

        let parsed = YsgHeader::from_bytes(&bytes).expect("Header parse should succeed");
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_voice_stream_round_trip_with_wait_runs() {
        let mut frames = Vec::new();

        // Frame 0: Note on (Period 0x01F4 = 500, Volume 15)
        frames.push(VoiceFrame {
            period: 500,
            volume: 15,
            envelope_mode: false,
        });

        // Frames 1..32: Sustained note (31 idle frames)
        for _ in 0..31 {
            frames.push(VoiceFrame {
                period: 500,
                volume: 15,
                envelope_mode: false,
            });
        }

        // Frame 32: Period LSB pitch vibrato to 502
        frames.push(VoiceFrame {
            period: 502,
            volume: 15,
            envelope_mode: false,
        });

        // Frame 33: Envelope mode enabled
        frames.push(VoiceFrame {
            period: 502,
            volume: 0,
            envelope_mode: true,
        });

        let encoded = encode_voice_pattern(&frames);
        assert!(
            encoded.len() < 15,
            "Expected high compression on sustained note, got {} bytes",
            encoded.len()
        );

        let decoded = decode_voice_stream(&encoded, frames.len()).expect("Decode should succeed");
        assert_eq!(decoded.len(), frames.len());

        for (idx, (orig, dec)) in frames.iter().zip(decoded.iter()).enumerate() {
            assert_eq!(orig.period, dec.period, "Period mismatch at frame {idx}");
            if orig.envelope_mode {
                assert!(dec.envelope_mode, "Envelope mode mismatch at frame {idx}");
            } else {
                assert_eq!(orig.volume, dec.volume, "Volume mismatch at frame {idx}");
            }
        }
    }

    #[test]
    fn test_global_stream_round_trip_with_r13_guard() {
        let mut frames = Vec::new();

        // 10 idle frames
        for _ in 0..10 {
            frames.push(GlobalFrame::default());
        }

        // Frame 10: Drum hit (Noise 16, Mixer with Noise A enabled)
        frames.push(GlobalFrame {
            noise_period: 16,
            mixer: 0x37, // Bit 3 cleared (Noise A enabled)
            envelope_period: 1000,
            envelope_shape: Some(0x09), // Retrigger
        });

        // Frames 11..25: Sustained without retrigger
        for _ in 0..14 {
            frames.push(GlobalFrame {
                noise_period: 16,
                mixer: 0x37,
                envelope_period: 1000,
                envelope_shape: None,
            });
        }

        let encoded = encode_global_pattern(&frames);
        assert!(
            encoded.len() < 15,
            "Expected high compression on global stream, got {} bytes",
            encoded.len()
        );

        let decoded = decode_global_stream(&encoded, frames.len()).expect("Decode should succeed");
        assert_eq!(decoded.len(), frames.len());

        // Verify R13 was retriggered on frame 10 only
        assert_eq!(decoded[10].envelope_shape, Some(0x09));
        assert_eq!(decoded[11].envelope_shape, None);
        assert_eq!(decoded[10].noise_period, 16);
        assert_eq!(decoded[11].noise_period, 16);
    }

    #[test]
    fn test_ysg_full_round_trip_compilation() {
        let mut frames = Vec::new();
        for i in 0..96 {
            frames.push(YmFrame {
                tone_a: Some(200 + (i % 12)),
                volume_a: Some(15),
                tone_enable_a: Some(true),
                tone_b: Some(400),
                volume_b: Some(12),
                tone_enable_b: Some(true),
                ..Default::default()
            });
        }

        let song = YmSequence {
            name: "test_ysg".to_string(),
            timing: TimingConfig {
                master_clock_hz: 1_789_773,
                frame_rate: SystemHz::Hz50,
            },
            priority: 0,
            loop_start: Some(48),
            frames,
        };

        let details = compile_ysg(&song, 48).expect("Compilation should succeed");
        assert_eq!(details.seq_len, 2);
        assert_eq!(
            details.unique_patterns_b, 1,
            "Channel B should be deduplicated to 1 pattern"
        );

        let decompiled =
            decompile_ysg("test_ysg", &details.bytes).expect("Decompile should succeed");
        assert_eq!(decompiled.frames.len(), 96);
        assert_eq!(decompiled.loop_start, Some(48));
        assert_eq!(decompiled.timing.master_clock_hz, 1_789_773);
        assert_eq!(decompiled.timing.frame_rate.hz_value(), 50);

        for (idx, (orig, dec)) in song.frames.iter().zip(decompiled.frames.iter()).enumerate() {
            assert_eq!(orig.tone_a, dec.tone_a, "Tone A mismatch at frame {idx}");
            assert_eq!(
                orig.volume_a, dec.volume_a,
                "Volume A mismatch at frame {idx}"
            );
            assert_eq!(orig.tone_b, dec.tone_b, "Tone B mismatch at frame {idx}");
            assert_eq!(
                orig.volume_b, dec.volume_b,
                "Volume B mismatch at frame {idx}"
            );
        }
    }

    #[test]
    fn test_cpc_dream_ysg_compression_round_trip() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
        let ym_path = std::path::Path::new(&manifest_dir).join("../test.ym");
        if !ym_path.exists() {
            return;
        }

        let bytes = std::fs::read(&ym_path).expect("Failed to read test.ym");
        let ym_file = crate::ym_file::YmFile::from_bytes("cpc_dream", &bytes, None)
            .expect("Failed to parse test.ym");
        let song = ym_file
            .to_sequence(None)
            .expect("Failed to convert to sequence");

        assert_eq!(song.frames.len(), 6144);

        println!("\n=== YSG Pattern Size Exploration (CPC-Dream) ===");
        println!("Frames | Steps | Uniq A/B/C/Glob | Track Bytes (A/B/C/Glob) | Total Bytes | Reduction vs Raw");
        println!("-----------------------------------------------------------------------------------------");

        let mut best_size = usize::MAX;
        let mut best_frames = 0;

        for &pf in &[8u8, 12, 16, 24, 32, 48, 64, 96, 128] {
            if let Ok(det) = compile_ysg(&song, pf) {
                let sz = det.bytes.len();
                let pct = (1.0 - (sz as f64 / 98424.0)) * 100.0;
                println!(
                    "{:6} | {:5} | {:2}/{:2}/{:2}/{:2}     | {:4}/{:4}/{:4}/{:4}     | {:11} | {:5.1}%",
                    pf, det.seq_len,
                    det.unique_patterns_a, det.unique_patterns_b, det.unique_patterns_c, det.unique_patterns_global,
                    det.track_a_bytes, det.track_b_bytes, det.track_c_bytes, det.track_global_bytes,
                    sz,
                    pct
                );
                if sz < best_size {
                    best_size = sz;
                    best_frames = pf;
                }
            }
        }
        println!("Best pattern_frames = {best_frames} with {best_size} bytes");

        let (ta, _tb, _tc, _tg) = decompose_frames(&song.frames);
        let pat0_a = encode_voice_pattern(&ta[0..48]);
        println!(
            "Track A Pattern 0 encoded bytes (len={}): {:?}",
            pat0_a.len(),
            pat0_a
        );
        let pat1_a = encode_voice_pattern(&ta[48..96]);
        println!(
            "Track A Pattern 1 encoded bytes (len={}): {:?}",
            pat1_a.len(),
            pat1_a
        );
        let pat2_a = encode_voice_pattern(&ta[96..144]);
        println!(
            "Track A Pattern 2 encoded bytes (len={}): {:?}",
            pat2_a.len(),
            pat2_a
        );

        let best_details = compile_ysg(&song, best_frames).expect("Compile best should succeed");
        // Verify round-trip decompression
        let decompiled =
            decompile_ysg("cpc_dream", &best_details.bytes).expect("Decompile should succeed");
        assert_eq!(decompiled.frames.len(), 6144);

        println!("Frame 4560 orig: {:?}", song.frames[4560]);
        println!("Frame 4560 dec : {:?}", decompiled.frames[4560]);

        assert_round_trip_frames_match(&song.frames, &decompiled.frames);
    }

    #[test]
    fn test_scout_ysg_round_trip() {
        let ym_path = std::path::Path::new("../tests/fixtures/song/Scout.ym");
        if !ym_path.exists() {
            return;
        }
        let bytes = std::fs::read(ym_path).unwrap();
        let ym_file = crate::ym_file::YmFile::from_bytes("scout", &bytes, None).unwrap();
        let song = ym_file.to_sequence(None).unwrap();

        let compiled = compile_ysg_optimal(&song).expect("Compile scout should succeed");
        println!(
            "Scout compiled pattern_frames: {}, seq_len: {}, total_bytes: {}",
            compiled.pattern_frames,
            compiled.seq_len,
            compiled.bytes.len()
        );

        let decompiled =
            decompile_ysg("scout", &compiled.bytes).expect("Decompile scout should succeed");
        assert!(decompiled.frames.len() >= song.frames.len());

        assert_round_trip_frames_match(&song.frames, &decompiled.frames);

        // Compare first 5 seconds of synthesized audio
        let mut r_orig = YmSongRenderer::new(&song, 44100);
        let mut r_dec = YmSongRenderer::new(&decompiled, 44100);
        let sample_count = 44100 * 5;
        let mut buf_orig = vec![0.0f32; sample_count];
        let mut buf_dec = vec![0.0f32; sample_count];
        r_orig.render_samples(&mut buf_orig, 1);
        r_dec.render_samples(&mut buf_dec, 1);

        let mut diff_count = 0;
        for (i, (s1, s2)) in buf_orig.iter().zip(buf_dec.iter()).enumerate() {
            if (s1 - s2).abs() > 0.001 {
                if diff_count < 5 {
                    println!("Audio diff at sample {i}: orig={s1}, dec={s2}");
                }
                diff_count += 1;
            }
        }
        println!("Total audio sample diffs in first 5s: {diff_count} / {sample_count}");
        assert_eq!(
            diff_count, 0,
            "Audio output differs between original and decompiled Scout!"
        );
    }

    /// Effective R13 shape at each frame: the last retrigger value seen so far.
    fn effective_shapes(frames: &[YmFrame]) -> Vec<Option<u8>> {
        let mut shape = None;
        frames
            .iter()
            .map(|f| {
                if f.envelope_shape.is_some() {
                    shape = f.envelope_shape;
                }
                shape
            })
            .collect()
    }

    #[test]
    fn test_reused_global_pattern_inherits_envelope_shape_like_source() {
        // Steps 1-3 and 5 have identical global content and dedupe to one pattern, while
        // step 4 retriggers a different shape. Playback follows the sequence table in
        // source order, so step 5 must inherit 0x08 from step 4 exactly as the source does.
        const PF: usize = 16;
        let mut frames = Vec::new();
        for step in 0..6 {
            for i in 0..PF {
                let shape = match (step, i) {
                    (0, 0) => Some(0x0E),
                    (4, 0) => Some(0x08),
                    _ => None,
                };
                frames.push(YmFrame {
                    tone_a: Some(300),
                    volume_a: Some(0x10),
                    tone_enable_a: Some(true),
                    envelope_period: Some(100),
                    envelope_shape: shape,
                    ..Default::default()
                });
            }
        }
        let song = YmSequence {
            name: "env_reuse".to_string(),
            timing: TimingConfig {
                master_clock_hz: 1_789_773,
                frame_rate: SystemHz::Hz50,
            },
            priority: 0,
            loop_start: None,
            frames,
        };

        let (file, details) = YsgFile::from_sequence(&song, PF as u8).unwrap();
        let table = &file.track_global.sequence_table;
        assert_eq!(
            table[1], table[5],
            "Step 5 should reuse step 1's global pattern"
        );
        assert_ne!(table[4], table[5]);

        let decompiled = decompile_ysg("env_reuse", &details.bytes).unwrap();
        assert_eq!(
            effective_shapes(&song.frames),
            effective_shapes(&decompiled.frames)
        );
        assert_eq!(effective_shapes(&decompiled.frames)[5 * PF], Some(0x08));
    }

    #[test]
    fn test_track_byte_len_matches_serialized_len() {
        let mut track = TrackDescriptor::default();
        track.push_pattern(vec![1, 2, 3]).unwrap();
        track.push_empty_pattern();
        track.push_pattern(vec![4, 5]).unwrap();
        track.push_pattern(vec![1, 2, 3]).unwrap();
        assert_eq!(track.byte_len(), track.to_bytes().len());
    }

    #[test]
    fn test_oversized_song_is_rejected_not_wrapped() {
        // Pseudo-random pitches and volumes defeat deduplication, pushing the
        // compiled file past the 16-bit offset limit.
        let mut seed: u32 = 0x1234_5678;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 8
        };
        let frames: Vec<YmFrame> = (0..12_000)
            .map(|_| YmFrame {
                tone_a: Some((next() & 0x0FFF) as u16),
                volume_a: Some((next() % 15 + 1) as u8),
                tone_b: Some((next() & 0x0FFF) as u16),
                volume_b: Some((next() % 15 + 1) as u8),
                tone_c: Some((next() & 0x0FFF) as u16),
                volume_c: Some((next() % 15 + 1) as u8),
                ..Default::default()
            })
            .collect();
        let song = YmSequence {
            name: "huge".to_string(),
            timing: TimingConfig {
                master_clock_hz: 1_789_773,
                frame_rate: SystemHz::Hz50,
            },
            priority: 0,
            loop_start: None,
            frames,
        };

        let err = YsgFile::from_sequence(&song, 64).expect_err("Song should exceed 64 KB");
        assert!(err.to_string().contains("exceeds the YSG"), "{err}");
    }
}
