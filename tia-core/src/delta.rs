#![allow(clippy::similar_names)]

use crate::sequence::{TiaSequence, TiaSfxSequence};
use std::collections::HashMap;

/// Bit 7 of the frame mask byte — signals an RLE idle-run token.
pub const RLE_FLAG: u8 = 0x80;

/// Controls how much compression is applied when compiling a TIA song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// Full: delta encoding + pattern deduplication + RLE.
    Full,
    /// Delta only: delta-encode each frame within a single pattern.
    DeltaOnly,
    /// None: write all 6 registers every frame (raw register stream).
    None,
}

/// Feature flags for [`DeltaCompiler::compile_song`].
#[derive(Debug, Clone)]
pub struct CompilerOptions {
    /// Enable pattern deduplication.
    pub dedup: bool,
    /// Enable idle-frame RLE.
    pub rle: bool,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            dedup: true,
            rle: true,
        }
    }
}

/// Platform-agnostic delta-mask compiler for TIA sound and song streams.
#[derive(Debug, Default)]
pub struct DeltaCompiler;

/// Result of [`DeltaCompiler::compile_song`].
#[derive(Debug, Clone)]
pub struct TiaSongDetails {
    pub bytes: Vec<u8>,
    pub pattern_size: usize,
    pub raw_bytes: usize,
    pub compiled_bytes: usize,
}

impl DeltaCompiler {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Compiles a single-channel TIA sound effect sequence into a fixed-width 3-byte payload (`[AUDF, AUDC, AUDV]`).
    #[must_use]
    pub fn compile_sfx(&self, sequence: &TiaSfxSequence) -> Vec<u8> {
        let mut compiled = Vec::with_capacity(sequence.frames.len() * 3);
        let mut last_audf = 0u8;
        let mut last_audc = 0u8;
        let mut last_audv = 0u8;

        for frame in &sequence.frames {
            if let Some(f) = frame.audf {
                last_audf = f & 0x1F;
            }
            if let Some(c) = frame.audc {
                last_audc = c & 0x0F;
            }
            if let Some(v) = frame.audv {
                last_audv = v & 0x0F;
            }

            compiled.push(last_audf);
            compiled.push(last_audc);
            compiled.push(last_audv);
        }

        compiled
    }

    /// Compiles a dual-channel TIA song sequence into an optimized TSG binary stream.
    ///
    /// # Errors
    /// Returns an error if sequence length exceeds allowable limits.
    pub fn compile_song(
        &self,
        sequence: &TiaSequence,
        level: CompressionLevel,
        options: &CompilerOptions,
    ) -> Result<TiaSongDetails, Box<dyn std::error::Error>> {
        let raw_bytes = sequence.frames.len() * 6;
        if sequence.frames.is_empty() {
            let header = vec![16, 0, 0, 255, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            return Ok(TiaSongDetails {
                bytes: header,
                pattern_size: 16,
                raw_bytes: 0,
                compiled_bytes: 14,
            });
        }

        let candidate_sizes: &[usize] = match level {
            CompressionLevel::None | CompressionLevel::DeltaOnly => &[sequence.frames.len().max(1)],
            CompressionLevel::Full => {
                if options.dedup {
                    &[16, 32, 64]
                } else {
                    &[sequence.frames.len().max(1)]
                }
            }
        };

        let mut best_result: Option<(Vec<u8>, usize)> = None;

        for &pattern_size in candidate_sizes {
            if let Ok(bytes) =
                Self::try_compile_with_pattern_size(sequence, pattern_size, level, options)
            {
                if best_result
                    .as_ref()
                    .is_none_or(|(best_b, _)| bytes.len() < best_b.len())
                {
                    best_result = Some((bytes, pattern_size));
                }
            }
        }

        let (bytes, pattern_size) =
            best_result.ok_or("Failed to compile TIA song with candidate pattern sizes")?;
        let compiled_bytes = bytes.len();

        Ok(TiaSongDetails {
            bytes,
            pattern_size,
            raw_bytes,
            compiled_bytes,
        })
    }

    fn try_compile_with_pattern_size(
        sequence: &TiaSequence,
        pattern_size: usize,
        level: CompressionLevel,
        options: &CompilerOptions,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let total_frames = sequence.frames.len();
        let num_patterns = total_frames.div_ceil(pattern_size);
        if num_patterns > 255 {
            return Err("Song too long for sequence table (max 255 patterns)".into());
        }

        let mut pattern_blobs: Vec<Vec<u8>> = Vec::new();
        let mut sequence_table: Vec<u8> = Vec::new();
        let mut pattern_map: HashMap<Vec<u8>, u8> = HashMap::new();

        for pat_idx in 0..num_patterns {
            let start = pat_idx * pattern_size;
            let end = (start + pattern_size).min(total_frames);
            let chunk = &sequence.frames[start..end];

            let blob = Self::encode_pattern_chunk(chunk, pattern_size, level, options);

            let pat_id = if options.dedup && level == CompressionLevel::Full {
                if let Some(&existing_id) = pattern_map.get(&blob) {
                    existing_id
                } else {
                    let new_id = pattern_blobs.len() as u8;
                    if pattern_blobs.len() >= 255 {
                        return Err("Too many unique patterns (max 255)".into());
                    }
                    pattern_blobs.push(blob.clone());
                    pattern_map.insert(blob, new_id);
                    new_id
                }
            } else {
                let new_id = pattern_blobs.len() as u8;
                pattern_blobs.push(blob);
                new_id
            };

            sequence_table.push(pat_id);
        }

        let loop_pattern = if let Some(loop_frame) = sequence.loop_start {
            (loop_frame / pattern_size).min(254) as u8
        } else {
            255
        };

        let last_pat_frames = (total_frames % pattern_size) as u8;
        let last_pat_frames = if last_pat_frames == 0 {
            pattern_size as u8
        } else {
            last_pat_frames
        };

        // Header format (14 bytes):
        // 0: pattern_size (u8)
        // 1: num_unique (u8)
        // 2: seq_len (u8)
        // 3: loop_pattern (u8, 255=none)
        // 4: frame_rate_hz (u8)
        // 5..9: master_clock_hz (u32 LE)
        // 9: last_pat_frames (u8)
        // 10..14: reserved (4 bytes zero)
        let mut payload = vec![
            pattern_size as u8,
            pattern_blobs.len() as u8,
            sequence_table.len() as u8,
            loop_pattern,
            sequence.timing.frame_rate.hz_value() as u8,
        ];
        payload.extend_from_slice(&sequence.timing.master_clock_hz.to_le_bytes());
        payload.push(last_pat_frames);
        payload.extend_from_slice(&[0u8; 4]); // reserved

        // Sequence table
        payload.extend_from_slice(&sequence_table);

        // Offset table (u32 LE per unique pattern)
        let mut running_offset = 0u32;
        let mut offsets = Vec::new();
        for blob in &pattern_blobs {
            offsets.push(running_offset);
            running_offset += blob.len() as u32;
        }
        for offset in offsets {
            payload.extend_from_slice(&offset.to_le_bytes());
        }

        // Pattern payload data
        for blob in pattern_blobs {
            payload.extend_from_slice(&blob);
        }

        Ok(payload)
    }

    fn encode_pattern_chunk(
        frames: &[crate::sequence::TiaFrame],
        target_len: usize,
        level: CompressionLevel,
        options: &CompilerOptions,
    ) -> Vec<u8> {
        let mut blob = Vec::new();
        let mut current_state = [0u8; 6]; // AUDF0, AUDC0, AUDV0, AUDF1, AUDC1, AUDV1

        let mut i = 0;
        while i < frames.len() {
            let f = &frames[i];

            // Resolve target register values for this frame
            let next_audf0 = f.audf0.unwrap_or(current_state[0]) & 0x1F;
            let next_audc0 = f.audc0.unwrap_or(current_state[1]) & 0x0F;
            let next_audv0 = f.audv0.unwrap_or(current_state[2]) & 0x0F;
            let next_audf1 = f.audf1.unwrap_or(current_state[3]) & 0x1F;
            let next_audc1 = f.audc1.unwrap_or(current_state[4]) & 0x0F;
            let next_audv1 = f.audv1.unwrap_or(current_state[5]) & 0x0F;

            let next_state = [
                next_audf0, next_audc0, next_audv0, next_audf1, next_audc1, next_audv1,
            ];

            // Check if this frame is unchanged from current_state
            if options.rle && level == CompressionLevel::Full && next_state == current_state {
                // Count consecutive unchanged frames
                let mut run_len = 1;
                while i + run_len < frames.len() && run_len < 127 {
                    let peek = &frames[i + run_len];
                    let p0 = peek.audf0.unwrap_or(current_state[0]) & 0x1F;
                    let p1 = peek.audc0.unwrap_or(current_state[1]) & 0x0F;
                    let p2 = peek.audv0.unwrap_or(current_state[2]) & 0x0F;
                    let p3 = peek.audf1.unwrap_or(current_state[3]) & 0x1F;
                    let p4 = peek.audc1.unwrap_or(current_state[4]) & 0x0F;
                    let p5 = peek.audv1.unwrap_or(current_state[5]) & 0x0F;
                    if [p0, p1, p2, p3, p4, p5] == current_state {
                        run_len += 1;
                    } else {
                        break;
                    }
                }

                if run_len >= 2 {
                    blob.push(RLE_FLAG);
                    blob.push(run_len as u8);
                    i += run_len;
                    continue;
                }
            }

            // Normal delta or uncompressed encoding
            if level == CompressionLevel::None {
                blob.push(0x3F); // All 6 bits set
                blob.extend_from_slice(&next_state);
            } else {
                let mut mask = 0u8;
                let mut data = Vec::new();

                for reg_idx in 0..6 {
                    if next_state[reg_idx] != current_state[reg_idx]
                        || level == CompressionLevel::None
                    {
                        mask |= 1 << reg_idx;
                        data.push(next_state[reg_idx]);
                    }
                }

                blob.push(mask);
                blob.extend_from_slice(&data);
            }

            current_state = next_state;
            i += 1;
        }

        // If chunk is shorter than target_len (e.g. last pattern), pad with idle if needed
        let remaining = target_len.saturating_sub(frames.len());
        if remaining > 0 && options.rle && level == CompressionLevel::Full {
            blob.push(RLE_FLAG);
            blob.push(remaining as u8);
        }

        blob
    }
}
