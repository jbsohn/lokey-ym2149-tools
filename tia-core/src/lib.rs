//! # `tia-core` - Atari TIA Sound Chip Emulation & Toolchain
//!
//! Cycle-accurate emulation, sequence authoring, delta compression, and real-time audio
//! synthesis for the Atari 2600 / 7800 Television Interface Adapter (TIA) sound hardware.
//!
//! ## Acknowledgements & Attribution
//!
//! The TIA polynomial generators, waveform definitions, and synthesis models in this crate
//! were adapted from the open-source **`TIASound`** JavaScript emulator:
//! - **Original Author**: Fabio Cardoso ([`@fabiopiratininga`](https://github.com/fabiopiratininga))
//! - **Original Repository**: <https://github.com/fabiopiratininga/TIASound>
//! - **License**: MIT License - Copyright (c) 2025 Fabio Cardoso

pub mod chip;
pub mod delta;
pub mod player;
pub mod sequence;
pub mod timing;

pub use chip::{TiaChannel, TiaChannelState, TiaChip, TiaSoundType, DIVISORS, POLYS};
pub use delta::{CompilerOptions, CompressionLevel, DeltaCompiler, TiaSongDetails, RLE_FLAG};
pub use player::{TiaMixer, TiaSfxRenderer, TiaSongRenderer};
pub use sequence::{TiaFrame, TiaSequence, TiaSfxFrame, TiaSfxSequence};
pub use timing::{
    calculate_delay, HzOption, SystemHz, TimingConfig, ATARI_2600_NTSC_CLOCK, ATARI_7800_CLOCK,
    TIA_NTSC_AUDIO_CLOCK, TIA_PAL_AUDIO_CLOCK,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_defaults() {
        let hz60 = SystemHz::Hz60;
        assert_eq!(hz60.hz_value(), 60);
        assert!((hz60.frame_duration_ms() - (1000.0 / 60.0)).abs() < 1e-6);

        let hz50 = SystemHz::Hz50;
        assert_eq!(hz50.hz_value(), 50);
        assert!((hz50.frame_duration_ms() - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_sound_types() {
        assert_eq!(TiaSoundType::from_name("saw"), TiaSoundType::Saw);
        assert_eq!(TiaSoundType::from_name("engine"), TiaSoundType::Engine);
        assert_eq!(TiaSoundType::from_name("square"), TiaSoundType::Square);
        assert_eq!(TiaSoundType::from_name("bass"), TiaSoundType::Bass);
        assert_eq!(TiaSoundType::from_name("pitfall"), TiaSoundType::Pitfall);
        assert_eq!(TiaSoundType::from_name("noise"), TiaSoundType::Noise);
        assert_eq!(TiaSoundType::from_name("lead"), TiaSoundType::Lead);
        assert_eq!(TiaSoundType::from_name("buzz"), TiaSoundType::Buzz);

        assert_eq!(TiaSoundType::Saw.audc_value(), 1);
        assert_eq!(TiaSoundType::Square.audc_value(), 4);
        assert_eq!(TiaSoundType::Noise.audc_value(), 8);
    }

    #[test]
    fn test_chip_emulation_and_registers() {
        let mut chip = TiaChip::new(TIA_NTSC_AUDIO_CLOCK, 48_000);
        chip.set_channel_by_name(TiaChannel::Ch0, 15, "square", 8);
        chip.set_channel_by_name(TiaChannel::Ch1, 8, "bass", 6);

        assert_eq!(chip.channels[0].audf, 15);
        assert_eq!(chip.channels[0].audc, 4); // square = 4
        assert_eq!(chip.channels[0].audv, 8);

        assert_eq!(chip.channels[1].audf, 8);
        assert_eq!(chip.channels[1].audc, 6); // bass = 6
        assert_eq!(chip.channels[1].audv, 6);

        let mut buffer = [0.0f32; 128];
        chip.render_samples(&mut buffer, 1);
        // Ensure samples were generated and within expected bounds [0.0, 1.0]
        let max_val = buffer.iter().fold(0.0f32, |m, &s| m.max(s));
        assert!(max_val > 0.0);
        assert!(max_val <= 1.0);
    }

    #[test]
    fn test_sfx_sequence_csv_and_binary() {
        let csv = "15,4,8\n12,4,6\n10,8,4\n";
        let seq = TiaSfxSequence::from_csv("laser", csv).unwrap();
        assert_eq!(seq.name, "laser");
        assert_eq!(seq.frames.len(), 3);
        assert_eq!(seq.frames[0].audf, Some(15));
        assert_eq!(seq.frames[0].audc, Some(4));
        assert_eq!(seq.frames[0].audv, Some(8));

        let tfx_bytes = seq.to_tfx();
        assert_eq!(tfx_bytes.len(), 9); // 3 frames * 3 bytes

        let decoded = TiaSfxSequence::from_tfx("laser_dec", &tfx_bytes).unwrap();
        assert_eq!(decoded.frames.len(), 3);
        assert_eq!(decoded.frames[0].audf, Some(15));
        assert_eq!(decoded.frames[0].audc, Some(4));
        assert_eq!(decoded.frames[0].audv, Some(8));
    }

    #[test]
    fn test_song_sequence_json_and_compiler() {
        let mut frames = Vec::new();
        for i in 0..32 {
            frames.push(TiaFrame {
                audf0: Some((i % 32) as u8),
                audc0: Some(4),
                audv0: Some(8),
                audf1: Some(((i + 4) % 32) as u8),
                audc1: Some(6),
                audv1: Some(6),
                duration: Some(1),
            });
        }

        let seq = TiaSequence {
            name: "test_tune".to_string(),
            timing: TimingConfig::default(),
            priority: 0,
            loop_start: Some(0),
            frames,
        };

        // Test JSON roundtrip
        let json = seq.to_json().unwrap();
        let from_json = TiaSequence::from_json(&json).unwrap();
        assert_eq!(from_json.frames.len(), 32);

        // Test Delta compiler
        let compiler = DeltaCompiler::new();
        let details = compiler
            .compile_song(&seq, CompressionLevel::Full, &CompilerOptions::default())
            .unwrap();

        assert!(!details.bytes.is_empty());
        assert!(details.compiled_bytes > 0);

        // Test TSG binary decoding
        let from_tsg = TiaSequence::from_tsg("test_tune", &details.bytes).unwrap();
        assert_eq!(from_tsg.frames.len(), 32);
    }

    #[test]
    fn test_song_renderer_and_seeking() {
        let mut frames = Vec::new();
        for i in 0..10 {
            frames.push(TiaFrame {
                audf0: Some(i as u8),
                audc0: Some(4),
                audv0: Some(10),
                audf1: None,
                audc1: None,
                audv1: None,
                duration: Some(1),
            });
        }

        let seq = TiaSequence {
            name: "short".to_string(),
            timing: TimingConfig::default(),
            priority: 0,
            loop_start: None,
            frames,
        };

        let mut renderer = TiaSongRenderer::new(&seq, 48_000);
        let mut buffer = [0.0f32; 256];
        renderer.render_samples(&mut buffer, 1);
        assert!(!renderer.is_finished());

        renderer.seek(5);
        assert_eq!(renderer.current_frame(), 5);
    }

    #[test]
    fn test_mixer_sfx_triggering() {
        let song_frames = vec![
            TiaFrame {
                audf0: Some(10),
                audc0: Some(4),
                audv0: Some(8),
                audf1: Some(20),
                audc1: Some(6),
                audv1: Some(6),
                duration: Some(1),
            };
            20
        ];

        let song = TiaSequence {
            name: "bgm".to_string(),
            timing: TimingConfig::default(),
            priority: 0,
            loop_start: Some(0),
            frames: song_frames,
        };

        let sfx_frames = vec![
            TiaSfxFrame {
                audf: Some(5),
                audc: Some(8),
                audv: Some(15),
                duration: Some(1),
            };
            5
        ];

        let sfx = TiaSfxSequence {
            name: "hit".to_string(),
            source_clock: TIA_NTSC_AUDIO_CLOCK,
            source_hz: 60,
            priority: 0,
            preferred_channel: Some(TiaChannel::Ch0),
            loop_start: None,
            frames: sfx_frames,
        };

        let mut mixer = TiaMixer::new(&song, &[sfx], TiaChannel::Ch0, 48_000);
        mixer.trigger_sfx(0);
        assert!(mixer.is_sfx_active(0));

        let mut buffer = [0.0f32; 256];
        mixer.render_samples(&mut buffer, 1);
        assert!(!mixer.is_song_finished());
    }

    #[test]
    fn test_playback_sound() {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        // Build a 60-frame (1 second) authentic TIA musical tune with melody and bass
        let mut frames = Vec::new();
        // Notes on AUDF: 0..31 (lower values = higher pitch)
        let melody = [18, 16, 14, 12, 10, 8, 6, 4];
        let bass = [28, 24, 20, 16];

        for i in 0..60 {
            let mel_note = melody[(i / 7) % melody.len()];
            let bass_note = bass[(i / 15) % bass.len()];
            let vol = 12u8.saturating_sub((i % 7) as u8); // decay envelope

            frames.push(TiaFrame {
                audf0: Some(mel_note),
                audc0: Some(4), // Square wave
                audv0: Some(vol),
                audf1: Some(bass_note),
                audc1: Some(6), // Bass tone
                audv1: Some(8),
                duration: Some(1),
            });
        }

        let seq = TiaSequence {
            name: "tia_demo".to_string(),
            timing: TimingConfig::default(),
            priority: 0,
            loop_start: Some(0),
            frames,
        };

        // Try playing via host audio device if available
        let host = cpal::default_host();
        if let Some(device) = host.default_output_device() {
            if let Ok(default_config) = device.default_output_config() {
                let sample_rate = default_config.sample_rate();
                let channels = default_config.channels() as usize;
                let sample_format = default_config.sample_format();
                let stream_config: cpal::StreamConfig = default_config.into();

                let renderer = Arc::new(Mutex::new(TiaSongRenderer::new(&seq, sample_rate)));
                let r_clone = Arc::clone(&renderer);

                let stream_res = match sample_format {
                    cpal::SampleFormat::F32 => device
                        .build_output_stream(
                            stream_config,
                            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                                if let Ok(mut r) = r_clone.lock() {
                                    r.render_samples(data, channels);
                                }
                            },
                            |err| eprintln!("Audio stream error: {err}"),
                            None,
                        )
                        .ok(),
                    cpal::SampleFormat::I16 => device
                        .build_output_stream(
                            stream_config,
                            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                                if let Ok(mut r) = r_clone.lock() {
                                    let mut f32_buf = vec![0.0f32; data.len()];
                                    r.render_samples(&mut f32_buf, channels);
                                    for (out, &s) in data.iter_mut().zip(&f32_buf) {
                                        *out = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                                    }
                                }
                            },
                            |err| eprintln!("Audio stream error: {err}"),
                            None,
                        )
                        .ok(),
                    cpal::SampleFormat::U16 => device
                        .build_output_stream(
                            stream_config,
                            move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                                if let Ok(mut r) = r_clone.lock() {
                                    let mut f32_buf = vec![0.0f32; data.len()];
                                    r.render_samples(&mut f32_buf, channels);
                                    for (out, &s) in data.iter_mut().zip(&f32_buf) {
                                        let normalized =
                                            (s.clamp(-1.0, 1.0) * 0.5 + 0.5) * f32::from(u16::MAX);
                                        *out = normalized as u16;
                                    }
                                }
                            },
                            |err| eprintln!("Audio stream error: {err}"),
                            None,
                        )
                        .ok(),
                    _ => None,
                };

                if let Some(stream) = stream_res {
                    if stream.play().is_ok() {
                        println!("Playing TIA sound through audio output ({sample_rate} Hz, {channels} ch)...");
                        std::thread::sleep(Duration::from_millis(800));
                        let _ = stream.pause();
                    }
                }
            }
        }

        // Also assert that offline PCM rendering produces non-zero audio samples
        let mut offline_renderer = TiaSongRenderer::new(&seq, 48_000);
        let mut test_buf = vec![0.0f32; 4800]; // 0.1s at 48kHz
        offline_renderer.render_samples(&mut test_buf, 1);
        let max_amp = test_buf.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(max_amp > 0.0, "Rendered samples should be non-zero");
    }

    #[test]
    #[ignore = "Interactive sound audition tour across all 8 TIA waveforms"]
    fn test_all_8_sounds_tour() {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let sounds: &[(&str, u8, u8, u8)] = &[
            ("saw (AUDC 1)", 1, 15, 8),
            ("engine (AUDC 3)", 3, 24, 10),
            ("square (AUDC 4)", 4, 12, 8),
            ("bass (AUDC 6)", 6, 20, 10),
            ("pitfall (AUDC 7)", 7, 10, 9),
            ("noise (AUDC 8)", 8, 15, 9),
            ("lead (AUDC 12)", 12, 10, 8),
            ("buzz (AUDC 15)", 15, 16, 8),
        ];

        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            println!("No output audio device found.");
            return;
        };

        let Ok(default_config) = device.default_output_config() else {
            return;
        };
        let sample_rate = default_config.sample_rate();
        let channels = default_config.channels() as usize;
        let stream_config: cpal::StreamConfig = default_config.into();

        for &(name, audc, audf, audv) in sounds {
            println!("Auditioning TIA sound: {name} [AUDF: {audf}, AUDC: {audc}, AUDV: {audv}]");

            let mut chip = TiaChip::new(TIA_NTSC_AUDIO_CLOCK, sample_rate);
            chip.set_channel(TiaChannel::Ch0, audf, audc, audv);
            let chip_arc = Arc::new(Mutex::new(chip));
            let chip_clone = Arc::clone(&chip_arc);

            let stream = device.build_output_stream(
                stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if let Ok(mut c) = chip_clone.lock() {
                        c.render_samples(data, channels);
                    }
                },
                |err| eprintln!("Audio error: {err}"),
                None,
            );

            if let Ok(stream) = stream {
                let _ = stream.play();
                std::thread::sleep(Duration::from_secs(1));
                let _ = stream.pause();
            }
        }
    }
}
