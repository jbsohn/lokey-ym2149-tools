pub mod ayfx;
pub mod container;
pub mod error;
pub mod player;
pub mod sequence;
pub mod timing;
pub mod traits;
pub mod yfx;
pub mod ym_file;
pub mod ysg;

pub use ayfx::AyfxFile;
pub use container::{SfxContainer, SongContainer};
pub use error::{Result, YmError};
pub use player::{YmDataRenderer, YmMixer, YmSfxRenderer, YmSongRenderer};
pub use sequence::{SfxFrame, SfxSequence, YmChannel, YmFrame, YmSequence};
pub use timing::{
    calculate_delay, HzOption, SystemHz, TimingConfig, ATARI_7800_CLOCK, ATARI_ST_CLOCK,
    ZX_SPECTRUM_CLOCK,
};
pub use traits::{SfxFile, SfxInput, SongFile, SongInput};
pub use yfx::YfxFile;
pub use ym_file::YmFile;
pub use ysg::{
    compile_ysg, compile_ysg_optimal, decompile_ysg, GlobalFrame, TrackDescriptor, VoiceFrame,
    YsgFile, YsgHeader, YsgSongDetails, CANDIDATE_PATTERN_FRAMES, SENTINEL_EMPTY_PATTERN,
    YSG_HEADER_SIZE, YSG_MAGIC, YSG_MAX_FILE_SIZE, YSG_VERSION,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_defaults() {
        let hz = SystemHz::Hz50;
        assert_eq!(hz.hz_value(), 50);
        assert!((hz.frame_duration_ms() - 20.0).abs() < f64::EPSILON);

        let hz60 = SystemHz::Hz60;
        assert_eq!(hz60.hz_value(), 60);
    }

    #[test]
    fn test_calculate_delay() {
        // Hand-computed against the same formula as a port-fidelity check:
        // remaining = 1_789_773/hz - 1800, y = floor(remaining/1285), x = round((remaining - y*1285)/5)
        assert_eq!(calculate_delay(50), (26, 117));
        assert_eq!(calculate_delay(60), (21, 209));
    }

    #[test]
    fn test_yfx_compilation_basic() {
        let mut seq = SfxSequence {
            name: "test_sfx".to_string(),
            source_clock: ATARI_ST_CLOCK,
            source_hz: 50,
            priority: 0,
            preferred_channels: None,
            loop_start: None,
            frames: Vec::new(),
        };
        seq.frames.push(SfxFrame {
            tone: Some(450),
            volume: Some(15),
            ..Default::default()
        });

        let payload = YfxFile::from_sequence(&seq).to_bytes();
        assert_eq!(payload.len(), 5);
        assert_eq!(payload[0], 194); // 450 & 0xFF = 194
    }

    #[test]
    fn test_ayfx_csv_parsing() {
        let csv_data = "0,1,0x8a8,0x1f,0xf\n0,1,0x8a8,0x1c,0xe";
        let seq = SfxSequence::from_ayfx_csv("laser", csv_data).unwrap();

        assert_eq!(seq.name, "laser");
        assert_eq!(seq.source_clock, ZX_SPECTRUM_CLOCK);
        assert_eq!(seq.source_hz, 50);
        assert_eq!(seq.frames.len(), 2);

        let frame = &seq.frames[0];
        assert_eq!(frame.tone_enable, Some(false));
        assert_eq!(frame.noise_enable, Some(true));
        assert_eq!(frame.tone, Some(2216)); // 0x8a8 = 2216
        assert_eq!(frame.noise, Some(31)); // 0x1f = 31
        assert_eq!(frame.volume, Some(15)); // 0xf = 15
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
        let bank = SfxSequence::from_ayfx_bank(&bank_bytes).unwrap();
        assert_eq!(bank.len(), 1);
        let seq = &bank[0];
        assert_eq!(seq.name, "wizball_1");
        assert_eq!(seq.frames.len(), 31);
        assert_eq!(seq.frames[0].volume, Some(13));
        assert_eq!(seq.frames[0].tone_enable, Some(true));
        assert_eq!(seq.frames[0].noise_enable, Some(false));
        assert_eq!(seq.frames[0].tone, Some(31));
        assert_eq!(seq.frames[0].noise, Some(0));
    }

    #[test]
    fn test_from_yfx() {
        let source_seq = SfxSequence {
            name: "test_sfx".to_string(),
            source_clock: ATARI_ST_CLOCK,
            source_hz: 50,
            priority: 1,
            preferred_channels: None,
            loop_start: None,
            frames: vec![
                SfxFrame {
                    tone_enable: Some(true),
                    noise_enable: Some(false),
                    tone: Some(100),
                    noise: Some(0),
                    volume: Some(15),
                    duration: Some(1),
                },
                SfxFrame {
                    tone_enable: Some(true),
                    noise_enable: Some(false),
                    tone: Some(102),
                    noise: Some(0),
                    volume: Some(14),
                    duration: Some(1),
                },
            ],
        };

        let payload = YfxFile::from_sequence(&source_seq).to_bytes();

        let decoded = SfxSequence::from_yfx("test_sfx", &payload).unwrap();
        assert_eq!(decoded.frames.len(), 2);
        assert_eq!(decoded.frames[0].tone, Some(100));
        assert_eq!(decoded.frames[0].volume, Some(15));
        assert_eq!(decoded.frames[1].tone, Some(102));
        assert_eq!(decoded.frames[1].volume, Some(14));
    }

    #[test]
    fn test_ayfx_effect_parsing() {
        // Just the effect data slice from pew.afb (after byte 3, length 106 minus name)
        let effect_bytes = vec![
            237, 31, 0, 0, 173, 37, 0, 172, 43, 0, 172, 49, 0, 172, 55, 0, 172, 61, 0, 172, 67, 0,
            172, 73, 0, 172, 79, 0, 172, 85, 0, 172, 91, 0, 172, 97, 0, 172, 103, 0, 172, 109, 0,
            172, 115, 0, 172, 121, 0, 172, 127, 0, 172, 133, 0, 172, 139, 0, 172, 145, 0, 171, 151,
            0, 170, 157, 0, 169, 163, 0, 168, 169, 0, 167, 175, 0, 166, 181, 0, 165, 187, 0, 164,
            193, 0, 163, 199, 0, 162, 205, 0, 161, 211, 0, 208, 32,
        ];
        let seq = SfxSequence::from_ayfx_effect("pew", &effect_bytes).unwrap();
        assert_eq!(seq.name, "pew");
        assert_eq!(seq.frames.len(), 31);
        assert_eq!(seq.frames[0].volume, Some(13));
        assert_eq!(seq.frames[0].tone_enable, Some(true));
        assert_eq!(seq.frames[0].noise_enable, Some(false));
        assert_eq!(seq.frames[0].tone, Some(31));
        assert_eq!(seq.frames[0].noise, Some(0));
    }

    #[test]
    fn test_song_compilation_and_parsing() {
        let mut frames = Vec::new();
        // Create 70 frames to span beyond a 48-frame pattern block
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
            timing: TimingConfig {
                master_clock_hz: 1_789_773,
                frame_rate: SystemHz::Custom(17),
            },
            priority: 0,
            loop_start: None,
            frames,
        };

        let details = compile_ysg(&song, 48).unwrap();
        let ysg_bytes = details.bytes;

        assert_eq!(&ysg_bytes[0..2], b"YS");

        let decoded = YmSequence::from_ysg("test_song", &ysg_bytes).unwrap();
        assert_eq!(decoded.frames[0].tone_a, Some(200));
        assert_eq!(decoded.frames[0].volume_a, Some(15));
        assert_eq!(decoded.frames[69].tone_a, Some(269));
        assert_eq!(decoded.timing.master_clock_hz, 1_789_773);
        assert_eq!(decoded.timing.frame_rate.hz_value(), 17);
    }

    #[test]
    fn test_zero_hz_safety() {
        let (y, _x) = calculate_delay(0);
        assert!(y > 0);

        let hz_custom = SystemHz::Custom(0);
        assert!(hz_custom.frame_duration_ms().is_finite());
    }

    #[test]
    fn test_idle_frames_compression() {
        // Build a song with a long silent section — channel-split wait tokens compress it.
        let mut frames = Vec::new();
        frames.push(YmFrame {
            tone_a: Some(440),
            volume_a: Some(15),
            tone_enable_a: Some(true),
            ..Default::default()
        });
        for _ in 0..50 {
            frames.push(YmFrame::default()); // 50 idle frames
        }
        let song = YmSequence {
            name: "idle_test".to_string(),
            timing: TimingConfig {
                master_clock_hz: ATARI_7800_CLOCK,
                frame_rate: SystemHz::Hz50,
            },
            priority: 0,
            loop_start: None,
            frames,
        };
        let details = compile_ysg(&song, 64).unwrap();
        assert!(details.track_a_bytes < 20);

        // Round-trip: decoded frame count must match
        let decoded = YmSequence::from_ysg("idle_test", &details.bytes).unwrap();
        assert_eq!(decoded.frames.len(), 64);
    }

    #[test]
    fn test_truncated_ysg_returns_err() {
        let truncated_bytes = vec![b'Y', b'S', 1, 0]; // 4 bytes instead of 20
        assert!(YmSequence::from_ysg("bad", &truncated_bytes).is_err());
    }

    #[test]
    fn test_song_renderer() {
        let song = YmSequence {
            name: "test".to_string(),
            timing: TimingConfig::default(),
            priority: 0,
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
        renderer.render_samples(&mut buf, 2);
        assert!(buf.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_song_file_trait_round_trip() {
        let song = YmSequence {
            name: "trait_test".to_string(),
            timing: TimingConfig {
                master_clock_hz: ATARI_7800_CLOCK,
                frame_rate: SystemHz::Hz50,
            },
            priority: 0,
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

        // Compile through YsgFile constructor
        let (ysg_file, details) = YsgFile::from_sequence(&song, 16).unwrap();
        assert_eq!(ysg_file.header.seq_len, 2);
        assert_eq!(ysg_file.header.pattern_frames, 16);

        // Serialize via SongFile trait
        let binary = ysg_file.to_bytes();
        assert_eq!(binary.len(), details.bytes.len());
        assert_eq!(ysg_file.frame_rate_hz(), 50);
        assert_eq!(ysg_file.master_clock_hz(), ATARI_7800_CLOCK);

        // Parse via YsgFile::from_bytes
        let parsed = YsgFile::from_bytes(&binary).unwrap();
        assert_eq!(parsed.header.seq_len, 2);

        // Decompile via SongFile trait
        let restored = parsed.to_sequence("restored").unwrap();
        assert_eq!(restored.frames.len(), 32);
        assert_eq!(restored.frames[0].tone_a, Some(300));
        assert_eq!(restored.frames[0].volume_a, Some(15));
    }

    #[test]
    fn test_ym_file_trait_input() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
        let ym_path =
            std::path::Path::new(&manifest_dir).join("../tests/fixtures/song/ND-Loader.ym");
        if !ym_path.exists() {
            return;
        }

        let bytes = std::fs::read(&ym_path).unwrap();
        let ym_file = YmFile::from_bytes("ND-Loader", &bytes, None).unwrap();

        // Exercise SongInput trait
        assert_eq!(ym_file.title(), "ND-Loader");
        assert_eq!(ym_file.source_hz(), 50);
        assert!(ym_file.source_clock() > 0);
        assert!(!ym_file.raw_frames.is_empty());

        let seq = ym_file.to_sequence(Some(ATARI_7800_CLOCK)).unwrap();
        assert_eq!(seq.frames.len(), ym_file.raw_frames.len());
        assert_eq!(seq.timing.master_clock_hz, ATARI_7800_CLOCK);
    }

    #[test]
    fn test_yfx_file_trait_round_trip() {
        let source_seq = SfxSequence {
            name: "test_sfx".to_string(),
            source_clock: ZX_SPECTRUM_CLOCK,
            source_hz: 50,
            priority: 1,
            preferred_channels: None,
            loop_start: None,
            frames: vec![
                SfxFrame {
                    tone_enable: Some(true),
                    noise_enable: Some(false),
                    tone: Some(250),
                    noise: Some(0),
                    volume: Some(15),
                    duration: Some(1),
                },
                SfxFrame {
                    tone_enable: Some(true),
                    noise_enable: Some(false),
                    tone: Some(260),
                    noise: Some(0),
                    volume: Some(12),
                    duration: Some(2),
                },
            ],
        };

        // Exercise SfxFile and SfxInput on YfxFile
        let yfx = YfxFile::from_sequence(&source_seq);
        let raw_bytes = yfx.to_bytes();
        assert_eq!(raw_bytes.len(), 10);
        assert_eq!(yfx.frame_count(), 2);

        let parsed_yfx = YfxFile::from_bytes(&raw_bytes).unwrap();
        let decoded_seq = parsed_yfx.to_sequence("test_sfx").unwrap();
        assert_eq!(decoded_seq.frames.len(), 2);
        assert_eq!(decoded_seq.frames[0].tone, Some(250));
        assert_eq!(decoded_seq.frames[0].volume, Some(15));
        assert_eq!(decoded_seq.frames[1].tone, Some(260));
        assert_eq!(decoded_seq.frames[1].volume, Some(12));
        assert_eq!(decoded_seq.frames[1].duration, Some(2));

        // SfxInput trait test
        let seqs = parsed_yfx.to_sequences().unwrap();
        assert_eq!(seqs.len(), 1);
        assert_eq!(seqs[0].frames.len(), 2);
    }

    #[test]
    fn test_ayfx_file_trait_input() {
        let csv_data = "0,1,0x8a8,0x1f,0xf\n0,1,0x8a8,0x1c,0xe";
        let ayfx = AyfxFile::from_csv("laser", csv_data).unwrap();

        // Exercise SfxInput trait
        let seqs = ayfx.to_sequences().unwrap();
        assert_eq!(seqs.len(), 1);
        assert_eq!(seqs[0].name, "laser");
        assert_eq!(seqs[0].frames.len(), 2);
        assert_eq!(seqs[0].frames[0].tone, Some(2216));
    }

    #[test]
    fn test_try_from_traits() {
        use std::convert::TryFrom;

        // Valid YfxFile TryFrom
        let raw_yfx = vec![0x12, 0x34, 0x0F, 0x01, 0x01];
        let yfx = YfxFile::try_from(raw_yfx.as_slice()).unwrap();
        assert_eq!(yfx.bytes.len(), 5);

        // Invalid YfxFile length
        assert!(YfxFile::try_from([0u8; 4].as_slice()).is_err());

        // Invalid YsgHeader length
        assert!(YsgHeader::try_from([0u8; 10].as_slice()).is_err());

        // Invalid YsgFile length
        assert!(YsgFile::try_from([0u8; 10].as_slice()).is_err());
    }

    #[test]
    fn test_ym_error_display() {
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
    }

    #[test]
    fn test_containers() {
        // SfxContainer CSV
        let csv_data = b"0,1,0x8a8,0x1f,0xf\n0,1,0x8a8,0x1c,0xe";
        let container = SfxContainer::from_bytes("laser", "csv", csv_data).unwrap();
        let seqs = container.to_sequences().unwrap();
        assert_eq!(seqs.len(), 1);
        assert_eq!(seqs[0].name, "laser");

        // SfxContainer YFX
        let raw_yfx = vec![0x12, 0x34, 0x0F, 0x01, 0x01];
        let container_yfx = SfxContainer::from_bytes("test_sfx", "yfx", &raw_yfx).unwrap();
        let seqs_yfx = container_yfx.to_sequences().unwrap();
        assert_eq!(seqs_yfx.len(), 1);

        // SfxContainer Unsupported
        assert!(SfxContainer::from_bytes("unknown", "wav", b"riff").is_err());

        // SongContainer JSON
        let song = YmSequence {
            name: "test_json".to_string(),
            timing: TimingConfig::default(),
            priority: 0,
            loop_start: None,
            frames: vec![YmFrame::default()],
        };
        let song_json = serde_json::to_vec(&song).unwrap();
        let container_song =
            SongContainer::from_bytes("test_json", "json", &song_json, None).unwrap();
        assert_eq!(container_song.title(), "test_json");
        let decoded = container_song.to_sequence("test_json", None).unwrap();
        assert_eq!(decoded.frames.len(), 1);
    }
}
