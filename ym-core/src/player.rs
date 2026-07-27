use crate::sequence::{SfxFrame, SfxSequence, YmChannel, YmFrame, YmSequence};
use console::style;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ym2149::{Ym2149, Ym2149Backend};

pub struct AudioPlayer;

/// Shared progress counters updated by audio thread and read lock-freely by UI thread.
#[derive(Clone)]
pub struct PlaybackProgress {
    current_frame: Arc<AtomicUsize>,
    finished: Arc<AtomicBool>,
}

/// Progress bar styled for a known frame count (sfx/song playback from `.yfx`/`.ysg`).
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

/// Progress bar styled for elapsed-time playback (raw `.ym` chiptune data, where no
/// frame count is exposed by the replayer crate).
fn time_progress_bar(total_deciseconds: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_deciseconds);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb
}

/// All state touched by the audio callback, behind a single lock so each
/// callback invocation takes one mutex instead of one per field.
struct PlaybackState {
    chip: Ym2149,
    frame_idx: usize,
    sample_in_frame: usize,
    mixer: u8,
    last_env_shape: Option<u8>,
    finished: bool,
}

/// cpal output device/stream parameters, bundled so `build_stream` doesn't need
/// a separate argument for each one.
struct AudioSink<'a> {
    device: &'a cpal::Device,
    stream_config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    channels: usize,
}

/// Helper container for opening and managing cpal audio output settings.
struct AudioOutputSession {
    device: cpal::Device,
    sample_rate: cpal::SampleRate,
    channels: usize,
    sample_format: cpal::SampleFormat,
    stream_config: cpal::StreamConfig,
}

impl AudioOutputSession {
    /// Opens the system default audio output device and captures its configuration.
    fn open_default() -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No default output audio device found")?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate();
        let channels = config.channels() as usize;
        let sample_format = config.sample_format();
        let stream_config = config.into();

        Ok(Self {
            device,
            sample_rate,
            channels,
            sample_format,
            stream_config,
        })
    }

    /// Borrows this session's device and config as an [`AudioSink`] for passing to `build_stream`.
    fn as_sink(&self) -> AudioSink<'_> {
        AudioSink {
            device: &self.device,
            stream_config: self.stream_config,
            sample_format: self.sample_format,
            channels: self.channels,
        }
    }
}

type StreamResult = Result<(cpal::Stream, PlaybackProgress), Box<dyn std::error::Error>>;

impl AudioPlayer {
    /// Builds and starts a cpal output stream that clocks `chip` and feeds it one
    /// frame of `F` at a time via `apply`, advancing every `samples_per_frame`
    /// samples and looping back to `loop_start` (or finishing) at the end.
    fn build_stream<F, Apply>(
        sink: AudioSink,
        samples_per_frame: usize,
        mut chip: Ym2149,
        frames: Arc<[F]>,
        loop_start: Option<usize>,
        mut apply: Apply,
    ) -> StreamResult
    where
        F: Send + Sync + 'static,
        Apply: FnMut(&F, &mut Ym2149, &mut u8, &mut Option<u8>) + Send + 'static,
    {
        // Apply initial frame registers before the stream (and its callback) exist.
        let mut mixer = 0x3F; // Default: all tones & noise muted
        let mut last_env_shape = None;
        apply(&frames[0], &mut chip, &mut mixer, &mut last_env_shape);

        let total_frames = frames.len();
        let frames_cb = Arc::clone(&frames);

        let state = Arc::new(Mutex::new(PlaybackState {
            chip,
            frame_idx: 0,
            sample_in_frame: 0,
            mixer,
            last_env_shape,
            finished: false,
        }));

        let progress = PlaybackProgress {
            current_frame: Arc::new(AtomicUsize::new(0)),
            finished: Arc::new(AtomicBool::new(false)),
        };

        let state_cb = Arc::clone(&state);
        let current_frame_atomic = Arc::clone(&progress.current_frame);
        let finished_atomic = Arc::clone(&progress.finished);

        let channels = sink.channels;
        let err_fn = |err| eprintln!("{} {}", style("Audio stream error:").red().bold(), err);

        let stream = match sink.sample_format {
            cpal::SampleFormat::F32 => sink.device.build_output_stream(
                sink.stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut s = state_cb.lock().unwrap_or_else(|e| e.into_inner());

                    if s.finished {
                        for sample in data.iter_mut() {
                            *sample = 0.0;
                        }
                        finished_atomic.store(true, Ordering::Relaxed);
                        return;
                    }

                    let mut i = 0;
                    while i < data.len() {
                        let sample_val = s.chip.get_sample();
                        s.chip.clock();

                        for c in 0..channels {
                            if i + c < data.len() {
                                data[i + c] = sample_val;
                            }
                        }
                        i += channels;

                        s.sample_in_frame += 1;
                        if s.sample_in_frame >= samples_per_frame {
                            s.sample_in_frame = 0;
                            s.frame_idx += 1;

                            if s.frame_idx >= total_frames {
                                if let Some(l_start) = loop_start {
                                    s.frame_idx = l_start;
                                    let idx = s.frame_idx;
                                    let s = &mut *s;
                                    apply(
                                        &frames_cb[idx],
                                        &mut s.chip,
                                        &mut s.mixer,
                                        &mut s.last_env_shape,
                                    );
                                } else {
                                    s.finished = true;
                                    finished_atomic.store(true, Ordering::Relaxed);
                                }
                            } else {
                                let idx = s.frame_idx;
                                let s = &mut *s;
                                apply(
                                    &frames_cb[idx],
                                    &mut s.chip,
                                    &mut s.mixer,
                                    &mut s.last_env_shape,
                                );
                            }
                            current_frame_atomic.store(s.frame_idx, Ordering::Relaxed);
                        }
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported audio sample format".into()),
        };

        Ok((stream, progress))
    }

    /// Plays a compiled SFX sequence on the default audio device, blocking until playback finishes.
    pub fn play_sfx(sequence: &SfxSequence) -> Result<(), Box<dyn std::error::Error>> {
        if sequence.frames.is_empty() {
            println!("{}", style("Sequence contains no frames to play.").yellow());
            return Ok(());
        }

        let audio = AudioOutputSession::open_default()?;
        let chip = Ym2149::with_clocks(sequence.source_clock, audio.sample_rate);
        let hz = sequence.source_hz;
        let samples_per_frame = Self::calculate_samples_per_frame(audio.sample_rate, hz);

        let frames: Arc<[SfxFrame]> = sequence.frames.as_slice().into();
        let total_frames = frames.len();

        let channel = sequence
            .preferred_channels
            .as_ref()
            .and_then(|c| c.first().copied())
            .unwrap_or(YmChannel::A);

        let (stream, progress) = Self::build_stream(
            audio.as_sink(),
            samples_per_frame,
            chip,
            frames,
            None,
            move |frame: &SfxFrame, chip, mixer, _| frame.apply_to_chip(chip, mixer, channel),
        )?;

        stream.play()?;

        println!(
            "{} '{}' ({} frames @ {} Hz on channel {:?})",
            style("PLAYING SOUND EFFECT:").bold().green(),
            sequence.name,
            total_frames,
            hz,
            channel
        );

        Self::monitor_frame_progress(progress, total_frames, false)?;
        std::thread::sleep(Duration::from_millis(100)); // drain queued buffer
        Ok(())
    }

    /// Plays a compiled song sequence on the default audio device, blocking until playback finishes (or loops indefinitely).
    pub fn play(sequence: &YmSequence) -> Result<(), Box<dyn std::error::Error>> {
        if sequence.frames.is_empty() {
            println!("{}", style("Sequence contains no frames to play.").yellow());
            return Ok(());
        }

        let audio = AudioOutputSession::open_default()?;
        let chip = Ym2149::with_clocks(sequence.timing.master_clock_hz, audio.sample_rate);
        let hz = sequence.timing.frame_rate.hz_value();
        let samples_per_frame = Self::calculate_samples_per_frame(audio.sample_rate, hz);

        let frames: Arc<[YmFrame]> = sequence.frames.as_slice().into();
        let total_frames = frames.len();
        let loop_start_val = sequence.loop_start;

        let (stream, progress) = Self::build_stream(
            audio.as_sink(),
            samples_per_frame,
            chip,
            frames,
            loop_start_val,
            |frame: &YmFrame, chip, mixer, last_env_shape| {
                frame.apply_to_chip(chip, mixer, last_env_shape)
            },
        )?;

        stream.play()?;

        println!(
            "{} '{}' ({} frames @ {} Hz)",
            style("PLAYING SONG:").bold().green(),
            sequence.name,
            total_frames,
            hz
        );

        Self::monitor_frame_progress(progress, total_frames, loop_start_val.is_some())?;
        if loop_start_val.is_none() {
            std::thread::sleep(Duration::from_millis(100)); // drain queued buffer
        }
        Ok(())
    }

    /// Plays raw YM chiptune data via the ym2149 replayer, blocking for the song's duration.
    pub fn play_ym_data(ym_data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        use ym2149_common::ChiptunePlayerBase;
        use ym2149_ym_replayer::player::PlaybackController;

        let audio = AudioOutputSession::open_default()?;
        let decompressed = ym2149_ym_replayer::compression::decompress_if_needed(ym_data)?;
        let (mut player, summary) = ym2149_ym_replayer::player::ym_player::load_song_with_rate(
            &decompressed,
            audio.sample_rate,
        )?;

        PlaybackController::play(&mut player)?;

        println!(
            "{} {:?}  {} {}  {} {}",
            style("FORMAT:").bold(),
            summary.format,
            style("FRAMES:").bold(),
            summary.frame_count,
            style("SAMPLES/FRAME:").bold(),
            summary.samples_per_frame
        );

        let player_mutex = Arc::new(Mutex::new(player));
        let player_cb = Arc::clone(&player_mutex);
        let err_fn = |err| eprintln!("{} {}", style("Audio stream error:").red().bold(), err);

        // Pre-allocate buffer with fixed 8192 capacity outside closure to avoid real-time audio thread allocations
        let mut temp_buf = vec![0.0f32; 8192];

        let stream = match audio.sample_format {
            cpal::SampleFormat::F32 => audio.device.build_output_stream(
                audio.stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut player = player_cb.lock().unwrap_or_else(|e| e.into_inner());

                    let needed_len = (data.len() / audio.channels).min(temp_buf.len());
                    let slice = &mut temp_buf[..needed_len];
                    slice.fill(0.0);
                    player.generate_samples_into(slice);

                    let mut temp_idx = 0;
                    for frame in data.chunks_exact_mut(audio.channels) {
                        if temp_idx < needed_len {
                            let sample_val = slice[temp_idx];
                            for sample in frame.iter_mut() {
                                *sample = sample_val;
                            }
                            temp_idx += 1;
                        }
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported audio sample format".into()),
        };

        stream.play()?;

        let duration = player_mutex
            .lock()
            .map_err(|_| "player mutex poisoned")?
            .duration_seconds() as f64;
        println!("{} {:.1}s", style("PLAYING SONG:").bold().green(), duration);

        Self::monitor_time_progress(duration)?;
        Ok(())
    }

    /// Computes target audio samples per frame given system sample rate and target Hz.
    fn calculate_samples_per_frame(sample_rate: cpal::SampleRate, hz: u32) -> usize {
        let hz_valid = hz.max(1);
        (sample_rate as f64 / hz_valid as f64).round() as usize
    }

    /// Monitors frame-based playback (SFX or Song) with a terminal progress bar lock-freely.
    fn monitor_frame_progress(
        progress: PlaybackProgress,
        total_frames: usize,
        is_looping: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pb = frame_progress_bar(total_frames as u64);
        if is_looping {
            pb.set_message(format!(" {}", style("(looping, Ctrl+C to stop)").yellow()));
        }

        loop {
            let current_frame = progress.current_frame.load(Ordering::Relaxed);
            let is_done = progress.finished.load(Ordering::Relaxed);

            pb.set_position(current_frame.min(total_frames) as u64);
            if is_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        pb.finish_and_clear();
        Ok(())
    }

    /// Monitors elapsed-time playback for raw YM files.
    fn monitor_time_progress(duration: f64) -> Result<(), Box<dyn std::error::Error>> {
        let total_deciseconds = (duration * 10.0).round().max(1.0) as u64;
        let pb = time_progress_bar(total_deciseconds);

        let start = std::time::Instant::now();
        while start.elapsed().as_secs_f64() < duration {
            let elapsed = start.elapsed().as_secs_f64();
            pb.set_position(((elapsed * 10.0).round() as u64).min(total_deciseconds));
            pb.set_message(format!("{:.1}s / {:.1}s", elapsed, duration));
            std::thread::sleep(Duration::from_millis(100));
        }
        pb.finish_and_clear();
        Ok(())
    }
}
