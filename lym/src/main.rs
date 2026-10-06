pub mod audio;

use audio::{AudioConfig, AudioPlayer};
use clap::{Args, Parser, Subcommand, ValueEnum};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ym_core::traits::{SfxFile, SongFile};
use ym_core::{
    HzOption, SfxSequence, SongContainer, SystemHz, YfxFile, YmChannel, YmSequence, YsgFile,
    YsgSongDetails,
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

    /// Truncate the song (dropping whole patterns from the end) until the compiled
    /// .ysg fits within N bytes
    #[arg(long, value_name = "N")]
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

        /// Use raw YM replayer without interactive seeking controls
        #[arg(long)]
        raw: bool,
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
            SongCommands::Play { input, hz, raw } => run_song_play(&input, hz, raw),
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
    sequence.decimate(step);
    apply_frame_rate(&mut sequence, hz.map(Into::into), step);

    let original_frames = sequence.frames.len();
    let compile = |seq: &YmSequence| match pattern_frames {
        Some(pf) => YsgFile::from_sequence(seq, pf),
        None => YsgFile::from_sequence_optimal(seq),
    };
    let (ysg_file, compiled) = with_spinner("Compiling YSG song...", || {
        match (compile(&sequence), max_bytes) {
            // Too large for the format itself: with --max-bytes, cut the song down to
            // the longest prefix that compiles, then let shrink_to_max_bytes trim further.
            (Err(_), Some(_)) => compile_longest_prefix(&mut sequence, compile),
            (result, _) => result,
        }
    })?;
    let (ysg_file, compiled) = match max_bytes {
        Some(limit) => {
            shrink_to_max_bytes(&mut sequence, ysg_file, compiled, limit, original_frames)?
        }
        None => (ysg_file, compiled),
    };
    fs::write(&output_path, ysg_file.to_bytes())?;
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
        let out_ext = output_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("ysg");
        print_size_comparison(original_size, compiled.bytes.len(), out_ext);
    }
    print_ysg_compression_report(&compiled);
    Ok(())
}

/// Binary-searches for the longest prefix of `sequence` that `compile` accepts and
/// truncates the sequence to it, returning that compile result.
fn compile_longest_prefix(
    sequence: &mut YmSequence,
    compile: impl Fn(&YmSequence) -> Result<(YsgFile, YsgSongDetails), Box<dyn std::error::Error>>,
) -> Result<(YsgFile, YsgSongDetails), Box<dyn std::error::Error>> {
    let mut best = None;
    let (mut lo, mut hi) = (1, sequence.frames.len());
    let mut trial = sequence.clone();
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        trial.frames.clear();
        trial.frames.extend_from_slice(&sequence.frames[..mid]);
        if let Ok(result) = compile(&trial) {
            best = Some((mid, result));
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    let (len, result) = best.ok_or("Song cannot be compiled even after truncation")?;
    sequence.frames.truncate(len);
    Ok(result)
}

/// Repeatedly drops the last pattern step and recompiles (keeping the pattern size of
/// the initial compile) until the song fits within `limit` bytes, warning about how many
/// frames were truncated in the process.
fn shrink_to_max_bytes(
    sequence: &mut YmSequence,
    mut ysg_file: YsgFile,
    mut compiled: YsgSongDetails,
    limit: usize,
    original_frames: usize,
) -> Result<(YsgFile, YsgSongDetails), Box<dyn std::error::Error>> {
    if compiled.bytes.len() <= limit && sequence.frames.len() == original_frames {
        return Ok((ysg_file, compiled));
    }

    let pattern_frames = compiled.pattern_frames;
    let pat_len = usize::from(pattern_frames);
    with_spinner("Truncating to fit --max-bytes...", || {
        while compiled.bytes.len() > limit {
            let steps = sequence.frames.len().div_ceil(pat_len);
            if steps <= 1 {
                return Err(format!(
                    "Cannot fit even one pattern within --max-bytes {limit} \
                     (smallest is {} bytes)",
                    compiled.bytes.len()
                )
                .into());
            }
            sequence.frames.truncate((steps - 1) * pat_len);
            (ysg_file, compiled) = YsgFile::from_sequence(sequence, pattern_frames)?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;

    println!(
        "{} truncated {} frames to fit within {} bytes",
        style("WARNING:").bold().yellow(),
        style(original_frames - sequence.frames.len()).yellow(),
        style(limit).yellow(),
    );
    Ok((ysg_file, compiled))
}

/// Writes the `.ysi` ca65 include sidecar (frame/pattern counts, timing constants)
/// for a compiled `.ysg` song alongside `output_path`.
fn write_ysi_include(
    input: &Path,
    name: &str,
    output_path: &Path,
    sequence: &YmSequence,
    details: &YsgSongDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    let final_hz = sequence.timing.frame_rate.hz_value();
    let (delay_y, delay_x) = sequence.timing.frame_rate.calculate_delay();
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

    Ok(())
}

/// Prints a diagnostic compression report showing track layout and unique pattern counts.
fn print_ysg_compression_report(details: &YsgSongDetails) {
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
    let bytes = fs::read(input)?;
    let container = with_spinner("Decoding song...", || {
        SongContainer::from_bytes(name, extension, &bytes, clock)
    })?;

    let (digidrum_frames, original_ym_size) = match &container {
        SongContainer::Ym(ym) => (ym.digidrum_frames, Some(ym.uncompressed_len)),
        SongContainer::Ysg(_) => (0, Some(bytes.len())),
        SongContainer::Json(_) => (0, None),
    };

    let sequence = container.to_sequence(name, target_clock)?;
    Ok((sequence, digidrum_frames, original_ym_size))
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
    raw: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    if raw && extension.eq_ignore_ascii_case("ym") {
        println!(
            "{} {} (raw YM replayer mode)...",
            style("LOADING:").bold().cyan(),
            style(input.display()).cyan()
        );
        let ym_data = fs::read(input)?;
        AudioPlayer::play_ym_data(&ym_data, &AudioConfig::default())?;
    } else {
        let mut sequence = YmSequence::load_from_path(input, None, None)?;
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
    let mut sequence = SfxSequence::load_from_path(&args.input, args.index)?;
    if let Some(c) = args.clock {
        sequence.source_clock = c;
    }
    if let Some(hz_override) = args.hz {
        sequence.source_hz = SystemHz::from(HzOption::from(hz_override)).hz_value();
    }

    let yfx_file = YfxFile::from_sequence(&sequence);
    let binary = yfx_file.to_bytes();

    fs::write(&output_path, &binary)?;

    let (delay_y, delay_x) = SystemHz::calculate_delay_for_hz(sequence.source_hz);
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
    let mut sequence = SfxSequence::load_from_path(&args.input, args.index)?;
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
    let mut song_seq = YmSequence::load_from_path(song, clock, target_clock)?;

    println!(
        "{} Loading sound effect bank...",
        style("LOADING SFX:").bold().cyan()
    );
    let sfx_list = SfxSequence::load_all_from_paths(sfx)?;
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
