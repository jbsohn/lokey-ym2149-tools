pub mod delta;
pub mod player;
pub mod sequence;
pub mod timing;

pub use delta::{CompilerOptions, CompressionLevel, DeltaCompiler, YmSongDetails, RLE_FLAG};
pub use player::{spawn_key_listener, AudioPlayer};
pub use sequence::{SfxFrame, SfxSequence, YmChannel, YmFrame, YmSequence};
pub use timing::{
    calculate_delay, HzOption, SystemHz, TimingConfig, ATARI_7800_CLOCK, ATARI_ST_CLOCK,
    ZX_SPECTRUM_CLOCK,
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
    fn test_delta_compiler_basic() {
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

        let compiler = DeltaCompiler::new();
        let payload = compiler.compile_sfx(&seq);
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

        let compiler = DeltaCompiler::new();
        let payload = compiler.compile_sfx(&source_seq);

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
        // Create 70 frames to span beyond a 64-frame pattern block
        for i in 0u16..70u16 {
            frames.push(YmFrame {
                tone_a: Some(200 + i),
                volume_a: Some(15),
                tone_enable_a: Some(true),
                ..Default::default()
            });
        }
        // Deliberately non-default timing (as `--step` decimation or `.ym` import would
        // produce) to catch the format silently dropping it back to hardcoded defaults.
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

        let compiler = DeltaCompiler::new();
        let details = compiler
            .compile_song(&song, CompressionLevel::Full, &CompilerOptions::default())
            .unwrap();
        let ysg_bytes = details.bytes;

        let chosen_size = details.pattern_size;
        assert_eq!(ysg_bytes[0] as usize, chosen_size);
        let seq_len = ysg_bytes[2] as usize;

        let decoded = YmSequence::from_ysg("test_song", &ysg_bytes).unwrap();
        // Should be padded to a multiple of the chosen pattern size
        assert_eq!(decoded.frames.len(), chosen_size * seq_len);
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
    fn test_rle_reduces_idle_frames() {
        // Build a song with a long silent section — should shrink with RLE enabled.
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
            name: "rle_test".to_string(),
            timing: TimingConfig {
                master_clock_hz: ATARI_7800_CLOCK,
                frame_rate: SystemHz::Hz50,
            },
            priority: 0,
            loop_start: None,
            frames,
        };
        let compiler = DeltaCompiler::new();
        let with_rle = compiler
            .compile_song(
                &song,
                CompressionLevel::Full,
                &CompilerOptions {
                    rle: true,
                    ..CompilerOptions::default()
                },
            )
            .unwrap();
        let without_rle = compiler
            .compile_song(
                &song,
                CompressionLevel::Full,
                &CompilerOptions {
                    rle: false,
                    ..CompilerOptions::default()
                },
            )
            .unwrap();
        assert!(
            with_rle.bytes.len() < without_rle.bytes.len(),
            "RLE should reduce size for idle-heavy songs"
        );

        // Round-trip: decoded frame count must match
        let decoded = YmSequence::from_ysg("rle_test", &with_rle.bytes).unwrap();
        assert_eq!(
            decoded.frames.len(),
            song.frames.len().next_multiple_of(with_rle.pattern_size)
        );
    }

    #[test]
    fn test_truncated_ysg_returns_err() {
        let truncated_bytes = vec![64, 2, 5, 0]; // 4 bytes instead of >=12
        assert!(YmSequence::from_ysg("bad", &truncated_bytes).is_err());
    }
}
