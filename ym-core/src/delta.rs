use crate::sequence::{SfxSequence, YmSequence};

/// Magic prefix for upkr-compressed YSG files.
pub const UPKR_MAGIC: [u8; 4] = [b'Y', b'Z', b'L', b'Z'];

/// Controls how much compression is applied when compiling a song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// Full: delta encoding + pattern deduplication (default).
    Full,
    /// Delta only: delta-encode each frame but use one single pattern (no dedup, no mid-song boundary resets).
    DeltaOnly,
    /// None: write all 14 registers every frame (no delta, no dedup). Raw register stream.
    None,
    /// Lz: full delta + dedup, then upkr-compressed. Smallest output, requires decompression on playback.
    Lz,
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
    pub fn new() -> Self {
        Self
    }

    /// Compiles a sound effect sequence into a 5-byte fixed-width frame representation.
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

    /// Compiles a music song sequence into a YSG binary payload.
    ///
    /// `level` controls how much compression is applied — use [`CompressionLevel::Full`]
    /// for production, or a reduced level to isolate audio issues.
    pub fn compile_song(
        &self,
        sequence: &YmSequence,
        level: CompressionLevel,
    ) -> Result<YmSongDetails, Box<dyn std::error::Error>> {
        if level == CompressionLevel::DeltaOnly || level == CompressionLevel::None {
            return self.compile_song_no_dedup(sequence, level);
        }

        if level == CompressionLevel::Lz {
            let base = self.compile_song_full(sequence)?;
            let compressed = Self::upkr_pack(&base.bytes);
            return Ok(YmSongDetails { bytes: compressed, pattern_size: base.pattern_size });
        }

        self.compile_song_full(sequence)
    }

    /// Full delta + dedup compression, searching for the best pattern size.
    fn compile_song_full(
        &self,
        sequence: &YmSequence,
    ) -> Result<YmSongDetails, Box<dyn std::error::Error>> {
        let mut best_data: Option<Vec<u8>> = None;
        let mut best_size = 64;

        let candidate_sizes = Self::candidate_pattern_sizes(sequence.frames.len());

        for size in candidate_sizes {
            if let Some(data) = self.compile_song_with_size(sequence, size) {
                if best_data.as_ref().map_or(true, |b| data.len() < b.len()) {
                    best_data = Some(data);
                    best_size = size;
                }
            }
        }

        let bytes = best_data.ok_or_else(|| {
            format!(
                "Song too long to compile: {} frames exceed the 255-entry sequence table limit for all pattern sizes",
                sequence.frames.len()
            )
        })?;

        Ok(YmSongDetails { bytes, pattern_size: best_size })
    }

    /// Wraps YSG bytes with upkr compression and a YZLZ magic header.
    fn upkr_pack(ysg_bytes: &[u8]) -> Vec<u8> {
        let config = upkr::Config::default();
        let compressed = upkr::pack(ysg_bytes, 5, &config, None);
        let uncompressed_len = ysg_bytes.len() as u32;
        let mut out = Vec::with_capacity(8 + compressed.len());
        out.extend_from_slice(&UPKR_MAGIC);
        out.extend_from_slice(&uncompressed_len.to_le_bytes());
        out.extend_from_slice(&compressed);
        out
    }

    /// Decompresses a upkr-wrapped YSG payload back to raw YSG bytes.
    pub fn upkr_unpack(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if bytes.len() < 8 || bytes[0..4] != UPKR_MAGIC {
            return Err("Not a YZLZ compressed file".into());
        }
        let uncompressed_len = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
        let config = upkr::Config::default();
        upkr::unpack(&bytes[8..], &config, uncompressed_len)
            .map_err(|e| format!("upkr decompress failed: {:?}", e).into())
    }

    /// Compiles without deduplication: every block is a unique pattern.
    /// Used for `DeltaOnly` and `None` compression levels so we can isolate which
    /// stage causes audio problems.
    fn compile_song_no_dedup(
        &self,
        sequence: &YmSequence,
        level: CompressionLevel,
    ) -> Result<YmSongDetails, Box<dyn std::error::Error>> {
        // Pick the largest pattern size that keeps the sequence table within 255 entries.
        let total_frames = sequence.frames.len();
        let mut best: Option<(Vec<u8>, usize)> = None;

        for size in (8..=255usize).rev() {
            let num_blocks = total_frames.div_ceil(size);
            if num_blocks > 255 {
                continue;
            }

            let raw_blocks = Self::chunk_frames_into_patterns(&sequence.frames, size);
            let unique_patterns: Vec<Vec<u8>> = raw_blocks
                .iter()
                .map(|block| Self::serialize_ym_block_with_level(block, level))
                .collect();

            // Sequence table = 0, 1, 2, … (no dedup — every block is unique)
            let sequence_table: Vec<u8> = (0..unique_patterns.len() as u8).collect();
            let loop_pattern =
                Self::calculate_loop_pattern(sequence.loop_start, size, sequence_table.len());
            let header = Self::build_ysg_header(
                size,
                unique_patterns.len(),
                sequence_table.len(),
                loop_pattern,
                &sequence.timing,
            );
            let offset_table = Self::build_pattern_offset_table(&unique_patterns);
            let bytes =
                Self::assemble_ysg_payload(header, &sequence_table, &offset_table, unique_patterns);

            if best.as_ref().map_or(true, |(b, _)| bytes.len() < b.len()) {
                best = Some((bytes, size));
            }
            break; // largest valid size is always best for no-dedup (fewer boundaries)
        }

        let (bytes, pattern_size) = best.ok_or("Song too long to compile without dedup")?;
        Ok(YmSongDetails { bytes, pattern_size })
    }

    /// Attempts to compile `sequence` using a fixed `pattern_size`; returns `None` if the
    /// resulting sequence table exceeds 255 entries.
    fn compile_song_with_size(
        &self,
        sequence: &YmSequence,
        pattern_size: usize,
    ) -> Option<Vec<u8>> {
        if sequence.frames.is_empty() {
            return Some(Vec::new());
        }

        // Chunk & serialize frames into pattern bytes
        let serialized_blocks = Self::serialize_pattern_blocks(&sequence.frames, pattern_size)?;

        // Deduplicate pattern blocks into unique table & sequence array
        let (unique_patterns, sequence_table) = Self::deduplicate_patterns(serialized_blocks)?;

        // Resolve loop index, 12-byte header, and 32-bit offset table
        let loop_pattern =
            Self::calculate_loop_pattern(sequence.loop_start, pattern_size, sequence_table.len());
        let header = Self::build_ysg_header(
            pattern_size,
            unique_patterns.len(),
            sequence_table.len(),
            loop_pattern,
            &sequence.timing,
        );
        let offset_table = Self::build_pattern_offset_table(&unique_patterns);

        // Assemble binary output payload
        Some(Self::assemble_ysg_payload(
            header,
            &sequence_table,
            &offset_table,
            unique_patterns,
        ))
    }

    /// Returns the sorted list of pattern sizes to try when searching for the best compression.
    fn candidate_pattern_sizes(total_frames: usize) -> Vec<usize> {
        let mut sizes: Vec<usize> = (8..=255).step_by(8).collect();
        for sz in [12, 24, 48, 96, 192, 255] {
            if !sizes.contains(&sz) {
                sizes.push(sz);
            }
        }
        if total_frames > 0 {
            for div in [1, 2, 3, 4, 5, 6, 8, 10, 12, 16] {
                let sz = total_frames / div;
                if (8..=255).contains(&sz) && !sizes.contains(&sz) {
                    sizes.push(sz);
                }
            }
        }
        sizes.sort_unstable();
        sizes
    }

    /// Resolves the loop pattern index for the YSG header.
    fn calculate_loop_pattern(
        loop_start: Option<usize>,
        pattern_size: usize,
        seq_len: usize,
    ) -> u8 {
        match loop_start {
            Some(frame) => {
                let pat_idx = frame / pattern_size;
                if pat_idx < seq_len {
                    pat_idx as u8
                } else {
                    255 // Disable looping if specified loop frame is out of bounds
                }
            }
            None => 255, // 255 signifies no loop
        }
    }

    /// Builds the fixed 12-byte YSG header.
    fn build_ysg_header(
        pattern_size: usize,
        num_unique: usize,
        seq_len: usize,
        loop_pattern: u8,
        timing: &crate::timing::TimingConfig,
    ) -> Vec<u8> {
        let mut header = vec![
            pattern_size as u8,
            num_unique as u8,
            seq_len as u8,
            loop_pattern,
        ];
        header.extend(timing.frame_rate.hz_value().to_le_bytes());
        header.extend(timing.master_clock_hz.to_le_bytes());
        header
    }

    /// Helper: Constructs the 32-bit little-endian pattern offset table.
    fn build_pattern_offset_table(patterns: &[Vec<u8>]) -> Vec<u8> {
        let mut current_offset = 0usize;
        let mut offset_table = Vec::with_capacity(patterns.len() * 4);
        for pat in patterns {
            offset_table.extend((current_offset as u32).to_le_bytes());
            current_offset += pat.len();
        }
        offset_table
    }

    /// Assembles header, sequence table, offset table, and pattern data into the final binary payload.
    fn assemble_ysg_payload(
        header: Vec<u8>,
        sequence_table: &[u8],
        offset_table: &[u8],
        unique_patterns: Vec<Vec<u8>>,
    ) -> Vec<u8> {
        let mut output = header;
        output.extend(sequence_table);
        output.extend(offset_table);
        for pat in unique_patterns {
            output.extend(pat);
        }
        output
    }

    /// Deduplicates identical pattern blocks into a unique pattern table and sequence index array.
    fn deduplicate_patterns(serialized_blocks: Vec<Vec<u8>>) -> Option<(Vec<Vec<u8>>, Vec<u8>)> {
        let mut unique_patterns: Vec<Vec<u8>> = Vec::new();
        let mut sequence_table: Vec<u8> = Vec::new();

        for block in serialized_blocks {
            let position = unique_patterns.iter().position(|x| x == &block);
            match position {
                Some(p_idx) => {
                    sequence_table.push(p_idx as u8);
                }
                None => {
                    let new_idx = unique_patterns.len();
                    if new_idx >= 256 {
                        return None;
                    }
                    unique_patterns.push(block);
                    sequence_table.push(new_idx as u8);
                }
            }
        }

        Some((unique_patterns, sequence_table))
    }

    /// Chunks frames into pattern blocks and serializes each block to delta-mask binary bytes.
    /// Returns `None` if the total number of blocks exceeds the 255 sequence table limit.
    fn serialize_pattern_blocks(
        frames: &[crate::sequence::YmFrame],
        pattern_size: usize,
    ) -> Option<Vec<Vec<u8>>> {
        let raw_blocks = Self::chunk_frames_into_patterns(frames, pattern_size);
        if raw_blocks.len() > 255 {
            return None;
        }

        Some(
            raw_blocks
                .iter()
                .map(|block| Self::serialize_ym_block_with_level(block, CompressionLevel::Full))
                .collect(),
        )
    }

    /// Chunks a sequence of frames into blocks of `pattern_size`, padding the final block with default frames.
    fn chunk_frames_into_patterns(
        frames: &[crate::sequence::YmFrame],
        pattern_size: usize,
    ) -> Vec<Vec<crate::sequence::YmFrame>> {
        frames
            .chunks(pattern_size)
            .map(|chunk| {
                let mut block = chunk.to_vec();
                block.resize(pattern_size, crate::sequence::YmFrame::default());
                block
            })
            .collect()
    }

    /// Serializes a block of frames to binary using the given compression level.
    fn serialize_ym_block_with_level(
        frames: &[crate::sequence::YmFrame],
        level: CompressionLevel,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        let mut registers = [0u8; 14];
        registers[7] = 0x3F; // Match playback's initial mixer state: all channels muted

        for (idx, frame) in frames.iter().enumerate() {
            let new_registers = Self::extract_frame_registers(frame, &registers);

            let (mask, payload) = match level {
                CompressionLevel::None => {
                    // Write all R0-R12 every frame; R13 only when it carries a real write.
                    let mut m = 0x1FFFu16;
                    let mut p = new_registers[..13].to_vec();
                    if new_registers[13] != 0xFF {
                        m |= 1 << 13;
                        p.push(new_registers[13]);
                    }
                    (m, p)
                }
                CompressionLevel::Full | CompressionLevel::DeltaOnly | CompressionLevel::Lz => {
                    Self::encode_frame_delta(&new_registers, &registers, idx == 0)
                }
            };

            registers = new_registers;

            data.push((mask & 0xFF) as u8);
            data.push(((mask >> 8) & 0xFF) as u8);
            data.extend(payload);
        }

        data
    }

    /// Converts a high-level [`crate::sequence::YmFrame`] into raw YM2149 14-byte register array.
    /// `None` fields inherit their value from `prev_registers`, matching `apply_to_chip` semantics.
    fn extract_frame_registers(
        frame: &crate::sequence::YmFrame,
        prev_registers: &[u8; 14],
    ) -> [u8; 14] {
        let mut regs = *prev_registers;

        // Tone A, B, C
        if let Some(tone_a) = frame.tone_a {
            regs[0] = (tone_a & 0xFF) as u8;
            regs[1] = ((tone_a >> 8) & 0x0F) as u8;
        }
        if let Some(tone_b) = frame.tone_b {
            regs[2] = (tone_b & 0xFF) as u8;
            regs[3] = ((tone_b >> 8) & 0x0F) as u8;
        }
        if let Some(tone_c) = frame.tone_c {
            regs[4] = (tone_c & 0xFF) as u8;
            regs[5] = ((tone_c >> 8) & 0x0F) as u8;
        }

        // Noise Period
        if let Some(noise) = frame.noise_period {
            regs[6] = noise & 0x1F;
        }

        // Mixer R7 (0 is ENABLED, 1 is DISABLED) — only modify bits that are specified
        let mut mixer = prev_registers[7];
        if let Some(en) = frame.tone_enable_a {
            if en { mixer &= !0x01; } else { mixer |= 0x01; }
        }
        if let Some(en) = frame.tone_enable_b {
            if en { mixer &= !0x02; } else { mixer |= 0x02; }
        }
        if let Some(en) = frame.tone_enable_c {
            if en { mixer &= !0x04; } else { mixer |= 0x04; }
        }
        if let Some(en) = frame.noise_enable_a {
            if en { mixer &= !0x08; } else { mixer |= 0x08; }
        }
        if let Some(en) = frame.noise_enable_b {
            if en { mixer &= !0x10; } else { mixer |= 0x10; }
        }
        if let Some(en) = frame.noise_enable_c {
            if en { mixer &= !0x20; } else { mixer |= 0x20; }
        }
        regs[7] = mixer;

        // Volumes R8, R9, R10
        if let Some(vol) = frame.volume_a { regs[8] = vol & 0x1F; }
        if let Some(vol) = frame.volume_b { regs[9] = vol & 0x1F; }
        if let Some(vol) = frame.volume_c { regs[10] = vol & 0x1F; }

        // Envelopes R11, R12
        if let Some(env_period) = frame.envelope_period {
            regs[11] = (env_period & 0xFF) as u8;
            regs[12] = ((env_period >> 8) & 0xFF) as u8;
        }

        // R13 (envelope shape) uses 0xFF as "not written this frame" sentinel.
        // We never inherit from prev for R13 — the encoder must know whether a
        // write actually occurred, not just what the chip state happens to hold.
        regs[13] = frame.envelope_shape.map(|v| v & 0x0F).unwrap_or(0xFF);

        regs
    }

    /// Computes 16-bit delta bitmask and gathers changed payload bytes.
    ///
    /// R13 is included only when the value changes — same-value writes retrigger the
    /// envelope phase and cause audible clicks without musical intent.
    fn encode_frame_delta(
        new_registers: &[u8; 14],
        prev_registers: &[u8; 14],
        is_first_frame: bool,
    ) -> (u16, Vec<u8>) {
        if is_first_frame {
            // R0-R12 always written on the first frame of a pattern for O(1) pattern independence.
            // R13 only written when it carries a real write (not 0xFF sentinel).
            let mut mask = 0x1FFFu16;
            let mut payload = new_registers[..13].to_vec();
            if new_registers[13] != 0xFF {
                mask |= 1 << 13;
                payload.push(new_registers[13]);
            }
            (mask, payload)
        } else {
            let mut mask = 0u16;
            let mut payload = Vec::new();
            for r in 0..13 {
                if new_registers[r] != prev_registers[r] {
                    mask |= 1 << r;
                    payload.push(new_registers[r]);
                }
            }
            // R13: include only when the value changes (same-value writes retrigger envelope phase).
            if new_registers[13] != 0xFF && new_registers[13] != prev_registers[13] {
                mask |= 1 << 13;
                payload.push(new_registers[13]);
            }
            (mask, payload)
        }
    }
}
