use console::{style, Key, Term};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use ym_core::{
    SfxSequence, YmChannel, YmDataRenderer, YmMixer, YmSequence, YmSfxRenderer, YmSongRenderer,
};

/// Configuration options for host audio output settings.
#[derive(Debug, Clone, Default)]
pub struct AudioConfig {
    /// Optional sample rate override in Hz (e.g. 44100, 48000).
    /// If `None`, uses the output device's default sample rate.
    pub sample_rate: Option<u32>,
    /// Optional audio output device name filter.
    /// If `None`, uses the host system default output audio device.
    pub device_name: Option<String>,
}

/// Helper container for opening and managing cpal audio output settings.
struct AudioOutputSession {
    device: cpal::Device,
    sample_rate: u32,
    channels: usize,
    sample_format: cpal::SampleFormat,
    stream_config: cpal::StreamConfig,
}

impl AudioOutputSession {
    fn open(config: &AudioConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = if let Some(ref name) = config.device_name {
            host.output_devices()?
                .find(|d| d.to_string().to_lowercase().contains(&name.to_lowercase()))
                .ok_or_else(|| format!("Audio device matching '{name}' not found"))?
        } else {
            host.default_output_device()
                .ok_or("No default output audio device found")?
        };
        let default_config = device.default_output_config()?;
        let sample_rate_u32 = config
            .sample_rate
            .unwrap_or_else(|| default_config.sample_rate());
        let channels = default_config.channels() as usize;
        let sample_format = default_config.sample_format();
        let mut stream_config: cpal::StreamConfig = default_config.into();
        stream_config.sample_rate = sample_rate_u32;

        Ok(Self {
            device,
            sample_rate: sample_rate_u32,
            channels,
            sample_format,
            stream_config,
        })
    }
}

/// Renderer engines that can be pulled for interleaved PCM samples on demand.
trait SampleSource {
    fn render_samples(&mut self, data: &mut [f32], channels: usize);
}

impl SampleSource for YmSongRenderer {
    fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        self.render_samples(data, channels);
    }
}

impl SampleSource for YmSfxRenderer {
    fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        self.render_samples(data, channels);
    }
}

impl SampleSource for YmMixer {
    fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        self.render_samples(data, channels);
    }
}

impl SampleSource for YmDataRenderer {
    fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        self.render_samples(data, channels);
    }
}

pub struct AudioPlayer;

impl AudioPlayer {
    /// Spawns a background thread reading raw key presses and forwarding them over a channel.
    #[must_use]
    pub fn spawn_key_listener() -> mpsc::Receiver<Key> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let term = Term::stdout();
            while let Ok(key) = term.read_key() {
                if tx.send(key).is_err() {
                    break;
                }
            }
        });
        rx
    }

    /// Prints the keyboard trigger for each of the first 10 loaded SFX.
    pub fn print_sfx_keybindings(sfx_list: &[SfxSequence]) {
        for (idx, sfx_item) in sfx_list.iter().enumerate().take(10) {
            let key_label = match idx {
                0 => "1 or SPACEBAR".to_string(),
                1..=8 => format!("{}", idx + 1),
                _ => "0".to_string(),
            };
            println!(
                "  [{}] Key {}: {} ({} frames)",
                style(idx).dim(),
                style(key_label).yellow().bold(),
                style(&sfx_item.name).cyan(),
                sfx_item.frames.len()
            );
        }
    }

    fn frame_progress_bar(total_frames: u64) -> ProgressBar {
        let pb = ProgressBar::new(total_frames);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] frame {pos}/{len}{msg}",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        pb
    }

    fn time_progress_bar(total_deciseconds: u64) -> ProgressBar {
        let pb = ProgressBar::new(total_deciseconds);
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        pb
    }

    fn key_to_sfx_index(key: &Key) -> Option<usize> {
        match key {
            Key::Char(' ' | '1') => Some(0),
            Key::Char('2') => Some(1),
            Key::Char('3') => Some(2),
            Key::Char('4') => Some(3),
            Key::Char('5') => Some(4),
            Key::Char('6') => Some(5),
            Key::Char('7') => Some(6),
            Key::Char('8') => Some(7),
            Key::Char('9') => Some(8),
            Key::Char('0') => Some(9),
            _ => None,
        }
    }

    /// Builds and starts an f32 cpal output stream that pulls samples from `renderer`
    /// on each callback, under a lock shared with the caller (for seeking, SFX triggers, etc).
    fn build_f32_stream<R: SampleSource + Send + 'static>(
        audio: &AudioOutputSession,
        renderer: Arc<Mutex<R>>,
    ) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
        let channels = audio.channels;
        let err_fn = |err| eprintln!("{} {}", style("Audio stream error:").red().bold(), err);
        match audio.sample_format {
            cpal::SampleFormat::F32 => Ok(audio.device.build_output_stream(
                audio.stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut r = renderer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    r.render_samples(data, channels);
                },
                err_fn,
                None,
            )?),
            _ => Err("Unsupported audio sample format".into()),
        }
    }
    /// Auditions a song sequence over system audio.
    ///
    /// # Errors
    ///
    /// Returns an error if initializing the host audio device or stream fails.
    pub fn play_song(
        sequence: &YmSequence,
        config: &AudioConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if sequence.frames.is_empty() {
            println!("{}", style("Sequence contains no frames to play.").yellow());
            return Ok(());
        }

        let audio = AudioOutputSession::open(config)?;
        let renderer = Arc::new(Mutex::new(YmSongRenderer::new(sequence, audio.sample_rate)));

        let stream = Self::build_f32_stream(&audio, Arc::clone(&renderer))?;
        stream.play()?;

        let total_frames = sequence.frames.len();
        let hz = sequence.timing.frame_rate.hz_value();
        println!(
            "{} '{}' ({} frames @ {} Hz)",
            style("PLAYING SONG:").bold().green(),
            sequence.name,
            total_frames,
            hz
        );

        let interactive = Term::stdout().is_term();
        let pb = Self::frame_progress_bar(total_frames as u64);
        pb.set_message(format!(
            " {}",
            style("(\u{2190}/\u{2192} to seek, 'q' to quit)").yellow()
        ));

        let key_rx = interactive.then(Self::spawn_key_listener);
        let seek_step_frames = ((hz as usize) * 5).max(1);

        loop {
            if let Some(rx) = &key_rx {
                while let Ok(key) = rx.try_recv() {
                    match key {
                        Key::ArrowRight => {
                            let mut r = renderer
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let target = r.current_frame().saturating_add(seek_step_frames);
                            r.seek(target);
                        }
                        Key::ArrowLeft => {
                            let mut r = renderer
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let target = r.current_frame().saturating_sub(seek_step_frames);
                            r.seek(target);
                        }
                        Key::Char('q' | 'Q') => {
                            let mut r = renderer
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            r.set_finished(true);
                        }
                        _ => {}
                    }
                }
            }

            let (current, is_done) = {
                let r = renderer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (r.current_frame(), r.is_finished())
            };

            pb.set_position(current.min(total_frames) as u64);
            if is_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        pb.finish_and_clear();

        if key_rx.is_some() {
            let _ = std::process::Command::new("stty").arg("sane").status();
        }
        if sequence.loop_start.is_none() {
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    /// Auditions a sound effect sequence over system audio.
    ///
    /// # Errors
    ///
    /// Returns an error if initializing the host audio device or stream fails.
    pub fn play_sfx(
        sequence: &SfxSequence,
        config: &AudioConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if sequence.frames.is_empty() {
            println!("{}", style("Sequence contains no frames to play.").yellow());
            return Ok(());
        }

        let audio = AudioOutputSession::open(config)?;
        let renderer = Arc::new(Mutex::new(YmSfxRenderer::new(sequence, audio.sample_rate)));

        let stream = Self::build_f32_stream(&audio, Arc::clone(&renderer))?;
        stream.play()?;

        let total_frames = sequence.frames.len();
        let hz = sequence.source_hz;
        let channel = sequence
            .preferred_channels
            .as_ref()
            .and_then(|c| c.first().copied())
            .unwrap_or(YmChannel::A);

        println!(
            "{} '{}' ({} frames @ {} Hz on channel {:?})",
            style("PLAYING SOUND EFFECT:").bold().green(),
            sequence.name,
            total_frames,
            hz,
            channel
        );

        let interactive = Term::stdout().is_term();
        let pb = Self::frame_progress_bar(total_frames as u64);
        pb.set_message(format!(" {}", style("('q' to quit)").yellow()));

        let key_rx = interactive.then(Self::spawn_key_listener);

        loop {
            if let Some(rx) = &key_rx {
                while let Ok(key) = rx.try_recv() {
                    if matches!(key, Key::Char('q' | 'Q')) {
                        let mut r = renderer
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        r.set_finished(true);
                    }
                }
            }

            let (current, is_done) = {
                let r = renderer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (r.current_frame(), r.is_finished())
            };

            pb.set_position(current.min(total_frames) as u64);
            if is_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        pb.finish_and_clear();

        if key_rx.is_some() {
            let _ = std::process::Command::new("stty").arg("sane").status();
        }
        std::thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    /// Auditions an interactive song and sound effect mix over system audio.
    ///
    /// # Errors
    ///
    /// Returns an error if initializing the host audio device or stream fails.
    pub fn play_mix(
        song_seq: &YmSequence,
        sfx_list: &[SfxSequence],
        preferred_channel: YmChannel,
        config: &AudioConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::print_sfx_keybindings(sfx_list);

        let audio = AudioOutputSession::open(config)?;
        let mixer = Arc::new(Mutex::new(YmMixer::new(
            song_seq,
            sfx_list,
            preferred_channel,
            audio.sample_rate,
        )));

        let stream = Self::build_f32_stream(&audio, Arc::clone(&mixer))?;
        stream.play()?;

        let total_song_frames = song_seq.frames.len();
        let song_hz = song_seq.timing.frame_rate.hz_value();

        let pb = ProgressBar::new(total_song_frames as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] frame {pos}/{len} {msg}",
            )?
            .progress_chars("=>-"),
        );

        pb.set_message(format!(
            " Press \u{2190}/\u{2192} to seek, 1-9/0/SPACE to trigger SFX, 'q' to quit (Primary Ch: {preferred_channel:?})"
        ));

        let key_rx = Self::spawn_key_listener();
        let seek_step_frames = ((song_hz as usize) * 5).max(1);

        loop {
            while let Ok(key) = key_rx.try_recv() {
                if matches!(key, Key::Char('q' | 'Q')) {
                    let mut m = mixer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    m.set_finished(true);
                    break;
                }

                if matches!(key, Key::ArrowRight | Key::ArrowLeft) {
                    let mut m = mixer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let current = m.current_song_frame();
                    let target = match key {
                        Key::ArrowRight => current
                            .saturating_add(seek_step_frames)
                            .min(total_song_frames.saturating_sub(1)),
                        Key::ArrowLeft => current.saturating_sub(seek_step_frames),
                        _ => current,
                    };
                    m.seek_song(target);
                }

                if let Some(sfx_idx) = Self::key_to_sfx_index(&key) {
                    let mut m = mixer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    m.trigger_sfx(sfx_idx);
                }
            }

            let (current, is_done) = {
                let m = mixer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (m.current_song_frame(), m.is_finished())
            };

            pb.set_position(current.min(total_song_frames) as u64);
            if is_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }

        pb.finish_with_message("Playback finished.");
        let _ = std::process::Command::new("stty").arg("sane").status();
        Ok(())
    }

    /// Auditions raw YM chiptune data over system audio.
    ///
    /// # Errors
    ///
    /// Returns an error if decompressing or initializing host audio fails.
    pub fn play_ym_data(
        ym_data: &[u8],
        config: &AudioConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let audio = AudioOutputSession::open(config)?;
        let renderer = Arc::new(Mutex::new(YmDataRenderer::new(ym_data, audio.sample_rate)?));

        let duration = renderer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .duration_seconds();

        let stream = Self::build_f32_stream(&audio, Arc::clone(&renderer))?;
        stream.play()?;

        println!("{} {:.1}s", style("PLAYING SONG:").bold().green(), duration);

        let total_deciseconds = (duration * 10.0).round().max(1.0) as u64;
        let pb = Self::time_progress_bar(total_deciseconds);

        let interactive = Term::stdout().is_term();
        let key_rx = interactive.then(Self::spawn_key_listener);

        let start = std::time::Instant::now();
        while start.elapsed().as_secs_f64() < duration {
            if let Some(rx) = &key_rx {
                while let Ok(key) = rx.try_recv() {
                    if matches!(key, Key::Char('q' | 'Q')) {
                        pb.finish_and_clear();
                        let _ = std::process::Command::new("stty").arg("sane").status();
                        return Ok(());
                    }
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            pb.set_position(((elapsed * 10.0).round() as u64).min(total_deciseconds));
            pb.set_message(format!("{elapsed:.1}s / {duration:.1}s ('q' to quit)"));
            std::thread::sleep(Duration::from_millis(100));
        }
        pb.finish_and_clear();
        if key_rx.is_some() {
            let _ = std::process::Command::new("stty").arg("sane").status();
        }
        Ok(())
    }
}
