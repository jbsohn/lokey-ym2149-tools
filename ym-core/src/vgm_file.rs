use std::borrow::Cow;
use std::io::Read;

use crate::error::YmError;
use crate::sequence::YmSequence;
use crate::traits::SongInput;
use crate::ym_file::YmFile;

/// Refuse gzip streams that expand past this many bytes.
const MAX_UNCOMPRESSED: usize = 32 * 1024 * 1024;
/// Refuse to materialize more playback frames than this (about 5.5 hours at 50 Hz).
const MAX_FRAMES: usize = 1_000_000;
/// VGM sample clock. One frame is `44_100 / source_hz` samples.
const VGM_SAMPLE_RATE: u32 = 44_100;
/// Bit 30 is the dual-chip flag. Bit 31 is masked as well so a stray chip-type
/// flag cannot inflate the clock into the gigahertz range.
const CLOCK_META_MASK: u32 = 0xC000_0000;
/// AY8910 flags byte at `0x79`, bit 4: YM2149 pin 26 internal `/2` divider.
/// Furnace sets this for "Half Clock divider" and for Sunsoft 5B.
const AY_FLAG_PIN26_DIV2: u8 = 0x10;

/// One step of a VGM command stream.
enum Event {
    End,
    Wait(u32),
    /// Chip-0 write to an audible AY register (`0..=13`).
    Ay {
        reg: u8,
        value: u8,
    },
    Other,
}

/// A `.vgm` or `.vgz` song log, discretized to one YM-2149 register frame per tick.
///
/// Furnace Tracker exports AY-3-8910 / YM2149 music as VGM. Register writes are
/// sampled on the file's playback rate (50 Hz or 60 Hz). R13 is stored in byte 13
/// of a frame only when the stream wrote it during that frame; otherwise byte 13
/// is `0xFF` (no hardware-envelope retrigger), matching [`.ym`](crate::YmFile) dumps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VgmFile {
    /// Song name or file stem.
    pub name: String,
    /// PSG master clock in Hz, after the YM2149 pin-26 divider and any caller override.
    pub source_clock: u32,
    /// Playback frame rate in Hz (50 or 60, unless the header names another rate).
    pub source_hz: u16,
    /// Frame index the loop point falls on, when the header has one.
    pub loop_frame: Option<usize>,
    /// Sanitized 16-byte frames. Byte 13 is `0xFF` when R13 was not written.
    pub raw_frames: Vec<[u8; 16]>,
    /// Byte length after gzip decompression.
    pub uncompressed_len: usize,
}

impl VgmFile {
    /// Parses a `.vgm` or `.vgz` buffer.
    ///
    /// Gzip (`1F 8B`) is inflated either way, so a compressed `.vgm` and a `.vgz`
    /// take the same path. `source_clock_override` replaces the header clock when
    /// it is non-zero. The header clock is halved when AY flags bit 4 is set
    /// (YM2149 pin 26), which is how Furnace records its half-clock option.
    ///
    /// # Errors
    /// Returns an error if the magic is wrong, the file is truncated, gzip
    /// expansion exceeds 32 MiB, the AY-3-8910 clock is missing, or the command
    /// stream contains an opcode this parser cannot skip safely.
    pub fn from_bytes(
        name: &str,
        data: &[u8],
        source_clock_override: Option<u32>,
    ) -> Result<Self, YmError> {
        let decompressed = inflate_if_gzip(data)?;
        let uncompressed_len = decompressed.len();
        let header = parse_header(&decompressed, source_clock_override)?;
        let source_hz = detect_hz(&decompressed, header.data_start, header.rate)?;
        let samples_per_frame = VGM_SAMPLE_RATE / u32::from(source_hz);
        let raw_frames = parse_frames(
            &decompressed,
            header.data_start,
            header.loop_abs,
            samples_per_frame,
        )?;
        let loop_frame = raw_frames.1.filter(|frame| *frame < raw_frames.0.len());

        Ok(Self {
            name: name.to_string(),
            source_clock: header.source_clock,
            source_hz,
            loop_frame,
            raw_frames: raw_frames.0,
            uncompressed_len,
        })
    }
}

impl SongInput for VgmFile {
    fn title(&self) -> &str {
        &self.name
    }

    fn source_clock(&self) -> u32 {
        self.source_clock
    }

    fn source_hz(&self) -> u16 {
        self.source_hz
    }

    fn to_sequence(&self, target_clock: Option<u32>) -> Result<YmSequence, YmError> {
        Ok(YmFile::sequence_from_raw(
            &self.name,
            self.source_clock,
            self.source_hz,
            self.loop_frame,
            &self.raw_frames,
            target_clock,
        ))
    }
}

struct ParsedHeader {
    data_start: usize,
    source_clock: u32,
    rate: u32,
    loop_abs: Option<usize>,
}

fn inflate_if_gzip(data: &[u8]) -> Result<Cow<'_, [u8]>, YmError> {
    if data.len() >= 2 && data[0] == 0x1F && data[1] == 0x8B {
        let mut decoder = flate2::read::GzDecoder::new(data);
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = decoder.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if out.len().saturating_add(n) > MAX_UNCOMPRESSED {
                return Err(YmError::DecompressedTooLarge {
                    max: MAX_UNCOMPRESSED,
                });
            }
            out.extend_from_slice(&buf[..n]);
        }
        Ok(Cow::Owned(out))
    } else {
        Ok(Cow::Borrowed(data))
    }
}

fn parse_header(data: &[u8], clock_override: Option<u32>) -> Result<ParsedHeader, YmError> {
    if data.len() < 4 {
        return Err(YmError::TruncatedHeader {
            expected: 4,
            actual: data.len(),
        });
    }
    if &data[0..4] != b"Vgm " {
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&data[0..4]);
        return Err(YmError::InvalidVgmMagic(magic));
    }
    if data.len() < 0x40 {
        return Err(YmError::TruncatedHeader {
            expected: 0x40,
            actual: data.len(),
        });
    }

    let version = read_u32(data, 0x08);
    let data_start = vgm_data_start(data, version);
    if data_start > data.len() {
        return Err(YmError::TruncatedHeader {
            expected: data_start,
            actual: data.len(),
        });
    }
    // The AY clock lives at 0x74 and only exists once the header extends past it.
    if data_start < 0x78 {
        return Err(YmError::MissingAy8910Clock);
    }

    let clock_word = read_u32(data, 0x74);
    let header_clock = clock_word & !CLOCK_META_MASK;
    // Flags at 0x79 count only when that byte is still inside the header.
    let flags = if data_start > 0x79 { data[0x79] } else { 0 };
    let source_clock = resolve_clock(header_clock, flags, clock_override)?;
    let rate = read_u32(data, 0x24);
    let loop_rel = read_u32(data, 0x1C);
    let loop_abs = if loop_rel == 0 {
        None
    } else {
        0x1Cusize
            .checked_add(loop_rel as usize)
            .filter(|&abs| (data_start..data.len()).contains(&abs))
    };

    Ok(ParsedHeader {
        data_start,
        source_clock,
        rate,
        loop_abs,
    })
}

fn vgm_data_start(data: &[u8], version: u32) -> usize {
    if version < 0x150 {
        return 0x40;
    }
    let rel = read_u32(data, 0x34);
    if rel == 0 {
        0x40
    } else {
        0x34usize.saturating_add(rel as usize)
    }
}

fn resolve_clock(
    header_clock: u32,
    flags: u8,
    clock_override: Option<u32>,
) -> Result<u32, YmError> {
    if let Some(clock) = clock_override {
        if clock == 0 {
            return Err(YmError::MissingAy8910Clock);
        }
        return Ok(clock);
    }
    if header_clock == 0 {
        return Err(YmError::MissingAy8910Clock);
    }
    let clock = if flags & AY_FLAG_PIN26_DIV2 == 0 {
        header_clock
    } else {
        header_clock / 2
    };
    if clock == 0 {
        Err(YmError::MissingAy8910Clock)
    } else {
        Ok(clock)
    }
}

fn detect_hz(data: &[u8], data_start: usize, rate: u32) -> Result<u16, YmError> {
    if rate == 50 || rate == 60 {
        return Ok(u16::try_from(rate).unwrap_or(60));
    }
    let mut wait_50 = 0u32;
    let mut wait_60 = 0u32;
    let mut pos = data_start;
    while pos < data.len() {
        let opcode = data[pos];
        let (next, event) = next_event(data, pos)?;
        match opcode {
            0x62 => wait_60 += 1,
            0x63 => wait_50 += 1,
            _ => {}
        }
        if matches!(event, Event::End) {
            break;
        }
        pos = next;
    }
    if wait_50 > wait_60 {
        Ok(50)
    } else {
        Ok(60)
    }
}

fn parse_frames(
    data: &[u8],
    data_start: usize,
    loop_abs: Option<usize>,
    samples_per_frame: u32,
) -> Result<(Vec<[u8; 16]>, Option<usize>), YmError> {
    let mut regs = [0u8; 14];
    regs[7] = 0x3F;
    let mut r13_written = false;
    let mut accum = 0u32;
    let mut frames = Vec::new();
    let mut loop_frame = None;
    let mut pos = data_start;

    while pos < data.len() {
        if loop_abs == Some(pos) {
            loop_frame = Some(frames.len());
        }
        let (next, event) = next_event(data, pos)?;
        match event {
            Event::End => break,
            Event::Wait(samples) => {
                accum = accum.saturating_add(samples);
                emit_elapsed(
                    &mut frames,
                    &regs,
                    &mut r13_written,
                    &mut accum,
                    samples_per_frame,
                )?;
            }
            Event::Ay { reg, value } => {
                regs[usize::from(reg)] = value;
                if reg == 13 {
                    r13_written = true;
                }
            }
            Event::Other => {}
        }
        pos = next;
    }

    if let Some(k) = loop_frame {
        if k > 0 && frames.len() > k && frames.ends_with(&frames[..k]) {
            frames.truncate(frames.len() - k);
            loop_frame = Some(0);
        }
    }

    Ok((frames, loop_frame))
}

fn emit_elapsed(
    frames: &mut Vec<[u8; 16]>,
    regs: &[u8; 14],
    r13_written: &mut bool,
    accum: &mut u32,
    samples_per_frame: u32,
) -> Result<(), YmError> {
    while *accum >= samples_per_frame {
        if frames.len() >= MAX_FRAMES {
            return Err(YmError::TooManyFrames { max: MAX_FRAMES });
        }
        frames.push(capture_frame(regs, *r13_written));
        *r13_written = false;
        *accum -= samples_per_frame;
    }
    Ok(())
}

fn capture_frame(regs: &[u8; 14], r13_written: bool) -> [u8; 16] {
    let mut raw = [0u8; 16];
    raw[..14].copy_from_slice(regs);
    raw[13] = if r13_written { regs[13] & 0x0F } else { 0xFF };
    YmFile::sanitize_raw16(&raw).0
}

fn next_event(data: &[u8], pos: usize) -> Result<(usize, Event), YmError> {
    if pos >= data.len() {
        return Ok((pos, Event::End));
    }
    let opcode = data[pos];
    match opcode {
        0x66 => Ok((pos + 1, Event::End)),
        0x61 => {
            need(data, pos, 3)?;
            let samples = u32::from(u16::from_le_bytes([data[pos + 1], data[pos + 2]]));
            Ok((pos + 3, Event::Wait(samples)))
        }
        0x62 => Ok((pos + 1, Event::Wait(735))),
        0x63 => Ok((pos + 1, Event::Wait(882))),
        0x70..=0x7F => Ok((pos + 1, Event::Wait(u32::from(opcode & 0x0F) + 1))),
        0x80..=0x8F => Ok((pos + 1, Event::Wait(u32::from(opcode & 0x0F)))),
        0xA0 => read_ay_write(data, pos),
        0x67 => skip_data_block(data, pos),
        0x68 => {
            // 0x68 0x66 cc oo oo oo dd dd dd ss ss ss
            need(data, pos, 12)?;
            Ok((pos + 12, Event::Other))
        }
        other => {
            let Some(extra) = operand_len(other) else {
                return Err(YmError::UnsupportedVgmCommand(other));
            };
            need(data, pos, 1 + extra)?;
            Ok((pos + 1 + extra, Event::Other))
        }
    }
}

fn read_ay_write(data: &[u8], pos: usize) -> Result<(usize, Event), YmError> {
    need(data, pos, 3)?;
    let aa = data[pos + 1];
    let value = data[pos + 2];
    let event = if aa & 0x80 == 0 {
        let reg = aa & 0x7F;
        if reg < 14 {
            Event::Ay { reg, value }
        } else {
            Event::Other
        }
    } else {
        Event::Other
    };
    Ok((pos + 3, event))
}

fn skip_data_block(data: &[u8], pos: usize) -> Result<(usize, Event), YmError> {
    // 0x67 0x66 tt ss ss ss ss, then `ss` payload bytes.
    need(data, pos, 7)?;
    let size = read_u32(data, pos + 3) as usize;
    let end = pos
        .checked_add(7)
        .and_then(|start| start.checked_add(size))
        .ok_or(YmError::UnexpectedEof("VGM data block"))?;
    if end > data.len() {
        return Err(YmError::UnexpectedEof("VGM data block"));
    }
    Ok((end, Event::Other))
}

/// Operand byte count for documented commands this parser only skips.
/// `None` means the opcode is not in the VGM spec, so parsing must stop.
fn operand_len(opcode: u8) -> Option<usize> {
    match opcode {
        0x62 | 0x63 | 0x66 | 0x70..=0x8F => Some(0),
        0x30..=0x3F | 0x4F | 0x50 | 0x94 => Some(1),
        0x40..=0x4E | 0x51..=0x5F | 0x61 | 0xA0..=0xBF => Some(2),
        0x90 | 0x91 | 0x95 | 0xE0..=0xFF => Some(4),
        0x92 => Some(5),
        0x93 => Some(10),
        0xC0..=0xDF => Some(3),
        _ => None,
    }
}

fn need(data: &[u8], pos: usize, len: usize) -> Result<(), YmError> {
    if pos.checked_add(len).is_some_and(|end| end <= data.len()) {
        Ok(())
    } else {
        Err(YmError::UnexpectedEof("VGM command stream"))
    }
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::SongContainer;
    use crate::traits::SongInput;
    use crate::ysg::compile_ysg;
    use crate::YsgFile;

    const DATA_START: usize = 0x100;

    fn build_vgm(
        clock: u32,
        rate: u32,
        flags: u8,
        loop_abs: Option<usize>,
        commands: &[u8],
    ) -> Vec<u8> {
        let mut file = vec![0u8; DATA_START];
        file[0..4].copy_from_slice(b"Vgm ");
        file[8..12].copy_from_slice(&0x170u32.to_le_bytes());
        file[0x24..0x28].copy_from_slice(&rate.to_le_bytes());
        let data_rel = u32::try_from(DATA_START - 0x34).unwrap();
        file[0x34..0x38].copy_from_slice(&data_rel.to_le_bytes());
        file[0x74..0x78].copy_from_slice(&clock.to_le_bytes());
        file[0x78] = 0x10;
        file[0x79] = flags;
        file.extend_from_slice(commands);
        if let Some(abs) = loop_abs {
            let rel = u32::try_from(abs - 0x1C).unwrap();
            file[0x1C..0x20].copy_from_slice(&rel.to_le_bytes());
        }
        let eof = u32::try_from(file.len() - 4).unwrap();
        file[4..8].copy_from_slice(&eof.to_le_bytes());
        file
    }

    fn gzip(raw: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn parses_50hz_and_preserves_r13_retrigger() {
        let bytes = build_vgm(
            2_000_000,
            50,
            0x01,
            None,
            &[
                0xA0, 0x00, 0x34, // R0
                0xA0, 0x01, 0x01, // R1
                0xA0, 0x08, 0x0F, // volume A
                0xA0, 0x07, 0x3E, // tone A on
                0xA0, 0x0D, 0x0E, // envelope shape
                0x63, // one 50 Hz frame
                0xA0, 0x00, 0x00, // no R13 this frame
                0x63, 0x66,
            ],
        );
        let vgm = VgmFile::from_bytes("fifty", &bytes, None).unwrap();
        assert_eq!(vgm.source_hz, 50);
        assert_eq!(vgm.source_clock, 2_000_000);
        assert_eq!(vgm.raw_frames.len(), 2);
        assert_eq!(vgm.raw_frames[0][13], 0x0E);
        assert_eq!(vgm.raw_frames[1][13], 0xFF);

        let seq = vgm.to_sequence(Some(2_000_000)).unwrap();
        assert_eq!(seq.frames[0].tone_a, Some(0x0134));
        assert_eq!(seq.frames[0].volume_a, Some(0x0F));
        assert_eq!(seq.frames[0].tone_enable_a, Some(true));
        assert_eq!(seq.frames[0].envelope_shape, Some(0x0E));
        assert_eq!(seq.frames[1].tone_a, Some(0x0100));
        assert_eq!(seq.frames[1].envelope_shape, None);
        assert_eq!(seq.timing.master_clock_hz, 2_000_000);
        assert_eq!(seq.timing.frame_rate.hz_value(), 50);
    }

    #[test]
    fn parses_60hz_wait_when_header_rate_is_zero() {
        let bytes = build_vgm(
            1_789_772,
            0,
            0x01,
            None,
            &[0xA0, 0x00, 0x10, 0xA0, 0x08, 0x0C, 0x62, 0x66],
        );
        let vgm = VgmFile::from_bytes("sixty", &bytes, None).unwrap();
        assert_eq!(vgm.source_hz, 60);
        assert_eq!(vgm.raw_frames.len(), 1);
        let seq = vgm.to_sequence(Some(1_789_772)).unwrap();
        assert_eq!(seq.frames[0].tone_a, Some(0x0010));
        assert_eq!(seq.frames[0].volume_a, Some(0x0C));
        assert_eq!(seq.frames[0].envelope_shape, None);
    }

    #[test]
    fn fifty_hz_predominates_when_header_rate_is_zero() {
        let bytes = build_vgm(2_000_000, 0, 0x01, None, &[0x63, 0x63, 0x62, 0x66]);
        let vgm = VgmFile::from_bytes("mixed", &bytes, None).unwrap();
        assert_eq!(vgm.source_hz, 50);
        assert_eq!(vgm.raw_frames.len(), 2);
    }

    #[test]
    fn gzip_round_trip_matches_raw_vgm() {
        let raw = build_vgm(
            2_000_000,
            50,
            0x01,
            None,
            &[0xA0, 0x00, 0x22, 0xA0, 0x0D, 0x0A, 0x63, 0x66],
        );
        let plain = VgmFile::from_bytes("song", &raw, None).unwrap();
        let packed = VgmFile::from_bytes("song", &gzip(&raw), None).unwrap();
        assert_eq!(packed.raw_frames, plain.raw_frames);
        assert_eq!(packed.source_hz, plain.source_hz);
        assert_eq!(packed.source_clock, plain.source_clock);
        assert_eq!(packed.uncompressed_len, raw.len());
    }

    #[test]
    fn records_loop_frame_from_header_offset() {
        let mut commands = vec![0xA0, 0x00, 0x11, 0xA0, 0x0D, 0x0C, 0x63];
        let loop_abs = DATA_START + commands.len();
        commands.extend_from_slice(&[0xA0, 0x00, 0x22, 0x63, 0x66]);
        let bytes = build_vgm(2_000_000, 50, 0x01, Some(loop_abs), &commands);
        let vgm = VgmFile::from_bytes("loop", &bytes, None).unwrap();
        assert_eq!(vgm.loop_frame, Some(1));
        let seq = vgm.to_sequence(Some(2_000_000)).unwrap();
        assert_eq!(seq.loop_start, Some(1));
        assert_eq!(seq.frames[0].envelope_shape, Some(0x0C));
        assert_eq!(seq.frames[1].envelope_shape, None);
        assert_eq!(seq.frames[1].tone_a, Some(0x0022));
    }

    #[test]
    fn ignores_second_chip_and_skips_data_blocks() {
        let bytes = build_vgm(
            2_000_000,
            50,
            0x01,
            None,
            &[
                0x67, 0x66, 0x00, 0x04, 0x00, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0xA0, 0x00, 0x55,
                0xA0, 0x80, 0xFF, // second chip, ignored
                0xA0, 0x0D, 0x08, 0x63, 0x66,
            ],
        );
        let seq = VgmFile::from_bytes("skip", &bytes, None)
            .unwrap()
            .to_sequence(Some(2_000_000))
            .unwrap();
        assert_eq!(seq.frames.len(), 1);
        assert_eq!(seq.frames[0].tone_a, Some(0x0055));
        assert_eq!(seq.frames[0].envelope_shape, Some(0x08));
    }

    #[test]
    fn half_clock_flag_divides_and_dual_chip_bit_does_not() {
        let commands = &[0x63, 0x66];
        let halved = VgmFile::from_bytes(
            "half",
            &build_vgm(2_000_000, 50, AY_FLAG_PIN26_DIV2, None, commands),
            None,
        )
        .unwrap();
        assert_eq!(halved.source_clock, 1_000_000);

        let mut dual_clock = 2_000_000u32;
        dual_clock |= 0x4000_0000;
        let dual = VgmFile::from_bytes(
            "dual",
            &build_vgm(dual_clock, 50, 0x01, None, commands),
            None,
        )
        .unwrap();
        assert_eq!(dual.source_clock, 2_000_000);

        let overridden = VgmFile::from_bytes(
            "over",
            &build_vgm(2_000_000, 50, AY_FLAG_PIN26_DIV2, None, commands),
            Some(1_789_773),
        )
        .unwrap();
        assert_eq!(overridden.source_clock, 1_789_773);
    }

    #[test]
    fn scales_pitch_to_target_clock() {
        let bytes = build_vgm(
            2_000_000,
            50,
            0x01,
            None,
            &[0xA0, 0x00, 0xD0, 0xA0, 0x01, 0x07, 0x63, 0x66],
        );
        let seq = VgmFile::from_bytes("pitch", &bytes, None)
            .unwrap()
            .to_sequence(Some(1_000_000))
            .unwrap();
        // 0x07D0 = 2000, halved with the clock.
        assert_eq!(seq.frames[0].tone_a, Some(1000));
        assert_eq!(seq.timing.master_clock_hz, 1_000_000);
    }

    #[test]
    fn rejects_missing_clock_bad_magic_and_unknown_opcode() {
        let silent = build_vgm(0, 50, 0x01, None, &[0x66]);
        let err = VgmFile::from_bytes("noclock", &silent, None).unwrap_err();
        assert!(matches!(err, YmError::MissingAy8910Clock));

        let err = VgmFile::from_bytes("nope", b"RIFF", None).unwrap_err();
        assert!(matches!(err, YmError::InvalidVgmMagic(_)));

        let bytes = build_vgm(2_000_000, 50, 0x01, None, &[0x64, 0x00, 0x66]);
        let err = VgmFile::from_bytes("badop", &bytes, None).unwrap_err();
        assert!(matches!(err, YmError::UnsupportedVgmCommand(0x64)));

        let bytes_zero = build_vgm(2_000_000, 50, 0x01, None, &[0x00, 0x66]);
        let err = VgmFile::from_bytes("zero_op", &bytes_zero, None).unwrap_err();
        assert!(matches!(err, YmError::UnsupportedVgmCommand(0x00)));
    }

    #[test]
    fn container_accepts_vgm_and_vgz_extensions() {
        let raw = build_vgm(2_000_000, 60, 0x01, None, &[0xA0, 0x08, 0x0A, 0x62, 0x66]);
        let vgm = SongContainer::from_bytes("demo", "VGM", &raw, None).unwrap();
        assert_eq!(vgm.source_hz(), 60);
        assert_eq!(
            vgm.to_sequence("demo", Some(2_000_000))
                .unwrap()
                .frames
                .len(),
            1
        );

        let vgz = SongContainer::from_bytes("demo", "vgz", &gzip(&raw), None).unwrap();
        assert_eq!(vgz.title(), "demo");
        assert_eq!(
            vgz.to_sequence("demo", Some(2_000_000)).unwrap().frames[0].volume_a,
            Some(0x0A)
        );
    }

    #[test]
    fn compiles_vgm_through_ysg_round_trip() {
        let mut commands = Vec::new();
        for i in 0u8..16 {
            commands.extend_from_slice(&[0xA0, 0x00, i.wrapping_mul(3), 0xA0, 0x08, 0x0F]);
            if i % 2 == 0 {
                commands.extend_from_slice(&[0xA0, 0x0D, 0x0E]);
            }
            commands.push(0x63);
        }
        commands.push(0x66);
        let bytes = build_vgm(2_000_000, 50, 0x01, None, &commands);
        let seq = VgmFile::from_bytes("round", &bytes, None)
            .unwrap()
            .to_sequence(Some(1_789_773))
            .unwrap();
        assert_eq!(seq.frames.len(), 16);

        let details = compile_ysg(&seq, 16).unwrap();
        let decoded = YmSequence::from_ysg("round", &details.bytes).unwrap();
        assert_eq!(decoded.frames.len(), seq.frames.len());
        assert_eq!(decoded.timing.master_clock_hz, seq.timing.master_clock_hz);
        assert_eq!(
            decoded.timing.frame_rate.hz_value(),
            seq.timing.frame_rate.hz_value()
        );
        for (index, (orig, dec)) in seq.frames.iter().zip(decoded.frames.iter()).enumerate() {
            assert_eq!(dec.tone_a, orig.tone_a, "tone A at frame {index}");
            assert_eq!(dec.volume_a, orig.volume_a, "volume A at frame {index}");
            assert_eq!(
                dec.envelope_shape, orig.envelope_shape,
                "envelope at frame {index}"
            );
        }
    }

    #[test]
    fn parses_ay_demo_fixture_when_present() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
        let path_fixtures =
            std::path::Path::new(&manifest_dir).join("../tests/fixtures/song/ay-demo.vgm");
        let path_jam =
            std::path::Path::new(&manifest_dir).join("../tests/fixtures/song/AY-3-8910_Jam.vgm");
        let path_root = std::path::Path::new(&manifest_dir).join("../ay-demo.vgm");
        let path = if path_fixtures.exists() {
            path_fixtures
        } else if path_jam.exists() {
            path_jam
        } else if path_root.exists() {
            path_root
        } else {
            return;
        };
        let bytes = std::fs::read(&path).unwrap();
        let vgm = VgmFile::from_bytes("ay-demo", &bytes, None).unwrap();
        assert_eq!(vgm.source_clock, 1_789_772);
        assert_eq!(vgm.source_hz, 50);
        assert_eq!(vgm.loop_frame, Some(0));
        assert_eq!(vgm.raw_frames.len(), 5376);

        let seq = vgm.to_sequence(Some(vgm.source_clock)).unwrap();
        assert_eq!(seq.frames[0].tone_a, Some(0x0377));
        assert_eq!(seq.frames[0].tone_b, Some(0x01BC));
        assert_eq!(seq.frames[0].tone_c, Some(0x02EA));
        assert_eq!(seq.frames[0].volume_a, Some(0x0F));
        assert_eq!(seq.frames[0].envelope_shape, Some(0x0C));
        assert_eq!(seq.frames[1].tone_c, Some(0x0346));
        assert_eq!(seq.frames[1].envelope_shape, None);
        assert_eq!(seq.loop_start, Some(0));

        let (ysg, details) = YsgFile::from_sequence_optimal(&seq).unwrap();
        assert_eq!(ysg.header.loop_step, 0);
        let pat_len = usize::from(ysg.header.pattern_frames);
        assert_eq!(seq.frames.len() % pat_len, 0);
        assert_eq!(usize::from(ysg.header.seq_len) * pat_len, seq.frames.len());
        assert_eq!(details.seq_len, ysg.header.seq_len);
    }

    #[test]
    fn deduplicates_loop_wraparound_frame() {
        let commands = [
            0xA0, 8, 15,   // vol A = 15
            0x63, // wait 882 (50Hz) -> Frame 0
            0xA0, 8, 10,   // vol A = 10
            0x63, // wait 882 -> Frame 1
            0xA0, 8, 15,   // vol A = 15 (duplicates Frame 0)
            0x63, // wait 882 -> Frame 2
            0x66, // end
        ];
        // Frame 1 starts at DATA_START + 4 (after 0xA0, 8, 15, 0x63)
        let loop_abs = Some(DATA_START + 4);
        let bytes = build_vgm(2_000_000, 50, 0x01, loop_abs, &commands);

        let vgm = VgmFile::from_bytes("test_loop", &bytes, None).unwrap();
        assert_eq!(vgm.loop_frame, Some(0));
        assert_eq!(vgm.raw_frames.len(), 2);
    }
}
