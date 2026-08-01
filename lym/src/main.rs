use clap::{Args, Parser, Subcommand, ValueEnum};
use console::{style, Key};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ym2149::{Ym2149, Ym2149Backend};
use ym_core::{
    spawn_key_listener, AudioPlayer, CompilerOptions, CompressionLevel, DeltaCompiler, HzOption,
    SfxFrame, SfxSequence, SystemHz, YmChannel, YmFrame, YmSequence,
};

#[derive(Parser, Debug)]
#[command(
    name = "lym",
    version,
    about = "Lokey YM-2149 Command Line Toolchain",
    long_about = "A unified CLI tool for compiling, auditioning, and interactively mixing music songs and sound effects targeting the Yamaha YM-2149 Programmable Sound Generator."
)]
struct LymCli {
    #[command(subcommand)]
    command: MainCommands,
}

#[derive(Subcommand, Debug)]
enum MainCommands {
    /// Music song tools (compile, play, dump)
    Song {
        #[command(subcommand)]
        command: SongCommands,
    },
    /// Sound effect tools (compile, play)
    Sfx {
        #[command(subcommand)]
        command: SfxCommands,
    },
    /// Real-time interactive music & sound effect keyboard mixer
    Mix {
        /// Input background song file (.ysg, .ym, .json)
        #[arg(short, long)]
        song: PathBuf,

        /// One or more input sound effect files or banks (.yfx, .json, .csv, .afx, .afb)
        #[arg(short = 'e', long, num_args = 1..)]
        sfx: Vec<PathBuf>,

        /// Preferred primary channel on which to play SFX (A, B, or C)
        #[arg(short, long, value_enum, default_value = "c")]
        channel: ChannelArg,

        /// Timing refresh rate override (50 or 60 Hz)
        #[arg(long, value_enum)]
        hz: Option<HzOption>,

        /// Source chip clock in Hz (default: 2000000 for ST)
        #[arg(long)]
        clock: Option<u32>,
    },
}

// --- SONG SUBCOMMANDS ---

#[derive(Subcommand, Debug)]
enum SongCommands {
    /// Render a music song file into compiled YM-2149 binary stream (.ysg)
    Render {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long)]
        output: Option<PathBuf>,

        #[arg(long, value_enum)]
        hz: Option<HzOption>,

        #[arg(long)]
        clock: Option<u32>,

        #[arg(short, long, default_value_t = 1)]
        step: usize,

        #[arg(long, value_enum, default_value = "full")]
        compression: CompressionArg,

        #[arg(long)]
        no_dedup: bool,

        #[arg(long)]
        no_rle: bool,

        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// Dump raw frame register data for diagnostic inspection
    Dump {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long, default_value_t = 100)]
        frames: usize,

        #[arg(long, default_value_t = 0)]
        start: usize,
    },
    /// Audition and play a music song file or stream
    Play {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(long, value_enum)]
        hz: Option<HzOption>,

        #[arg(long)]
        via_sequence: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy)]
enum CompressionArg {
    Full,
    DeltaOnly,
    None,
}

impl From<CompressionArg> for CompressionLevel {
    fn from(a: CompressionArg) -> Self {
        match a {
            CompressionArg::Full => CompressionLevel::Full,
            CompressionArg::DeltaOnly => CompressionLevel::DeltaOnly,
            CompressionArg::None => CompressionLevel::None,
        }
    }
}

// --- SFX SUBCOMMANDS ---

#[derive(Args, Debug)]
struct SfxCommonArgs {
    #[arg(short, long)]
    input: PathBuf,

    #[arg(long, value_enum)]
    hz: Option<HzOption>,

    #[arg(long)]
    clock: Option<u32>,

    #[arg(long, default_value_t = 0)]
    index: usize,
}

#[derive(Subcommand, Debug)]
enum SfxCommands {
    /// Render a sound effect source file into compiled YM-2149 binary payload (.yfx)
    Render {
        #[command(flatten)]
        common: SfxCommonArgs,

        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Audition and play a sound effect sequence
    Play {
        #[command(flatten)]
        common: SfxCommonArgs,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelArg {
    A,
    B,
    C,
}

impl From<ChannelArg> for YmChannel {
    fn from(c: ChannelArg) -> Self {
        match c {
            ChannelArg::A => YmChannel::A,
            ChannelArg::B => YmChannel::B,
            ChannelArg::C => YmChannel::C,
        }
    }
}

fn channel_arg_to_idx(c: ChannelArg) -> usize {
    match c {
        ChannelArg::A => 0,
        ChannelArg::B => 1,
        ChannelArg::C => 2,
    }
}

fn idx_to_ym_channel(idx: usize) -> YmChannel {
    match idx {
        0 => YmChannel::A,
        1 => YmChannel::B,
        2 => YmChannel::C,
        _ => YmChannel::C,
    }
}

// --- HELPERS ---

fn with_spinner<T>(message: &str, f: impl FnOnce() -> T) -> T {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}").unwrap());
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));

    let result = f();
    pb.finish_and_clear();
    result
}

fn load_song(
    input: &Path,
    clock_override: Option<u32>,
) -> Result<YmSequence, Box<dyn std::error::Error>> {
    YmSequence::load_from_path(input, clock_override)
}

fn load_sfx(input: &Path, bank_index: usize) -> Result<SfxSequence, Box<dyn std::error::Error>> {
    SfxSequence::load_from_path(input, bank_index)
}

fn load_all_sfx(inputs: &[PathBuf]) -> Result<Vec<SfxSequence>, Box<dyn std::error::Error>> {
    SfxSequence::load_all_from_paths(inputs)
}

#[derive(Clone)]
struct PlayingSfx {
    frames: Arc<[SfxFrame]>,
    current_idx: usize,
}

struct MixerState {
    chip: Ym2149,
    song_frame_idx: usize,
    sample_in_frame: usize,
    mixer: u8,
    last_env_shape: Option<u8>,
    finished: bool,
    active_sfx: [Option<PlayingSfx>; 3],
}

// --- MAIN ENTRYPOINT ---

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = LymCli::parse();

    match cli.command {
        MainCommands::Song { command } => match command {
            SongCommands::Dump {
                input,
                frames,
                start,
            } => {
                let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");
                let ym_data = fs::read(&input)?;
                let (sequence, _) = with_spinner("Decoding...", || {
                    YmSequence::from_ym_data(name, &ym_data, None)
                })?;

                let end = (start + frames).min(sequence.frames.len());
                println!(
                    "{:>6}  {:>6} {:>6} {:>6}  {:>4}  {:>4}  {:>3} {:>3} {:>3}  {:>4} {:>4} {:>4}  {:>4}  {:>4}  R13",
                    "frame", "toneA", "toneB", "toneC", "volA", "volB", "volC",
                    "teA", "teB", "teC", "neA", "neB", "neC", "envP"
                );
                for (i, f) in sequence.frames[start..end].iter().enumerate() {
                    let r13 = match f.envelope_shape {
                        Some(v) => format!("{}", v),
                        None => "---".to_string(),
                    };
                    println!(
                        "{:>6}  {:>6} {:>6} {:>6}  {:>4}  {:>4}  {:>3} {:>3} {:>3}  {:>4} {:>4} {:>4}  {:>4}  {:>4}  {}",
                        start + i,
                        f.tone_a.map(|v| v.to_string()).unwrap_or("-".into()),
                        f.tone_b.map(|v| v.to_string()).unwrap_or("-".into()),
                        f.tone_c.map(|v| v.to_string()).unwrap_or("-".into()),
                        f.volume_a.map(|v| v.to_string()).unwrap_or("-".into()),
                        f.volume_b.map(|v| v.to_string()).unwrap_or("-".into()),
                        f.volume_c.map(|v| v.to_string()).unwrap_or("-".into()),
                        f.tone_enable_a.map(|v| if v { "T" } else { "f" }).unwrap_or("-"),
                        f.tone_enable_b.map(|v| if v { "T" } else { "f" }).unwrap_or("-"),
                        f.tone_enable_c.map(|v| if v { "T" } else { "f" }).unwrap_or("-"),
                        f.noise_enable_a.map(|v| if v { "T" } else { "f" }).unwrap_or("-"),
                        f.noise_enable_b.map(|v| if v { "T" } else { "f" }).unwrap_or("-"),
                        f.noise_enable_c.map(|v| if v { "T" } else { "f" }).unwrap_or("-"),
                        f.envelope_period.map(|v| v.to_string()).unwrap_or("-".into()),
                        r13,
                    );
                }
                println!("\nTotal frames: {}", sequence.frames.len());
            }
            SongCommands::Render {
                input,
                output,
                hz,
                clock,
                step,
                compression,
                no_dedup,
                no_rle,
                max_bytes,
            } => {
                let output_path = output.unwrap_or_else(|| {
                    let mut path = input.clone();
                    path.set_extension("ysg");
                    path
                });

                let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
                let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");

                println!(
                    "{} {}...",
                    style("LOADING:").bold().cyan(),
                    style(input.display()).cyan()
                );

                let mut original_ym_size: Option<usize> = None;
                let (mut sequence, digidrum_frames) = if extension.eq_ignore_ascii_case("ym") {
                    let bytes = fs::read(&input)?;
                    original_ym_size = Some(YmSequence::ym_decompressed_len(&bytes)?);
                    with_spinner("Decoding YM chiptune...", || {
                        YmSequence::from_ym_data(name, &bytes, clock)
                    })?
                } else {
                    let content = fs::read_to_string(&input)?;
                    (serde_json::from_str(&content)?, 0)
                };

                let step = step.max(1);
                let limit = sequence.frames.len();
                let mut decimated_frames = Vec::new();
                let mut i = 0;
                while i < limit {
                    let window_end = (i + step).min(limit);
                    let mut best_idx_a = i;
                    let mut best_vol_a = sequence.frames[i].volume_a.unwrap_or(0);
                    let mut best_idx_b = i;
                    let mut best_vol_b = sequence.frames[i].volume_b.unwrap_or(0);
                    let mut best_idx_c = i;
                    let mut best_vol_c = sequence.frames[i].volume_c.unwrap_or(0);

                    for idx in i..window_end {
                        let f = &sequence.frames[idx];
                        let v_a = f.volume_a.unwrap_or(0);
                        if v_a > best_vol_a {
                            best_vol_a = v_a;
                            best_idx_a = idx;
                        }
                        let v_b = f.volume_b.unwrap_or(0);
                        if v_b > best_vol_b {
                            best_vol_b = v_b;
                            best_idx_b = idx;
                        }
                        let v_c = f.volume_c.unwrap_or(0);
                        if v_c > best_vol_c {
                            best_vol_c = v_c;
                            best_idx_c = idx;
                        }
                    }

                    let dominant_idx = if best_vol_a >= best_vol_b && best_vol_a >= best_vol_c {
                        best_idx_a
                    } else if best_vol_b >= best_vol_c {
                        best_idx_b
                    } else {
                        best_idx_c
                    };

                    let mut final_frame = sequence.frames[i].clone();
                    final_frame.volume_a = sequence.frames[best_idx_a].volume_a;
                    final_frame.tone_a = sequence.frames[best_idx_a].tone_a;
                    final_frame.tone_enable_a = sequence.frames[best_idx_a].tone_enable_a;
                    final_frame.noise_enable_a = sequence.frames[best_idx_a].noise_enable_a;

                    final_frame.volume_b = sequence.frames[best_idx_b].volume_b;
                    final_frame.tone_b = sequence.frames[best_idx_b].tone_b;
                    final_frame.tone_enable_b = sequence.frames[best_idx_b].tone_enable_b;
                    final_frame.noise_enable_b = sequence.frames[best_idx_b].noise_enable_b;

                    final_frame.volume_c = sequence.frames[best_idx_c].volume_c;
                    final_frame.tone_c = sequence.frames[best_idx_c].tone_c;
                    final_frame.tone_enable_c = sequence.frames[best_idx_c].tone_enable_c;
                    final_frame.noise_enable_c = sequence.frames[best_idx_c].noise_enable_c;

                    final_frame.noise_period = sequence.frames[dominant_idx].noise_period;
                    final_frame.envelope_period = sequence.frames[dominant_idx].envelope_period;
                    final_frame.envelope_shape = sequence.frames[dominant_idx].envelope_shape;

                    decimated_frames.push(final_frame);
                    i += step;
                }
                sequence.frames = decimated_frames;

                if let Some(hz_override) = hz {
                    sequence.timing.frame_rate = hz_override.into();
                } else if step > 1 {
                    let current_hz = sequence.timing.frame_rate.hz_value();
                    let decimated_hz = (current_hz as f64 / step as f64).round().max(1.0) as u32;
                    sequence.timing.frame_rate = SystemHz::Custom(decimated_hz);
                }

                let compiler = DeltaCompiler::new();
                let compression_level: CompressionLevel = compression.into();
                let compiler_options = CompilerOptions {
                    dedup: !no_dedup,
                    rle: !no_rle,
                    ..CompilerOptions::default()
                };

                let mut compiled = with_spinner("Compiling song...", || {
                    compiler.compile_song(&sequence, compression_level, &compiler_options)
                })?;

                if let Some(limit) = max_bytes {
                    if compiled.bytes.len() > limit {
                        let original_frames = sequence.frames.len();
                        loop {
                            let pattern_size = compiled.pattern_size;
                            let current_patterns = sequence.frames.len() / pattern_size;
                            if current_patterns == 0 {
                                return Err(
                                    "Cannot fit even one pattern within --max-bytes limit".into()
                                );
                            }
                            sequence
                                .frames
                                .truncate((current_patterns - 1) * pattern_size);
                            compiled = compiler.compile_song(
                                &sequence,
                                compression_level,
                                &compiler_options,
                            )?;
                            if compiled.bytes.len() <= limit {
                                break;
                            }
                        }
                        let dropped = original_frames - sequence.frames.len();
                        println!(
                            "{} truncated {} frames to fit within {} bytes",
                            style("WARNING:").bold().yellow(),
                            style(dropped).yellow(),
                            style(limit).yellow(),
                        );
                    }
                }

                fs::write(&output_path, &compiled.bytes)?;

                let final_hz = sequence.timing.frame_rate.hz_value();
                let (delay_y, delay_x) = ym_core::calculate_delay(final_hz);
                let num_patterns = compiled.bytes.get(1).copied().unwrap_or(0);
                let seq_len = compiled.bytes.get(2).copied().unwrap_or(0);

                let ysi_path = output_path.with_extension("ysi");
                let scope_name: String = name
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let ysi_contents = format!(
                    "; ca65 include generated by lym for {}\n\
                     .scope {}\n\
                         MAX_FRAMES   = {}\n\
                         PLAYER_HZ    = {}\n\
                         MASTER_CLOCK = {}\n\
                         YM_DELAY     = {}\n\
                         YM_FINE      = {}\n\
                         PATTERN_SIZE = {}\n\
                         NUM_PATTERNS = {}\n\
                         SEQ_LEN      = {}\n\
                     .endscope\n",
                    input.display(),
                    scope_name,
                    sequence.frames.len(),
                    final_hz,
                    sequence.timing.master_clock_hz,
                    delay_y,
                    delay_x,
                    compiled.pattern_size,
                    num_patterns,
                    seq_len,
                );
                fs::write(&ysi_path, ysi_contents)?;

                println!(
                    "{} {} frames -> {} ({} bytes, pattern size {}, {} Hz)",
                    style("RENDER SUCCESS:").bold().green(),
                    style(sequence.frames.len()).cyan(),
                    style(output_path.display()).cyan(),
                    style(compiled.bytes.len()).cyan(),
                    style(compiled.pattern_size).cyan(),
                    style(final_hz).cyan()
                );

                if digidrum_frames > 0 {
                    println!(
                        "{} {} frames contain YM6 digi-drum data — drums dropped, pitched content preserved",
                        style("WARNING:").bold().yellow(),
                        style(digidrum_frames).yellow(),
                    );
                }

                if let Some(original_size) = original_ym_size {
                    let new_size = compiled.bytes.len();
                    let pct_change = if original_size > 0 {
                        100.0 * (original_size as f64 - new_size as f64) / original_size as f64
                    } else {
                        0.0
                    };
                    println!(
                        "{} {} bytes (uncompressed .ym) -> {} bytes (.ysg) ({:.1}% change)",
                        style("SIZE:").bold(),
                        style(original_size).cyan(),
                        style(new_size).cyan(),
                        pct_change
                    );
                }
            }
            SongCommands::Play {
                input,
                hz,
                via_sequence,
            } => {
                let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");

                if extension == "json" || extension == "ysg" {
                    let mut sequence = load_song(&input, None)?;
                    if let Some(hz_override) = hz {
                        sequence.timing.frame_rate = hz_override.into();
                    }
                    println!(
                        "{} {} ({} Hz)...",
                        style("LOADING:").bold().cyan(),
                        style(input.display()).cyan(),
                        sequence.timing.frame_rate.hz_value()
                    );
                    AudioPlayer::play(&sequence)?;
                } else if via_sequence && extension.eq_ignore_ascii_case("ym") {
                    let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");
                    let ym_data = fs::read(&input)?;
                    let (mut sequence, _) =
                        with_spinner("Decoding YM via YmSequence pipeline...", || {
                            YmSequence::from_ym_data(name, &ym_data, None)
                        })?;
                    if let Some(hz_override) = hz {
                        sequence.timing.frame_rate = hz_override.into();
                    }
                    println!(
                        "{} {} ({} frames @ {} Hz)...",
                        style("LOADING:").bold().cyan(),
                        style(input.display()).cyan(),
                        sequence.frames.len(),
                        sequence.timing.frame_rate.hz_value()
                    );
                    AudioPlayer::play(&sequence)?;
                } else {
                    println!(
                        "{} {}...",
                        style("LOADING:").bold().cyan(),
                        style(input.display()).cyan()
                    );
                    let ym_data = fs::read(&input)?;
                    AudioPlayer::play_ym_data(&ym_data)?;
                }
            }
        },

        MainCommands::Sfx { command } => match command {
            SfxCommands::Render {
                common:
                    SfxCommonArgs {
                        input,
                        hz,
                        clock,
                        index,
                    },
                output,
            } => {
                let output_path = output.unwrap_or_else(|| {
                    let mut path = input.clone();
                    path.set_extension("yfx");
                    path
                });

                println!(
                    "{} {}...",
                    style("LOADING SFX:").bold().cyan(),
                    style(input.display()).cyan()
                );
                let mut sequence = load_sfx(&input, index)?;
                if let Some(c) = clock {
                    sequence.source_clock = c;
                }
                if let Some(hz_override) = hz {
                    sequence.source_hz = SystemHz::from(hz_override).hz_value();
                }

                let compiler = DeltaCompiler::new();
                let binary = compiler.compile_sfx(&sequence);

                fs::write(&output_path, &binary)?;

                let (delay_y, delay_x) = ym_core::calculate_delay(sequence.source_hz);
                let yfi_path = output_path.with_extension("yfi");
                let yfi_contents = format!(
                    "; ca65 include generated by lym for {}\n\
                     MAX_FRAMES   = {}\n\
                     PLAYER_HZ    = {}\n\
                     MASTER_CLOCK = {}\n\
                     YM_DELAY     = {}\n\
                     YM_FINE      = {}\n",
                    input.display(),
                    sequence.frames.len(),
                    sequence.source_hz,
                    sequence.source_clock,
                    delay_y,
                    delay_x,
                );
                fs::write(&yfi_path, yfi_contents)?;

                println!(
                    "{} {} frames -> {} ({} bytes, {} Hz)",
                    style("RENDER SUCCESS:").bold().green(),
                    style(sequence.frames.len()).cyan(),
                    style(output_path.display()).cyan(),
                    style(binary.len()).cyan(),
                    style(sequence.source_hz).cyan()
                );
            }
            SfxCommands::Play {
                common:
                    SfxCommonArgs {
                        input,
                        hz,
                        clock,
                        index,
                    },
            } => {
                let mut sequence = load_sfx(&input, index)?;
                if let Some(c) = clock {
                    sequence.source_clock = c;
                }
                if let Some(hz_override) = hz {
                    sequence.source_hz = SystemHz::from(hz_override).hz_value();
                }

                println!(
                    "{} {} ({} Hz)...",
                    style("LOADING SFX:").bold().cyan(),
                    style(input.display()).cyan(),
                    sequence.source_hz
                );
                AudioPlayer::play_sfx(&sequence)?;
            }
        },

        MainCommands::Mix {
            song,
            sfx,
            channel,
            hz,
            clock,
        } => {
            println!(
                "{} Loading song {}...",
                style("LOADING SONG:").bold().cyan(),
                style(song.display()).cyan()
            );
            let mut song_seq = load_song(&song, clock)?;

            println!(
                "{} Loading sound effect bank...",
                style("LOADING SFX:").bold().cyan()
            );
            let sfx_list = load_all_sfx(&sfx)?;
            println!(
                "{} Loaded {} sound effect(s).",
                style("SFX BANK READY:").bold().green(),
                style(sfx_list.len()).cyan()
            );

            for (idx, sfx_item) in sfx_list.iter().enumerate().take(10) {
                let key_label = if idx == 0 {
                    "1 or SPACEBAR".to_string()
                } else if idx < 9 {
                    format!("{}", idx + 1)
                } else {
                    "0".to_string()
                };
                println!(
                    "  [{}] Key {}: {} ({} frames)",
                    style(idx).dim(),
                    style(key_label).yellow().bold(),
                    style(&sfx_item.name).cyan(),
                    sfx_item.frames.len()
                );
            }

            if let Some(hz_override) = hz {
                song_seq.timing.frame_rate = hz_override.into();
            }

            let song_hz = song_seq.timing.frame_rate.hz_value();
            let preferred_chan_idx = channel_arg_to_idx(channel);

            let host = cpal::default_host();
            let device = host
                .default_output_device()
                .ok_or("No output audio device found")?;
            let config = device.default_output_config()?;
            let sample_rate = config.sample_rate();
            let channels = config.channels() as usize;
            let sample_format = config.sample_format();
            let stream_config: cpal::StreamConfig = config.into();

            let sample_rate_u32: u32 = sample_rate;
            let samples_per_frame =
                (sample_rate_u32 as f64 / song_hz as f64).round().max(1.0) as usize;
            let mut chip = Ym2149::with_clocks(song_seq.timing.master_clock_hz, sample_rate_u32);

            let song_frames: Arc<[YmFrame]> = song_seq.frames.as_slice().into();
            let sfx_frames_list: Vec<Arc<[SfxFrame]>> = sfx_list
                .iter()
                .map(|s| s.frames.as_slice().into())
                .collect();
            let total_song_frames = song_frames.len();

            let mut mixer = 0x3F;
            let mut last_env_shape = None;

            if !song_frames.is_empty() {
                song_frames[0].apply_to_chip(&mut chip, &mut mixer, &mut last_env_shape);
            }

            let state = Arc::new(Mutex::new(MixerState {
                chip,
                song_frame_idx: 0,
                sample_in_frame: 0,
                mixer,
                last_env_shape,
                finished: false,
                active_sfx: [None, None, None],
            }));

            let current_frame_atomic = Arc::new(AtomicUsize::new(0));
            let finished_atomic = Arc::new(AtomicBool::new(false));

            let state_cb = Arc::clone(&state);
            let current_frame_cb = Arc::clone(&current_frame_atomic);
            let finished_cb = Arc::clone(&finished_atomic);
            let song_frames_cb = Arc::clone(&song_frames);
            let loop_start = song_seq.loop_start.or(Some(0));

            let err_fn = |err| eprintln!("{} {}", style("Audio stream error:").red().bold(), err);

            let stream = match sample_format {
                cpal::SampleFormat::F32 => device.build_output_stream(
                    stream_config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let mut s = state_cb.lock().unwrap_or_else(|e| e.into_inner());

                        if s.finished {
                            for sample in data.iter_mut() {
                                *sample = 0.0;
                            }
                            finished_cb.store(true, Ordering::Relaxed);
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
                                s.song_frame_idx += 1;

                                if s.song_frame_idx >= total_song_frames {
                                    if let Some(l_start) = loop_start {
                                        s.song_frame_idx = l_start;
                                    } else {
                                        s.finished = true;
                                        finished_cb.store(true, Ordering::Relaxed);
                                        return;
                                    }
                                }

                                let song_idx = s.song_frame_idx;
                                current_frame_cb.store(song_idx, Ordering::Relaxed);

                                let s_ref = &mut *s;
                                if let Some(sf) = song_frames_cb.get(song_idx) {
                                    sf.apply_to_chip(
                                        &mut s_ref.chip,
                                        &mut s_ref.mixer,
                                        &mut s_ref.last_env_shape,
                                    );
                                }

                                for ch in 0..3 {
                                    if let Some(ref mut active) = s_ref.active_sfx[ch] {
                                        if active.current_idx < active.frames.len() {
                                            let frame = &active.frames[active.current_idx];
                                            frame.apply_to_chip(
                                                &mut s_ref.chip,
                                                &mut s_ref.mixer,
                                                idx_to_ym_channel(ch),
                                            );
                                            active.current_idx += 1;
                                        } else {
                                            s_ref.active_sfx[ch] = None;
                                        }
                                    }
                                }
                            }
                        }
                    },
                    err_fn,
                    None,
                )?,
                _ => return Err("Unsupported audio sample format".into()),
            };

            stream.play()?;

            let pb = ProgressBar::new(total_song_frames as u64);
            pb.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] frame {pos}/{len} {msg}",
                )?
                    .progress_chars("=>-"),
            );

            pb.set_message(format!(
                " Press \u{2190}/\u{2192} to seek, 1-9/0/SPACE to trigger SFX, 'q' to quit (Primary Ch: {:?})",
                channel
            ));

            let key_rx = spawn_key_listener();

            loop {
                while let Ok(key) = key_rx.try_recv() {
                    if matches!(key, Key::Char('q') | Key::Char('Q')) {
                        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                        s.finished = true;
                        finished_atomic.store(true, Ordering::Relaxed);
                        break;
                    }

                    if matches!(key, Key::ArrowRight | Key::ArrowLeft) {
                        let step_frames = (song_hz as usize) * 5;
                        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                        let current_idx = s.song_frame_idx;
                        let target_idx = match key {
                            Key::ArrowRight => current_idx
                                .saturating_add(step_frames)
                                .min(total_song_frames.saturating_sub(1)),
                            Key::ArrowLeft => current_idx.saturating_sub(step_frames),
                            _ => current_idx,
                        };

                        let mut scratch_chip =
                            Ym2149::with_clocks(song_seq.timing.master_clock_hz, sample_rate_u32);
                        let mut mixer = 0x3F;
                        let mut last_env_shape = None;
                        for f in song_frames[..=target_idx].iter() {
                            f.apply_to_chip(&mut scratch_chip, &mut mixer, &mut last_env_shape);
                        }

                        s.chip = scratch_chip;
                        s.song_frame_idx = target_idx;
                        s.sample_in_frame = 0;
                        s.mixer = mixer;
                        s.last_env_shape = last_env_shape;
                        drop(s);
                        current_frame_atomic.store(target_idx, Ordering::Relaxed);
                    }

                    let sfx_trigger_idx: Option<usize> = match key {
                        Key::Char(' ') => Some(0),
                        Key::Char('1') => Some(0),
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
                    };

                    if let Some(sfx_idx) = sfx_trigger_idx {
                        if let Some(frames) = sfx_frames_list.get(sfx_idx) {
                            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                            let target_ch = if s.active_sfx[preferred_chan_idx].is_none() {
                                preferred_chan_idx
                            } else if s.active_sfx[2].is_none() {
                                2
                            } else if s.active_sfx[1].is_none() {
                                1
                            } else if s.active_sfx[0].is_none() {
                                0
                            } else {
                                preferred_chan_idx
                            };

                            s.active_sfx[target_ch] = Some(PlayingSfx {
                                frames: Arc::clone(frames),
                                current_idx: 0,
                            });
                        }
                    }
                }

                let current = current_frame_atomic.load(Ordering::Relaxed);
                pb.set_position(current as u64);

                if finished_atomic.load(Ordering::Relaxed) {
                    break;
                }

                std::thread::sleep(Duration::from_millis(15));
            }

            pb.finish_with_message("Playback finished.");
            let _ = std::process::Command::new("stty").arg("sane").status();
        }
    }

    Ok(())
}
