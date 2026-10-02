pub mod audio;

use audio::{AudioConfig, AudioPlayer};
use clap::{Args, Parser, Subcommand, ValueEnum};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ym_core::{
    CompressionLevel, DeltaCompiler, HzOption, SfxSequence, SystemHz, YmChannel, YmFrame,
    YmSequence,
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
        hz: Option<HzOptionArg>,

        /// Source chip clock in Hz (default: 2000000 for ST)
        #[arg(long)]
        clock: Option<u32>,

        /// Target chip clock in Hz to scale pitch for (default: 1789773 for
        /// Atari 7800; pass 2000000 to keep an Atari ST source at native pitch)
        #[arg(long)]
        target_clock: Option<u32>,
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

    /// Pattern frames per chunk (default: automatic optimal search across candidates)
    #[arg(long)]
    pattern_frames: Option<u8>,

    /// Deprecated: legacy target stream format.
    #[arg(long, value_enum, hide = true)]
    format: Option<SongFormatArg>,

    /// Deprecated: legacy compression level.
    #[arg(long, value_enum, hide = true)]
    compression: Option<CompressionArg>,

    /// Deprecated: legacy deduplication flag.
    #[arg(long, hide = true)]
    no_dedup: bool,

    /// Deprecated: legacy RLE flag.
    #[arg(long, hide = true)]
    no_rle: bool,

    /// Deprecated: legacy max bytes truncation.
    #[arg(long, hide = true)]
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum SongFormatArg {
    Ysg,
    Ycs,
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
            channel,
            hz,
            clock,
            target_clock,
        } => run_mix(&song, &sfx, channel, hz, clock, target_clock),
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

#[allow(clippy::too_many_lines)]
fn run_song_render(args: SongRenderArgs) -> Result<(), Box<dyn std::error::Error>> {
    let SongRenderArgs {
        input,
        output,
        hz,
        clock,
        target_clock,
        step,
        pattern_frames,
        ..
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

    let compiled = with_spinner("Compiling YSG song...", || {
        if let Some(pf) = pattern_frames {
            ym_core::compile_ysg(&sequence, pf)
        } else {
            ym_core::compile_ysg_optimal(&sequence)
        }
    })?;
    fs::write(&output_path, &compiled.bytes)?;
    write_ysi_include(input, name, &output_path, &sequence, &compiled)?;

    let final_hz = sequence.timing.frame_rate.hz_value();
    println!(
        "{} {} frames -> {} ({} bytes, pattern frames {}, seq len {}, {} Hz)",
        style("RENDER SUCCESS:").bold().green(),
        style(sequence.frames.len()).cyan(),
        style(output_path.display()).cyan(),
        style(compiled.bytes.len()).cyan(),
        style(compiled.pattern_frames).cyan(),
        style(compiled.seq_len).cyan(),
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
        let out_ext = output_path.extension().and_then(|e| e.to_str()).unwrap_or("ysg");
        print_size_comparison(original_size, compiled.bytes.len(), out_ext);
    }
    print_ysg_compression_report(&compiled);
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

/// Writes the `.ysi` ca65 include sidecar (frame/pattern counts, timing constants)
/// for a compiled `.ysg` song alongside `output_path`.
fn write_ysi_include(
    input: &Path,
    name: &str,
    output_path: &Path,
    sequence: &YmSequence,
    details: &ym_core::YsgSongDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    let final_hz = sequence.timing.frame_rate.hz_value();
    let (delay_y, delay_x) = ym_core::calculate_delay(final_hz);
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
             MAX_FRAMES     = {}\n\
             PLAYER_HZ      = {}\n\
             MASTER_CLOCK   = {}\n\
             YM_DELAY       = {}\n\
             YM_FINE        = {}\n\
             PATTERN_FRAMES = {}\n\
             SEQ_LEN        = {}\n\
             TOTAL_BYTES    = {}\n\
         .endscope\n",
        input.display(),
        scope_name,
        sequence.frames.len(),
        final_hz,
        sequence.timing.master_clock_hz,
        delay_y,
        delay_x,
        details.pattern_frames,
        details.seq_len,
        details.bytes.len(),
    );
    fs::write(&ysi_path, &ysi_contents)?;

    // If caller explicitly requested .ycs output, also emit .yci for compatibility
    if output_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ycs"))
    {
        let yci_path = output_path.with_extension("yci");
        fs::write(yci_path, ysi_contents)?;
    }

    Ok(())
}

/// Prints a diagnostic compression report showing track layout and unique pattern counts.
fn print_ysg_compression_report(details: &ym_core::YsgSongDetails) {
    println!("\n{}", style("=== YSG COMPRESSION REPORT ===").bold());
    println!(
        "Total Stream Size: {} bytes (pattern frames {}, seq len {})",
        style(details.bytes.len()).cyan(),
        style(details.pattern_frames).cyan(),
        style(details.seq_len).cyan()
    );
    println!(
        "Track Layout:      Track A: {} B ({} uniq) | Track B: {} B ({} uniq) | Track C: {} B ({} uniq) | Global: {} B ({} uniq)",
        details.track_a_bytes, details.unique_patterns_a,
        details.track_b_bytes, details.unique_patterns_b,
        details.track_c_bytes, details.unique_patterns_c,
        details.track_global_bytes, details.unique_patterns_global,
    );
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



/// Prints the size delta between the original `.ym` chiptune and the compiled output stream.
fn print_size_comparison(original_size: usize, new_size: usize, format_ext: &str) {
    let orig_f64 = usize_to_f64(original_size);
    let new_f64 = usize_to_f64(new_size);
    let pct_change = if original_size > 0 {
        100.0 * (orig_f64 - new_f64) / orig_f64
    } else {
        0.0
    };
    println!(
        "{} {} bytes (uncompressed .ym) -> {} bytes (.{}) ({:.1}% change)",
        style("SIZE:").bold(),
        style(original_size).cyan(),
        style(new_size).cyan(),
        format_ext,
        pct_change
    );
}

fn run_song_play(
    input: &Path,
    hz: Option<HzOptionArg>,
    via_sequence: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    if extension == "json" || extension == "ysg" || extension == "ycs" {
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

    println!(
        "{} Loading sound effect bank...",
        style("LOADING SFX:").bold().cyan()
    );
    let sfx_list = load_all_sfx(sfx)?;
    println!(
        "{} Loaded {} sound effect(s).",
        style("SFX BANK READY:").bold().green(),
        style(sfx_list.len()).cyan()
    );

    if let Some(hz_override) = hz {
        song_seq.timing.frame_rate = HzOption::from(hz_override).into();
    }

    AudioPlayer::play_mix(
        &song_seq,
        &sfx_list,
        channel.into(),
        &AudioConfig::default(),
    )?;
    Ok(())
}
