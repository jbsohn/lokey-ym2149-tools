use console::{Key, Term, style};
use cpal::Sample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Duration;
use ym_core::{
    SfxSequence, TimingConfig, YmChannel, YmDataRenderer, YmMixer, YmSequence, YmSfxRenderer,
    YmSongRenderer,
};

/// Configuration options for host audio output settings.
#[derive(Debug, Clone, Default)]
pub(crate) struct AudioConfig {
    /// Optional sample rate override in Hz (e.g. 44100, 48000).
    /// If `None`, uses the output device's default sample rate.
    pub(crate) sample_rate: Option<u32>,
    /// Optional audio output device name filter.
    /// If `None`, uses the host system default output audio device.
    pub(crate) device_name: Option<String>,
}

/// Opened host cpal audio output device and negotiated stream parameters.
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
        let sample_rate = config
            .sample_rate
            .unwrap_or_else(|| default_config.sample_rate());
        let channels = usize::from(default_config.channels());
        let sample_format = default_config.sample_format();
        let mut stream_config: cpal::StreamConfig = default_config.into();
        stream_config.sample_rate = sample_rate;

        Ok(Self {
            device,
            sample_rate,
            channels,
            sample_format,
            stream_config,
        })
    }
}

/// Restores terminal state when interactive raw key listening exits.
struct TerminalGuard {
    active: bool,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = std::process::Command::new("stty").arg("sane").status();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Spawns a background thread reading raw key presses when stdout is a TTY.
fn spawn_key_listener() -> (Option<mpsc::Receiver<Key>>, TerminalGuard) {
    if !Term::stdout().is_term() {
        return (None, TerminalGuard { active: false });
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let term = Term::stdout();
        while let Ok(key) = term.read_key() {
            if tx.send(key).is_err() {
                break;
            }
        }
    });
    (Some(rx), TerminalGuard { active: true })
}

/// Prints the keyboard trigger for each of the first 10 loaded SFX.
fn print_sfx_keybindings(sfx_list: &[SfxSequence]) {
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

fn frame_progress_bar(total_frames: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_frames);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] frame {pos}/{len}{msg}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=>-"),
    );
    pb.set_message(message.to_string());
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

/// Pulls mono `f32` samples from `render_mono` and duplicates each sample across `channels`.
fn fill_interleaved_mono<T>(
    mut render_mono: impl FnMut(&mut [f32]),
    data: &mut [T],
    channels: usize,
) where
    T: Sample + cpal::FromSample<f32>,
{
    const CHUNK_FRAMES: usize = 256;

    let ch = channels.max(1);
    let mut mono = [0.0f32; CHUNK_FRAMES];
    for out_chunk in data.chunks_mut(CHUNK_FRAMES * ch) {
        let frames = out_chunk.len().div_ceil(ch);
        let mono_slice = &mut mono[..frames];
        render_mono(mono_slice);
        for (out_frame, &src) in out_chunk.chunks_mut(ch).zip(mono_slice.iter()) {
            out_frame.fill(Sample::from_sample(src));
        }
    }
}

fn build_typed_stream<T>(
    audio: &AudioOutputSession,
    mut render_mono: impl FnMut(&mut [f32]) + Send + 'static,
) -> Result<cpal::Stream, Box<dyn std::error::Error>>
where
    T: cpal::SizedSample + Sample + cpal::FromSample<f32>,
{
    let channels = audio.channels.max(1);
    let err_fn = |err| eprintln!("{} {}", style("Audio stream error:").red().bold(), err);
    Ok(audio.device.build_output_stream(
        audio.stream_config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            fill_interleaved_mono(&mut render_mono, data, channels);
        },
        err_fn,
        None,
    )?)
}

/// Builds a cpal output stream for the host device's sample format, pulling mono f32
/// samples from `render_mono` and interleaving them across all host output channels.
fn build_output_stream(
    audio: &AudioOutputSession,
    render_mono: impl FnMut(&mut [f32]) + Send + 'static,
) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    match audio.sample_format {
        cpal::SampleFormat::F32 => build_typed_stream::<f32>(audio, render_mono),
        cpal::SampleFormat::I16 => build_typed_stream::<i16>(audio, render_mono),
        cpal::SampleFormat::U16 => build_typed_stream::<u16>(audio, render_mono),
        other => Err(format!("Unsupported audio sample format: {other:?}").into()),
    }
}

/// Auditions a song sequence over system audio.
///
/// # Errors
/// Returns an error if initializing the host audio device or stream fails.
pub(crate) fn play_song(
    sequence: &YmSequence,
    config: &AudioConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if sequence.frames.is_empty() {
        println!("{}", style("Sequence contains no frames to play.").yellow());
        return Ok(());
    }

    let audio = AudioOutputSession::open(config)?;
    let renderer = Arc::new(Mutex::new(YmSongRenderer::new(sequence, audio.sample_rate)));

    let audio_renderer = Arc::clone(&renderer);
    let stream = build_output_stream(&audio, move |buf| {
        lock(&audio_renderer).render_samples(buf);
    })?;
    stream.play()?;

    let total_frames = sequence.frames.len();
    let hz = sequence.timing.frame_rate;
    println!(
        "{} '{}' ({} frames @ {} Hz)",
        style("PLAYING SONG:").bold().green(),
        sequence.name,
        total_frames,
        hz
    );

    let pb = frame_progress_bar(
        total_frames as u64,
        &format!(
            " {}",
            style("(\u{2190}/\u{2192} to seek, 'q' to quit)").yellow()
        ),
    );
    let (key_rx, _guard) = spawn_key_listener();
    let seek_step_frames = ((hz as usize) * 5).max(1);

    loop {
        if let Some(rx) = &key_rx {
            while let Ok(key) = rx.try_recv() {
                match key {
                    Key::ArrowRight => {
                        let mut r = lock(&renderer);
                        let target = r.current_frame().saturating_add(seek_step_frames);
                        r.seek(target);
                    }
                    Key::ArrowLeft => {
                        let mut r = lock(&renderer);
                        let target = r.current_frame().saturating_sub(seek_step_frames);
                        r.seek(target);
                    }
                    Key::Char('q' | 'Q') => lock(&renderer).set_finished(true),
                    _ => {}
                }
            }
        }

        let (current, is_done) = {
            let r = lock(&renderer);
            (r.current_frame(), r.is_finished())
        };
        pb.set_position(current.min(total_frames) as u64);
        if is_done {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    pb.finish_and_clear();
    if sequence.loop_start.is_none() {
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Auditions a sound effect sequence over system audio.
///
/// # Errors
/// Returns an error if initializing the host audio device or stream fails.
pub(crate) fn play_sfx(
    sequence: &SfxSequence,
    channel: YmChannel,
    timing: TimingConfig,
    config: &AudioConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if sequence.frames.is_empty() {
        println!("{}", style("Sequence contains no frames to play.").yellow());
        return Ok(());
    }

    let audio = AudioOutputSession::open(config)?;
    let renderer = Arc::new(Mutex::new(YmSfxRenderer::new(
        sequence,
        channel,
        timing,
        audio.sample_rate,
    )));

    let audio_renderer = Arc::clone(&renderer);
    let stream = build_output_stream(&audio, move |buf| {
        lock(&audio_renderer).render_samples(buf);
    })?;
    stream.play()?;

    let total_frames = sequence.frames.len();
    println!(
        "{} '{}' ({} frames @ {} Hz on channel {:?})",
        style("PLAYING SOUND EFFECT:").bold().green(),
        sequence.name,
        total_frames,
        timing.frame_rate,
        channel
    );

    let pb = frame_progress_bar(
        total_frames as u64,
        &format!(" {}", style("('q' to quit)").yellow()),
    );
    let (key_rx, _guard) = spawn_key_listener();

    loop {
        if let Some(rx) = &key_rx {
            while let Ok(key) = rx.try_recv() {
                if matches!(key, Key::Char('q' | 'Q')) {
                    lock(&renderer).set_finished(true);
                }
            }
        }

        let (current, is_done) = {
            let r = lock(&renderer);
            (r.current_frame(), r.is_finished())
        };
        pb.set_position(current.min(total_frames) as u64);
        if is_done {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    pb.finish_and_clear();
    std::thread::sleep(Duration::from_millis(100));
    Ok(())
}

/// Auditions an interactive song and sound effect mix over system audio.
///
/// # Errors
/// Returns an error if initializing the host audio device or stream fails.
pub(crate) fn play_mix(
    song_seq: &YmSequence,
    sfx_list: &[SfxSequence],
    preferred_channel: YmChannel,
    config: &AudioConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    print_sfx_keybindings(sfx_list);

    let audio = AudioOutputSession::open(config)?;
    let mixer = Arc::new(Mutex::new(YmMixer::new(
        song_seq,
        sfx_list,
        preferred_channel,
        audio.sample_rate,
    )));

    let audio_mixer = Arc::clone(&mixer);
    let stream = build_output_stream(&audio, move |buf| {
        lock(&audio_mixer).render_samples(buf);
    })?;
    stream.play()?;

    let total_song_frames = song_seq.frames.len();
    let song_hz = song_seq.timing.frame_rate;
    let pb = frame_progress_bar(
        total_song_frames as u64,
        &format!(
            "  Press \u{2190}/\u{2192} to seek, 1-9/0/SPACE to trigger SFX, 'q' to quit (Primary Ch: {preferred_channel:?})"
        ),
    );
    let (key_rx, _guard) = spawn_key_listener();
    let seek_step_frames = ((song_hz as usize) * 5).max(1);

    loop {
        if let Some(ref rx) = key_rx {
            while let Ok(key) = rx.try_recv() {
                match key {
                    Key::Char('q' | 'Q') => {
                        lock(&mixer).set_finished(true);
                        break;
                    }
                    Key::ArrowRight => {
                        let mut m = lock(&mixer);
                        let target = m
                            .current_song_frame()
                            .saturating_add(seek_step_frames)
                            .min(total_song_frames.saturating_sub(1));
                        m.seek_song(target);
                    }
                    Key::ArrowLeft => {
                        let mut m = lock(&mixer);
                        let target = m.current_song_frame().saturating_sub(seek_step_frames);
                        m.seek_song(target);
                    }
                    other => {
                        if let Some(sfx_idx) = key_to_sfx_index(&other) {
                            lock(&mixer).trigger_sfx(sfx_idx);
                        }
                    }
                }
            }
        }

        let (current, is_done) = {
            let m = lock(&mixer);
            (m.current_song_frame(), m.is_finished())
        };
        pb.set_position(current.min(total_song_frames) as u64);
        if is_done {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }

    pb.finish_with_message("Playback finished.");
    Ok(())
}

/// Auditions raw YM chiptune data over system audio.
///
/// # Errors
/// Returns an error if decompressing or initializing host audio fails.
pub(crate) fn play_ym_data(
    ym_data: &[u8],
    config: &AudioConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let audio = AudioOutputSession::open(config)?;
    let renderer = Arc::new(Mutex::new(YmDataRenderer::new(ym_data, audio.sample_rate)?));
    let duration = lock(&renderer).duration_seconds();

    let audio_renderer = Arc::clone(&renderer);
    let stream = build_output_stream(&audio, move |buf| {
        lock(&audio_renderer).render_samples(buf);
    })?;
    stream.play()?;

    println!("{} {:.1}s", style("PLAYING SONG:").bold().green(), duration);

    let total_deciseconds = (duration * 10.0).round().max(1.0) as u64;
    let pb = ProgressBar::new(total_deciseconds);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
    );
    let (key_rx, _guard) = spawn_key_listener();

    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f64() < duration {
        if let Some(rx) = &key_rx {
            while let Ok(key) = rx.try_recv() {
                if matches!(key, Key::Char('q' | 'Q')) {
                    pb.finish_and_clear();
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_interleaved_mono_duplicates_to_stereo_speakers() {
        let mut val = 0.25f32;
        let mut stereo = [0.0f32; 6];
        fill_interleaved_mono(
            |mono| {
                for s in mono.iter_mut() {
                    *s = val;
                    val += 0.25;
                }
            },
            &mut stereo,
            2,
        );
        assert_eq!(stereo, [0.25, 0.25, 0.5, 0.5, 0.75, 0.75]);
    }
}
