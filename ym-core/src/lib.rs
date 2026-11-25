pub mod ayfx;
pub mod error;
pub mod player;
pub mod sequence;
pub mod timing;
pub mod vgm_file;
pub mod yfx;
pub mod ym_file;
pub mod ysg;

pub use ayfx::AyfxFile;
pub use error::{Result, YmError};
pub use player::{YmDataRenderer, YmMixer, YmSfxRenderer, YmSongRenderer};
pub use sequence::{SfxFrame, SfxSequence, YmChannel, YmFrame, YmSequence};
pub use timing::{
    APPLE2_CLOCK, ATARI_7800_CLOCK, ATARI_ST_CLOCK, TimingConfig, ZX_SPECTRUM_CLOCK,
};
pub use vgm_file::VgmFile;
pub use yfx::YfxFile;
pub use ym_file::YmFile;
pub use ysg::{
    CANDIDATE_PATTERN_FRAMES, GlobalFrame, SENTINEL_EMPTY_PATTERN, TrackDescriptor, VoiceFrame,
    YSG_HEADER_SIZE, YSG_MAGIC, YSG_MAX_FILE_SIZE, YSG_VERSION, YsgFile, YsgHeader, YsgSongDetails,
    compile_ysg, compile_ysg_optimal, decompile_ysg,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_defaults() {
        let st = TimingConfig::ATARI_ST;
        assert_eq!(st.frame_rate, 50);
        assert!((st.frame_duration_ms() - 20.0).abs() < f64::EPSILON);

        let a7800 = TimingConfig::ATARI_7800;
        assert_eq!(a7800.frame_rate, 60);
        assert_eq!(a7800.master_clock_hz, ATARI_7800_CLOCK);
    }

    #[test]
    fn test_calculate_delay() {
        // Hand-computed against the same formula as a port-fidelity check:
        // remaining = 1_789_773/hz - 1800, y = floor(remaining/1285), x = round((remaining - y*1285)/5)
        assert_eq!(TimingConfig::calculate_delay_for_hz(50), (26, 117));
        assert_eq!(TimingConfig::calculate_delay_for_hz(60), (21, 209));
    }

    #[test]
    fn test_yfx_compilation_basic() {
        let seq = SfxSequence::new(
            "test_sfx",
            vec![SfxFrame::new(true, false, 450, 0, 15, 1)],
        );

        let payload = YfxFile::from_sequence(&seq).to_bytes();
        assert_eq!(payload.len(), 5);
        assert_eq!(payload[0], 194); // 450 & 0xFF = 194
    }

    #[test]
    fn test_ayfx_csv_parsing_and_optional_duration() {
        let csv_data = "# comment\n0,1,0x8a8,0x1f,0xf\n1,0,0x1c2,0x00,0xe,4";
        let ayfx = AyfxFile::from_csv("laser", csv_data).unwrap();
        assert_eq!(ayfx.sequences.len(), 1);
        let seq = &ayfx.sequences[0];

        assert_eq!(seq.name, "laser");
        assert_eq!(seq.frames.len(), 2);

        let frame0 = seq.frames[0];
        assert!(!frame0.tone_enable);
        assert!(frame0.noise_enable);
        assert_eq!(frame0.tone, 2216); // 0x8a8 = 2216
        assert_eq!(frame0.noise, 31); // 0x1f = 31
        assert_eq!(frame0.volume, 15); // 0xf = 15
        assert_eq!(frame0.duration, 1);

        let frame1 = seq.frames[1];
        assert!(frame1.tone_enable);
        assert!(!frame1.noise_enable);
        assert_eq!(frame1.tone, 450); // 0x1c2 = 450
        assert_eq!(frame1.volume, 14);
        assert_eq!(frame1.duration, 4);

        // Zero duration is rejected
        assert!(AyfxFile::from_csv("bad", "1,0,0x100,0,15,0").is_err());
    }

    #[test]
    fn test_ayfx_bank_parsing() {
        let bank_bytes = vec![
            1, 1, 0, 237, 31, 0, 0, 173, 37, 0, 172, 43, 0, 172, 49, 0, 172, 55, 0, 172, 61, 0,
            172, 67, 0, 172, 73, 0, 172, 79, 0, 172, 85, 0, 172, 91, 0, 172, 97, 0, 172, 103, 0,
            172, 109, 0, 172, 115, 0, 172, 121, 0, 172, 127, 0, 172, 133, 0, 172, 139, 0, 172, 145,
            0, 171, 151, 0, 170, 157, 0, 169, 163, 0, 168, 169, 0, 167, 175, 0, 166, 181, 0, 165,
            187, 0, 164, 193, 0, 163, 199, 0, 162, 205, 0, 161, 211, 0, 208, 32, 119, 105, 122, 98,
            97, 108, 108, 95, 49, 0,
        ];
        let bank = AyfxFile::from_bank(&bank_bytes).unwrap().to_sequences();
        assert_eq!(bank.len(), 1);
        let seq = &bank[0];
        assert_eq!(seq.name, "wizball_1");
        assert_eq!(seq.frames.len(), 31);
        assert_eq!(seq.frames[0].volume, 13);
        assert!(seq.frames[0].tone_enable);
        assert!(!seq.frames[0].noise_enable);
        assert_eq!(seq.frames[0].tone, 31);
        assert_eq!(seq.frames[0].noise, 0);
    }

    #[test]
    fn test_yfx_round_trip() {
        let source_seq = SfxSequence::new(
            "test_sfx",
            vec![
                SfxFrame::new(true, false, 100, 0, 15, 1),
                SfxFrame::new(true, false, 102, 0, 14, 2),
            ],
        );

        let yfx = YfxFile::from_sequence(&source_seq);
        assert_eq!(yfx.frame_count(), 2);
        let payload = yfx.to_bytes();
        assert_eq!(payload.len(), 10);

        let decoded = YfxFile::from_bytes(&payload).unwrap().to_sequence("test_sfx");
        assert_eq!(decoded, source_seq);
    }

    #[test]
    fn test_ayfx_effect_parsing() {
        let effect_bytes = vec![
            237, 31, 0, 0, 173, 37, 0, 172, 43, 0, 172, 49, 0, 172, 55, 0, 172, 61, 0, 172, 67, 0,
            172, 73, 0, 172, 79, 0, 172, 85, 0, 172, 91, 0, 172, 97, 0, 172, 103, 0, 172, 109, 0,
            172, 115, 0, 172, 121, 0, 172, 127, 0, 172, 133, 0, 172, 139, 0, 172, 145, 0, 171, 151,
            0, 170, 157, 0, 169, 163, 0, 168, 169, 0, 167, 175, 0, 166, 181, 0, 165, 187, 0, 164,
            193, 0, 163, 199, 0, 162, 205, 0, 161, 211, 0, 208, 32,
        ];
        let ayfx = AyfxFile::from_effect("pew", &effect_bytes).unwrap();
        let seq = &ayfx.sequences[0];
        assert_eq!(seq.name, "pew");
        assert_eq!(seq.frames.len(), 31);
        assert_eq!(seq.frames[0].volume, 13);
        assert!(seq.frames[0].tone_enable);
        assert!(!seq.frames[0].noise_enable);
        assert_eq!(seq.frames[0].tone, 31);
        assert_eq!(seq.frames[0].noise, 0);
    }

    #[test]
    fn test_song_compilation_and_parsing() {
        let mut frames = Vec::new();
        for i in 0u16..70u16 {
            frames.push(YmFrame {
                tone_a: Some(200 + i),
                volume_a: Some(15),
                tone_enable_a: Some(true),
                ..Default::default()
            });
        }
        let song = YmSequence {
            name: "test_song".to_string(),
            timing: TimingConfig::new(ATARI_7800_CLOCK, 17),
            loop_start: None,
            frames,
        };

        let details = compile_ysg(&song, 48).unwrap();
        let ysg_bytes = details.bytes;
        assert_eq!(&ysg_bytes[0..2], b"YS");

        let decoded = YsgFile::decompile("test_song", &ysg_bytes).unwrap();
        assert_eq!(decoded.frames[0].tone_a, Some(200));
        assert_eq!(decoded.frames[0].volume_a, Some(15));
        assert_eq!(decoded.frames[69].tone_a, Some(269));
        assert_eq!(decoded.timing.master_clock_hz, ATARI_7800_CLOCK);
        assert_eq!(decoded.timing.frame_rate, 17);
    }

    #[test]
    fn test_zero_hz_safety() {
        let (y, _x) = TimingConfig::calculate_delay_for_hz(0);
        assert!(y > 0);

        let zero_timing = TimingConfig::new(ATARI_7800_CLOCK, 0);
        assert!(zero_timing.frame_duration_ms().is_finite());
    }

    #[test]
    fn test_idle_frames_compression() {
        let mut frames = Vec::new();
        frames.push(YmFrame {
            tone_a: Some(440),
            volume_a: Some(15),
            tone_enable_a: Some(true),
            ..Default::default()
        });
        for _ in 0..50 {
            frames.push(YmFrame::default());
        }
        let song = YmSequence {
            name: "idle_test".to_string(),
            timing: TimingConfig::new(ATARI_7800_CLOCK, 50),
            loop_start: None,
            frames,
        };
        let details = compile_ysg(&song, 64).unwrap();
        assert!(details.track_a_bytes < 20);

        let decoded = YsgFile::decompile("idle_test", &details.bytes).unwrap();
        assert_eq!(decoded.frames.len(), 64);
    }

    #[test]
    fn test_truncated_ysg_returns_err() {
        let truncated_bytes = vec![b'Y', b'S', 1, 0];
        assert!(YsgFile::decompile("bad", &truncated_bytes).is_err());
    }

    #[test]
    fn test_song_renderer_mono() {
        let song = YmSequence {
            name: "test".to_string(),
            timing: TimingConfig::default(),
            loop_start: None,
            frames: vec![YmFrame {
                tone_a: Some(440),
                volume_a: Some(15),
                tone_enable_a: Some(true),
                ..Default::default()
            }],
        };
        let mut renderer = YmSongRenderer::new(&song, 44100);
        assert_eq!(renderer.total_frames(), 1);
        assert!(!renderer.is_finished());

        let mut buf = vec![0.0f32; 1024];
        renderer.render_samples(&mut buf);
        assert!(buf.iter().any(|&s| s != 0.0));
        assert!(renderer.is_finished());
        // Tail samples past the 1-frame mark (~735 samples) are zeroed.
        assert!(buf[800..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_ysg_file_round_trip() {
        let song = YmSequence {
            name: "ysg_test".to_string(),
            timing: TimingConfig::new(ATARI_7800_CLOCK, 50),
            loop_start: Some(0),
            frames: vec![
                YmFrame {
                    tone_a: Some(300),
                    volume_a: Some(15),
                    tone_enable_a: Some(true),
                    ..Default::default()
                };
                32
            ],
        };

        let (ysg_file, details) = YsgFile::from_sequence(&song, 16).unwrap();
        assert_eq!(ysg_file.header.seq_len, 2);
        assert_eq!(ysg_file.header.pattern_frames, 16);

        let binary = ysg_file.to_bytes();
        assert_eq!(binary.len(), details.bytes.len());
        assert_eq!(ysg_file.frame_rate_hz(), 50);
        assert_eq!(ysg_file.master_clock_hz(), ATARI_7800_CLOCK);

        let parsed = YsgFile::from_bytes(&binary).unwrap();
        assert_eq!(parsed.header.seq_len, 2);

        let restored = parsed.to_sequence("restored").unwrap();
        assert_eq!(restored.frames.len(), 32);
        assert_eq!(restored.frames[0].tone_a, Some(300));
        assert_eq!(restored.frames[0].volume_a, Some(15));
    }

    #[test]
    fn test_ym_file_input() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
        let ym_path =
            std::path::Path::new(&manifest_dir).join("../tests/fixtures/song/ND-Loader.ym");
        if !ym_path.exists() {
            return;
        }

        let bytes = std::fs::read(&ym_path).unwrap();
        let ym_file = YmFile::from_bytes("ND-Loader", &bytes, None).unwrap();

        assert_eq!(ym_file.title(), "ND-Loader");
        assert_eq!(ym_file.source_hz(), 50);
        assert!(ym_file.source_clock() > 0);
        assert_ne!(ym_file.raw_frames.len(), 0);

        let seq = ym_file.to_sequence(Some(ATARI_7800_CLOCK));
        assert_eq!(seq.frames.len(), ym_file.raw_frames.len());
        assert_eq!(seq.timing.master_clock_hz, ATARI_7800_CLOCK);
    }

    #[test]
    fn test_try_from_conversions() {
        let raw_yfx = vec![0x12, 0x34, 0x0F, 0x01, 0x01];
        let yfx = YfxFile::try_from(raw_yfx.as_slice()).unwrap();
        assert_eq!(yfx.bytes.len(), 5);

        assert!(YfxFile::try_from([0u8; 4].as_slice()).is_err());
        assert!(YsgHeader::try_from([0u8; 10].as_slice()).is_err());
        assert!(YsgFile::try_from([0u8; 10].as_slice()).is_err());
    }

    #[test]
    fn test_ym_error_display() {
        use std::error::Error;

        let err = YmError::TooManyUniquePatterns {
            count: 256,
            max: 255,
        };
        assert!(err.to_string().contains("255 unique patterns"));

        let err_trunc = YmError::TruncatedHeader {
            expected: 20,
            actual: 4,
        };
        assert!(err_trunc.to_string().contains("expected 20 bytes"));

        let err_song = YmError::SongBinaryTooLarge {
            size: 70000,
            max: 65535,
        };
        assert!(err_song.to_string().contains("70000 bytes"));

        let err_csv = YmError::MalformedCsv {
            line: 4,
            message: "too short".into(),
        };
        assert!(err_csv.to_string().contains("line 4"));

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err_io = YmError::from(io_err);
        assert!(err_io.source().is_some());
    }

    #[test]
    fn test_sequence_from_bytes_dispatch() {
        // SFX CSV
        let csv_data = b"0,1,0x8a8,0x1f,0xf\n0,1,0x8a8,0x1c,0xe";
        let seqs = SfxSequence::from_bytes("laser", "csv", csv_data).unwrap();
        assert_eq!(seqs.len(), 1);
        assert_eq!(seqs[0].name, "laser");

        // SFX YFX
        let raw_yfx = vec![0x12, 0x34, 0x0F, 0x01, 0x01];
        let seqs_yfx = SfxSequence::from_bytes("test_sfx", "yfx", &raw_yfx).unwrap();
        assert_eq!(seqs_yfx.len(), 1);

        // Unsupported SFX extensions (including dropped JSON)
        assert!(SfxSequence::from_bytes("unknown", "wav", b"riff").is_err());
        assert!(SfxSequence::from_bytes("legacy", "json", b"{}").is_err());

        // Song YSG with pitch retuning
        let song_st = YmSequence {
            name: "test_st".to_string(),
            timing: TimingConfig::ATARI_ST,
            loop_start: None,
            frames: vec![YmFrame {
                tone_a: Some(2000),
                volume_a: Some(15),
                tone_enable_a: Some(true),
                ..Default::default()
            }],
        };
        let ysg_bytes = compile_ysg(&song_st, 16).unwrap().bytes;
        let retuned =
            YmSequence::from_bytes("test_st", "ysg", &ysg_bytes, None, Some(1_000_000)).unwrap();
        assert_eq!(retuned.frames[0].tone_a, Some(1000));
        assert_eq!(retuned.timing.master_clock_hz, 1_000_000);

        // Unsupported song extension
        assert!(YmSequence::from_bytes("legacy", "json", b"{}", None, None).is_err());
    }

    #[test]
    fn test_sfx_duration_in_renderer() {
        let seq = SfxSequence::new(
            "dur",
            vec![
                SfxFrame::new(true, false, 440, 0, 15, 3),
                SfxFrame::new(true, false, 440, 0, 10, 1),
            ],
        );

        let mut renderer = YmSfxRenderer::new(&seq, YmChannel::A, TimingConfig::ATARI_ST, 50);
        let mut sample = [0.0f32; 1];

        // Frame 0: duration = 3 ticks.
        renderer.render_samples(&mut sample);
        assert_eq!(renderer.current_frame(), 0);
        assert!(!renderer.is_finished());

        renderer.render_samples(&mut sample);
        assert_eq!(renderer.current_frame(), 0);
        assert!(!renderer.is_finished());

        // Tick 3: finishes frame 0 and advances to frame 1
        renderer.render_samples(&mut sample);
        assert_eq!(renderer.current_frame(), 1);
        assert!(!renderer.is_finished());

        // Tick 4: renders frame 1 and finishes sequence
        renderer.render_samples(&mut sample);
        assert!(renderer.is_finished());
    }

    #[test]
    fn test_r13_consecutive_retrigger() {
        let mut chip = ym2149::Ym2149::new();
        let mut mixer = 0x3F;
        let mut last_shape = None;

        let frame1 = YmFrame {
            envelope_shape: Some(0x0E),
            ..Default::default()
        };
        frame1.apply_to_chip(&mut chip, &mut mixer, &mut last_shape);
        assert_eq!(last_shape, Some(0x0E));

        let frame2 = YmFrame {
            envelope_shape: Some(0x0E),
            ..Default::default()
        };
        frame2.apply_to_chip(&mut chip, &mut mixer, &mut last_shape);
        assert_eq!(last_shape, Some(0x0E));
    }

    #[test]
    fn test_malformed_bank_rejected() {
        let empty: &[u8] = &[];
        assert!(AyfxFile::from_bank(empty).is_err());

        let truncated = vec![2, 1];
        assert!(AyfxFile::from_bank(&truncated).is_err());
    }
}
