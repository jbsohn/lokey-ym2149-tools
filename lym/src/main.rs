pub mod audio;

use audio::{AudioConfig, AudioPlayer};
use clap::{Args, Parser, Subcommand, ValueEnum};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ym_core::{
    CompilerOptions, CompressionLevel, DeltaCompiler, HzOption, SfxSequence, SystemHz, YmChannel,
    YmFrame, YmSequence, YmSongDetails,
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
    /// Real-time interactive music & sound effect keyboard mixer (supports YM2149 and TIA hybrid mixing)
    Mix {
        /// Input background song file (.ysg, .ym, .json)
        #[arg(short, long)]
        song: PathBuf,

        /// One or more input YM-2149 sound effect files or banks (.yfx, .json, .csv, .afx, .afb)
        #[arg(short = 'e', long, num_args = 0..)]
        sfx: Vec<PathBuf>,

        /// One or more input Atari TIA sound effect files (.tfx, .json, .csv)
        #[arg(short = 't', long = "tia-sfx", num_args = 0..)]
        tia_sfx: Vec<PathBuf>,

        /// Preferred primary channel on which to play YM SFX (A, B, or C)
        #[arg(short, long, value_enum, default_value = "c")]
        channel: ChannelArg,

        /// Timing refresh rate override (50 or 60 Hz)
        #[arg(long, value_enum)]
        hz: Option<HzOptionArg>,

        /// Source chip clock in Hz (default: 2000000 for ST)
        #[arg(long)]
        clock: Option<u32>,

        /// Target chip clock in Hz to scale pitch for (default: 1789773 for
        /// Atari 7800; pass 2000000 to keep an Atari ST source at native pitch)
        #[arg(long)]
        target_clock: Option<u32>,
    },
    /// Atari TIA sound chip tools (compile, play SFX and songs)
    Tia {
        #[command(subcommand)]
        command: TiaCommands,
    },
}

#[derive(Subcommand, Debug)]
enum TiaCommands {
    /// Sound effect tools (render .tfx, play)
    Sfx {
        #[command(subcommand)]
        command: TiaSfxCommands,
    },
    /// Music song tools (render .tsg, play)
    Song {
        #[command(subcommand)]
        command: TiaSongCommands,
    },
}

#[derive(Subcommand, Debug)]
enum TiaSfxCommands {
    /// Render a TIA sound effect (.json, .csv) into compiled 3-byte binary stream (.tfx)
    Render {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Audition and play a TIA sound effect sequence (.tfx, .json, .csv)
    Play {
        #[arg(short, long)]
        input: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum TiaSongCommands {
    /// Render a TIA song sequence (.json) into compiled .tsg binary stream
    Render {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "full")]
        compression: CompressionArg,
    },
    /// Audition and play a TIA song sequence (.tsg, .json)
    Play {
        #[arg(short, long)]
        input: PathBuf,
    },
}

// --- SONG SUBCOMMANDS ---

#[derive(Args, Debug)]
struct SongRenderArgs {
    #[arg(short, long)]
    input: PathBuf,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long, value_enum)]
    hz: Option<HzOptionArg>,

    #[arg(long)]
    clock: Option<u32>,

    /// Target chip clock in Hz that pitches are retuned for (default: 1789773,
    /// the Atari 7800's YM-2149 clock). Pass 2000000 to keep an Atari ST source
    /// at native pitch. Real Apple II Mockingboard hardware (and `AppleWin`)
    /// clocks the AY-3-8910 from the 6502 clock (~1020484 Hz); pass
    /// --target-clock 1020484 when rendering for that platform, or notes
    /// will play back roughly an octave flat.
    #[arg(long)]
    target_clock: Option<u32>,

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
}

#[derive(Subcommand, Debug)]
enum SongCommands {
    /// Render a music song file into compiled YM-2149 binary stream (.ysg)
    Render {
        #[command(flatten)]
        args: SongRenderArgs,
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
        hz: Option<HzOptionArg>,

        #[arg(long)]
        via_sequence: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy)]
enum HzOptionArg {
    #[value(name = "50")]
    Hz50,
    #[value(name = "60")]
    Hz60,
}

impl From<HzOptionArg> for HzOption {
    fn from(opt: HzOptionArg) -> Self {
        match opt {
            HzOptionArg::Hz50 => HzOption::Hz50,
            HzOptionArg::Hz60 => HzOption::Hz60,
        }
    }
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

impl From<CompressionArg> for tia_core::CompressionLevel {
    fn from(a: CompressionArg) -> Self {
        match a {
            CompressionArg::Full => tia_core::CompressionLevel::Full,
            CompressionArg::DeltaOnly => tia_core::CompressionLevel::DeltaOnly,
            CompressionArg::None => tia_core::CompressionLevel::None,
        }
    }
}

// --- SFX SUBCOMMANDS ---

#[derive(Args, Debug)]
struct SfxCommonArgs {
    #[arg(short, long)]
    input: PathBuf,

    #[arg(long, value_enum)]
    hz: Option<HzOptionArg>,

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

fn f64_to_u32(val: f64) -> u32 {
    val as u32
}

fn usize_to_f64(val: usize) -> f64 {
    val as f64
}

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
    target_clock_override: Option<u32>,
) -> Result<YmSequence, Box<dyn std::error::Error>> {
    YmSequence::load_from_path(input, clock_override, target_clock_override)
}

fn load_sfx(input: &Path, bank_index: usize) -> Result<SfxSequence, Box<dyn std::error::Error>> {
    SfxSequence::load_from_path(input, bank_index)
}

fn load_all_sfx(inputs: &[PathBuf]) -> Result<Vec<SfxSequence>, Box<dyn std::error::Error>> {
    SfxSequence::load_all_from_paths(inputs)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = LymCli::parse();

    match cli.command {
        MainCommands::Song { command } => match command {
            SongCommands::Dump {
                input,
                frames,
                start,
            } => run_song_dump(&input, frames, start),
            SongCommands::Render { args } => run_song_render(args),
            SongCommands::Play {
                input,
                hz,
                via_sequence,
            } => run_song_play(&input, hz, via_sequence),
        },
        MainCommands::Sfx { command } => match command {
            SfxCommands::Render { common, output } => run_sfx_render(&common, output),
            SfxCommands::Play { common } => run_sfx_play(&common),
        },
        MainCommands::Mix {
            song,
            sfx,
            tia_sfx,
            channel,
            hz,
            clock,
            target_clock,
        } => run_mix(&song, &sfx, &tia_sfx, channel, hz, clock, target_clock),
        MainCommands::Tia { command } => match command {
            TiaCommands::Sfx { command } => match command {
                TiaSfxCommands::Render { input, output } => run_tia_sfx_render(&input, output),
                TiaSfxCommands::Play { input } => run_tia_sfx_play(&input),
            },
            TiaCommands::Song { command } => match command {
                TiaSongCommands::Render {
                    input,
                    output,
                    compression,
                } => run_tia_song_render(&input, output, compression),
                TiaSongCommands::Play { input } => run_tia_song_play(&input),
            },
        },
    }
}

fn run_song_dump(
    input: &Path,
    frames: usize,
    start: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let sequence = with_spinner("Decoding...", || {
        YmSequence::load_from_path(input, None, None)
    })?;

    let end = (start + frames).min(sequence.frames.len());
    println!(
        "{:>6}  {:>6} {:>6} {:>6}  {:>4}  {:>4}  {:>3} {:>3} {:>3}  {:>4} {:>4} {:>4}  {:>4}  {:>4}  R13",
        "frame", "toneA", "toneB", "toneC", "volA", "volB", "volC",
        "teA", "teB", "teC", "neA", "neB", "neC", "envP"
    );
    for (i, f) in sequence.frames[start..end].iter().enumerate() {
        let r13 = match f.envelope_shape {
            Some(v) => format!("{v}"),
            None => "---".to_string(),
        };
        println!(
            "{:>6}  {:>6} {:>6} {:>6}  {:>4}  {:>4}  {:>3} {:>3} {:>3}  {:>4} {:>4} {:>4}  {:>4}  {:>4}  {}",
            start + i,
            f.tone_a.map_or_else(|| "-".into(), |v| v.to_string()),
            f.tone_b.map_or_else(|| "-".into(), |v| v.to_string()),
            f.tone_c.map_or_else(|| "-".into(), |v| v.to_string()),
            f.volume_a.map_or_else(|| "-".into(), |v| v.to_string()),
            f.volume_b.map_or_else(|| "-".into(), |v| v.to_string()),
            f.volume_c.map_or_else(|| "-".into(), |v| v.to_string()),
            f.tone_enable_a.map_or("-", |v| if v { "T" } else { "f" }),
            f.tone_enable_b.map_or("-", |v| if v { "T" } else { "f" }),
            f.tone_enable_c.map_or("-", |v| if v { "T" } else { "f" }),
            f.noise_enable_a.map_or("-", |v| if v { "T" } else { "f" }),
            f.noise_enable_b.map_or("-", |v| if v { "T" } else { "f" }),
            f.noise_enable_c.map_or("-", |v| if v { "T" } else { "f" }),
            f.envelope_period.map_or_else(|| "-".into(), |v| v.to_string()),
            r13,
        );
    }
    println!("\nTotal frames: {}", sequence.frames.len());
    Ok(())
}

fn run_song_render(args: SongRenderArgs) -> Result<(), Box<dyn std::error::Error>> {
    let SongRenderArgs {
        input,
        output,
        hz,
        clock,
        target_clock,
        step,
        compression,
        no_dedup,
        no_rle,
        max_bytes,
    } = args;
    let input = input.as_path();
    let output_path = output.unwrap_or_else(|| {
        let mut path = input.to_path_buf();
        path.set_extension("ysg");
        path
    });
    let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");
    let (mut sequence, digidrum_frames, original_ym_size) =
        decode_song_input(input, extension, name, clock, target_clock)?;
    let step = step.max(1);
    println!(
        "{} {}...",
        style("LOADING:").bold().cyan(),
        style(input.display()).cyan()
    );
    sequence.frames = decimate_frames(&sequence.frames, step);
    apply_frame_rate(&mut sequence, hz.map(Into::into), step);
    let compiler = DeltaCompiler::new();
    let compression_level: CompressionLevel = compression.into();
    let compiler_options = CompilerOptions {
        dedup: !no_dedup,
        rle: !no_rle,
        ..CompilerOptions::default()
    };
    let mut compiled_song = with_spinner("Compiling song...", || {
        compiler.compile_song(&sequence, compression_level, &compiler_options)
    })?;
    if let Some(limit) = max_bytes {
        compiled_song = shrink_to_max_bytes(
            &mut sequence,
            &compiler,
            compression_level,
            &compiler_options,
            compiled_song,
            limit,
        )?;
    }

    fs::write(&output_path, &compiled_song.bytes)?;
    write_ysi_include(input, name, &output_path, &sequence, &compiled_song)?;

    let final_hz = sequence.timing.frame_rate.hz_value();
    println!(
        "{} {} frames -> {} ({} bytes, pattern size {}, {} Hz)",
        style("RENDER SUCCESS:").bold().green(),
        style(sequence.frames.len()).cyan(),
        style(output_path.display()).cyan(),
        style(compiled_song.bytes.len()).cyan(),
        style(compiled_song.pattern_size).cyan(),
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
        print_size_comparison(original_size, compiled_song.bytes.len());
    }
    Ok(())
}

/// Decimates `frames` by taking one output frame per `step`-sized window. For each
/// of the three channels independently, the loudest frame in the window is picked
/// and its tone/volume/enable state copied into the output frame, so that brief
/// transients louder than their neighbors survive decimation instead of being
/// skipped outright.
fn decimate_frames(frames: &[YmFrame], step: usize) -> Vec<YmFrame> {
    let limit = frames.len();
    let mut decimated_frames = Vec::new();
    let mut i = 0;
    while i < limit {
        let window_end = (i + step).min(limit);
        let mut best_idx_a = i;
        let mut best_vol_a = frames[i].volume_a.unwrap_or(0);
        let mut best_idx_b = i;
        let mut best_vol_b = frames[i].volume_b.unwrap_or(0);
        let mut best_idx_c = i;
        let mut best_vol_c = frames[i].volume_c.unwrap_or(0);

        for (idx, f) in frames.iter().enumerate().take(window_end).skip(i) {
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

        let mut final_frame = frames[i].clone();
        final_frame.volume_a = frames[best_idx_a].volume_a;
        final_frame.tone_a = frames[best_idx_a].tone_a;
        final_frame.tone_enable_a = frames[best_idx_a].tone_enable_a;
        final_frame.noise_enable_a = frames[best_idx_a].noise_enable_a;

        final_frame.volume_b = frames[best_idx_b].volume_b;
        final_frame.tone_b = frames[best_idx_b].tone_b;
        final_frame.tone_enable_b = frames[best_idx_b].tone_enable_b;
        final_frame.noise_enable_b = frames[best_idx_b].noise_enable_b;

        final_frame.volume_c = frames[best_idx_c].volume_c;
        final_frame.tone_c = frames[best_idx_c].tone_c;
        final_frame.tone_enable_c = frames[best_idx_c].tone_enable_c;
        final_frame.noise_enable_c = frames[best_idx_c].noise_enable_c;

        final_frame.noise_period = frames[dominant_idx].noise_period;
        final_frame.envelope_period = frames[dominant_idx].envelope_period;
        final_frame.envelope_shape = frames[dominant_idx].envelope_shape;

        decimated_frames.push(final_frame);
        i += step;
    }
    decimated_frames
}

/// Sanitizes a name into a valid ca65 `.scope` identifier by replacing any
/// character that isn't alphanumeric or `_` with `_`.
fn ca65_scope_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Writes the `.ysi` ca65 include sidecar (frame/pattern counts, timing constants)
/// for a compiled `.ysg` song alongside `output_path`.
fn write_ysi_include(
    input: &Path,
    name: &str,
    output_path: &Path,
    sequence: &YmSequence,
    compiled_song: &YmSongDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    let final_hz = sequence.timing.frame_rate.hz_value();
    let (delay_y, delay_x) = ym_core::calculate_delay(final_hz);
    let num_patterns = compiled_song.bytes.get(1).copied().unwrap_or(0);
    let seq_len = compiled_song.bytes.get(2).copied().unwrap_or(0);
    let ysi_path = output_path.with_extension("ysi");
    let scope_name = ca65_scope_name(name);
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
        compiled_song.pattern_size,
        num_patterns,
        seq_len,
    );
    fs::write(ysi_path, ysi_contents)?;
    Ok(())
}

/// Decodes a song source (`.ym` chiptune or `.json` sequence) into a `YmSequence`.
/// Returns the sequence, the number of YM6 digi-drum frames silenced, and (for `.ym`
/// input) the original decompressed byte size for later size-comparison reporting.
fn decode_song_input(
    input: &Path,
    extension: &str,
    name: &str,
    clock: Option<u32>,
    target_clock: Option<u32>,
) -> Result<(YmSequence, usize, Option<usize>), Box<dyn std::error::Error>> {
    if extension.eq_ignore_ascii_case("ym") {
        let bytes = fs::read(input)?;
        let original_ym_size = Some(YmSequence::ym_decompressed_len(&bytes)?);
        let (sequence, digidrum_frames) = with_spinner("Decoding YM chiptune...", || {
            YmSequence::from_ym_data(name, &bytes, clock, target_clock)
        })?;
        Ok((sequence, digidrum_frames, original_ym_size))
    } else {
        let content = fs::read_to_string(input)?;
        let sequence = serde_json::from_str(&content)?;
        Ok((sequence, 0, None))
    }
}

/// Applies an explicit `--hz` override, or (when frame-decimating via `--step`)
/// rescales the frame rate down to match the reduced frame count.
fn apply_frame_rate(sequence: &mut YmSequence, hz: Option<HzOption>, step: usize) {
    if let Some(hz_override) = hz {
        sequence.timing.frame_rate = hz_override.into();
    } else if step > 1 {
        let current_hz = sequence.timing.frame_rate.hz_value();
        let decimated_hz = f64_to_u32(
            (f64::from(current_hz) / usize_to_f64(step))
                .round()
                .max(1.0),
        );
        sequence.timing.frame_rate = SystemHz::Custom(decimated_hz);
    }
}

/// Repeatedly drops the last pattern and recompiles until the song fits within
/// `limit` bytes, warning about how many frames were truncated in the process.
fn shrink_to_max_bytes(
    sequence: &mut YmSequence,
    compiler: &DeltaCompiler,
    compression_level: CompressionLevel,
    compiler_options: &CompilerOptions,
    mut compiled_song: YmSongDetails,
    limit: usize,
) -> Result<YmSongDetails, Box<dyn std::error::Error>> {
    if compiled_song.bytes.len() <= limit {
        return Ok(compiled_song);
    }

    let original_frames = sequence.frames.len();
    loop {
        let pattern_size = compiled_song.pattern_size;
        let current_patterns = sequence.frames.len() / pattern_size;
        if current_patterns == 0 {
            return Err("Cannot fit even one pattern within --max-bytes limit".into());
        }
        sequence
            .frames
            .truncate((current_patterns - 1) * pattern_size);
        compiled_song = compiler.compile_song(sequence, compression_level, compiler_options)?;
        if compiled_song.bytes.len() <= limit {
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
    Ok(compiled_song)
}

/// Prints the size delta between the original `.ym` chiptune and the compiled `.ysg`.
fn print_size_comparison(original_size: usize, new_size: usize) {
    let orig_f64 = usize_to_f64(original_size);
    let new_f64 = usize_to_f64(new_size);
    let pct_change = if original_size > 0 {
        100.0 * (orig_f64 - new_f64) / orig_f64
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

fn run_song_play(
    input: &Path,
    hz: Option<HzOptionArg>,
    via_sequence: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    if extension == "json" || extension == "ysg" {
        let mut sequence = load_song(input, None, None)?;
        if let Some(hz_override) = hz {
            sequence.timing.frame_rate = HzOption::from(hz_override).into();
        }
        println!(
            "{} {} ({} Hz)...",
            style("LOADING:").bold().cyan(),
            style(input.display()).cyan(),
            sequence.timing.frame_rate.hz_value()
        );
        AudioPlayer::play_song(&sequence, &AudioConfig::default())?;
    } else if via_sequence && extension.eq_ignore_ascii_case("ym") {
        let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");
        let ym_data = fs::read(input)?;
        let (mut sequence, _) = with_spinner("Decoding YM via YmSequence pipeline...", || {
            YmSequence::from_ym_data(name, &ym_data, None, None)
        })?;
        if let Some(hz_override) = hz {
            sequence.timing.frame_rate = HzOption::from(hz_override).into();
        }
        println!(
            "{} {} ({} frames @ {} Hz)...",
            style("LOADING:").bold().cyan(),
            style(input.display()).cyan(),
            sequence.frames.len(),
            sequence.timing.frame_rate.hz_value()
        );
        AudioPlayer::play_song(&sequence, &AudioConfig::default())?;
    } else {
        println!(
            "{} {}...",
            style("LOADING:").bold().cyan(),
            style(input.display()).cyan()
        );
        let ym_data = fs::read(input)?;
        AudioPlayer::play_ym_data(&ym_data, &AudioConfig::default())?;
    }

    Ok(())
}

fn run_sfx_render(
    args: &SfxCommonArgs,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_path = output.unwrap_or_else(|| {
        let mut path = args.input.clone();
        path.set_extension("yfx");
        path
    });

    println!(
        "{} {}...",
        style("LOADING SFX:").bold().cyan(),
        style(args.input.display()).cyan()
    );
    let mut sequence = load_sfx(&args.input, args.index)?;
    if let Some(c) = args.clock {
        sequence.source_clock = c;
    }
    if let Some(hz_override) = args.hz {
        sequence.source_hz = SystemHz::from(HzOption::from(hz_override)).hz_value();
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
        args.input.display(),
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

    Ok(())
}

fn run_sfx_play(args: &SfxCommonArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut sequence = load_sfx(&args.input, args.index)?;
    if let Some(c) = args.clock {
        sequence.source_clock = c;
    }
    if let Some(hz_override) = args.hz {
        sequence.source_hz = SystemHz::from(HzOption::from(hz_override)).hz_value();
    }

    println!(
        "{} {} ({} Hz)...",
        style("LOADING SFX:").bold().cyan(),
        style(args.input.display()).cyan(),
        sequence.source_hz
    );
    AudioPlayer::play_sfx(&sequence, &AudioConfig::default())?;
    Ok(())
}

fn run_mix(
    song: &Path,
    sfx: &[PathBuf],
    tia_sfx: &[PathBuf],
    channel: ChannelArg,
    hz: Option<HzOptionArg>,
    clock: Option<u32>,
    target_clock: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{} Loading song {}...",
        style("LOADING SONG:").bold().cyan(),
        style(song.display()).cyan()
    );
    let mut song_seq = load_song(song, clock, target_clock)?;

    let ym_sfx_list = if sfx.is_empty() {
        Vec::new()
    } else {
        println!(
            "{} Loading YM sound effect bank...",
            style("LOADING YM SFX:").bold().cyan()
        );
        let list = load_all_sfx(sfx)?;
        println!(
            "{} Loaded {} YM sound effect(s).",
            style("YM SFX READY:").bold().green(),
            style(list.len()).cyan()
        );
        list
    };

    let tia_sfx_list = if tia_sfx.is_empty() {
        Vec::new()
    } else {
        println!(
            "{} Loading TIA sound effect bank...",
            style("LOADING TIA SFX:").bold().cyan()
        );
        let mut list = Vec::new();
        for path in tia_sfx {
            list.push(tia_core::TiaSfxSequence::from_file(path)?);
        }
        println!(
            "{} Loaded {} TIA sound effect(s).",
            style("TIA SFX READY:").bold().green(),
            style(list.len()).cyan()
        );
        list
    };

    if ym_sfx_list.is_empty() && tia_sfx_list.is_empty() {
        return Err(
            "Please provide at least one YM SFX (--sfx / -e) or TIA SFX (--tia-sfx / -t)".into(),
        );
    }

    if let Some(hz_override) = hz {
        song_seq.timing.frame_rate = HzOption::from(hz_override).into();
    }

    AudioPlayer::play_hybrid_mix(
        &song_seq,
        &ym_sfx_list,
        &tia_sfx_list,
        channel.into(),
        &AudioConfig::default(),
    )?;
    Ok(())
}

fn run_tia_sfx_render(
    input: &Path,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let sequence = tia_core::TiaSfxSequence::from_file(input)?;
    let output_path = output.unwrap_or_else(|| input.with_extension("tfx"));
    let compiler = tia_core::DeltaCompiler::new();
    let binary = compiler.compile_sfx(&sequence);

    fs::write(&output_path, &binary)?;

    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("sfx");
    let tfi_path = output_path.with_extension("tfi");
    let scope_name = ca65_scope_name(stem);

    let (delay_y, delay_x) = tia_core::calculate_delay(sequence.source_hz);
    let tfi_contents = format!(
        "; ca65 include generated by lym for {}\n\
         .scope {}\n\
             NUM_FRAMES   = {}\n\
             PLAYER_HZ    = {}\n\
             SOURCE_CLOCK = {}\n\
             TIA_DELAY    = {}\n\
             TIA_FINE     = {}\n\
         .endscope\n",
        input.display(),
        scope_name,
        sequence.frames.len(),
        sequence.source_hz,
        sequence.source_clock,
        delay_y,
        delay_x,
    );
    fs::write(&tfi_path, tfi_contents)?;

    println!(
        "{} {} frames -> {} ({} bytes, {} Hz)",
        style("RENDER SUCCESS:").bold().green(),
        style(sequence.frames.len()).cyan(),
        style(output_path.display()).cyan(),
        style(binary.len()).cyan(),
        style(sequence.source_hz).cyan()
    );

    Ok(())
}

fn run_tia_sfx_play(input: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let sequence = tia_core::TiaSfxSequence::from_file(input)?;
    println!(
        "{} {} ({} Hz)...",
        style("LOADING TIA SFX:").bold().cyan(),
        style(input.display()).cyan(),
        sequence.source_hz
    );
    AudioPlayer::play_tia_sfx(&sequence, &AudioConfig::default())?;
    Ok(())
}

fn run_tia_song_render(
    input: &Path,
    output: Option<PathBuf>,
    compression: CompressionArg,
) -> Result<(), Box<dyn std::error::Error>> {
    let sequence = tia_core::TiaSequence::from_file(input)?;
    let output_path = output.unwrap_or_else(|| input.with_extension("tsg"));
    let compiler = tia_core::DeltaCompiler::new();
    let details = compiler.compile_song(
        &sequence,
        compression.into(),
        &tia_core::CompilerOptions::default(),
    )?;

    fs::write(&output_path, &details.bytes)?;

    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");
    let tsi_path = output_path.with_extension("tsi");
    let scope_name = ca65_scope_name(stem);

    let hz_val = sequence.timing.frame_rate.hz_value();
    let (delay_y, delay_x) = tia_core::calculate_delay(hz_val);
    let tsi_contents = format!(
        "; ca65 include generated by lym for {}\n\
         .scope {}\n\
             MAX_FRAMES   = {}\n\
             PLAYER_HZ    = {}\n\
             MASTER_CLOCK = {}\n\
             TIA_DELAY    = {}\n\
             TIA_FINE     = {}\n\
             PATTERN_SIZE = {}\n\
         .endscope\n",
        input.display(),
        scope_name,
        sequence.frames.len(),
        hz_val,
        sequence.timing.master_clock_hz,
        delay_y,
        delay_x,
        details.pattern_size,
    );
    fs::write(&tsi_path, tsi_contents)?;

    println!(
        "{} {} frames -> {} ({} bytes, pattern size: {})",
        style("RENDER SUCCESS:").bold().green(),
        style(sequence.frames.len()).cyan(),
        style(output_path.display()).cyan(),
        style(details.compiled_bytes).cyan(),
        style(details.pattern_size).cyan()
    );

    Ok(())
}

fn run_tia_song_play(input: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let sequence = tia_core::TiaSequence::from_file(input)?;
    AudioPlayer::play_tia_song(&sequence, &AudioConfig::default())?;
    Ok(())
}
