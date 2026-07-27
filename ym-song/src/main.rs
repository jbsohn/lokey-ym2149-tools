use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use ym_core::{
    AudioPlayer, CompilerOptions, CompressionLevel, DeltaCompiler, HzOption, SystemHz, YmSequence,
};

/// Runs `f` behind an animated spinner labeled `message`, for CLI feedback around a
/// blocking call (e.g. chip-emulated decoding, pattern-size search).
fn with_spinner<T>(message: &str, f: impl FnOnce() -> T) -> T {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}").unwrap());
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));

    let result = f();

    pb.finish_and_clear();
    result
}

#[derive(Parser, Debug)]
#[command(
    name = "ym-song",
    version,
    about = "YM-2149 Music Compilation & Auditioning Toolchain",
    long_about = None
)]
struct SongCli {
    #[command(subcommand)]
    command: SongCommands,
}

#[derive(Subcommand, Debug)]
enum SongCommands {
    /// Render a music song file into compiled YM-2149 binary stream
    Render {
        /// Input music source file path (.json, etc.)
        #[arg(short, long)]
        input: PathBuf,

        /// Output compiled binary file path (.ym)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Timing refresh rate override (50 or 60 Hz)
        #[arg(long, value_enum)]
        hz: Option<HzOption>,

        /// Source chip clock in Hz. YM5/YM6 read from file header; older formats default to 2000000 (Atari ST). Override if needed.
        #[arg(long)]
        clock: Option<u32>,

        /// Frame step (downsample rate: e.g. 2 to skip every other frame)
        #[arg(short, long, default_value_t = 1)]
        step: usize,

        /// Compression level: full (delta+dedup), delta-only (delta, no dedup), none (raw registers)
        #[arg(long, value_enum, default_value = "full")]
        compression: CompressionArg,

        /// Disable pattern deduplication (useful for isolating dedup as a source of issues)
        #[arg(long)]
        no_dedup: bool,

        /// Disable idle-frame RLE (useful for isolating RLE as a source of issues)
        #[arg(long)]
        no_rle: bool,

        /// Fail if the compiled .ysg exceeds this many bytes (e.g. 16384 for a 16KB bank)
        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// Dump raw frame register data for diagnostic inspection
    Dump {
        /// Input .ym file
        #[arg(short, long)]
        input: PathBuf,

        /// Number of frames to dump (default: 100)
        #[arg(short, long, default_value_t = 100)]
        frames: usize,

        /// Starting frame index
        #[arg(long, default_value_t = 0)]
        start: usize,
    },
    /// Audition and play a music song file or stream
    Play {
        /// Input music source file path or compiled binary path
        #[arg(short, long)]
        input: PathBuf,

        /// Timing refresh rate override (50 or 60 Hz)
        #[arg(long, value_enum)]
        hz: Option<HzOption>,

        /// Play a .ym file through our YmSequence pipeline instead of the raw replayer
        #[arg(long)]
        via_sequence: bool,
    },
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
enum CompressionArg {
    /// Delta encoding + pattern deduplication (smallest output)
    Full,
    /// Delta encoding only, no pattern deduplication
    DeltaOnly,
    /// Raw register writes every frame, no compression
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = SongCli::parse();

    match cli.command {
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
                with_spinner("Decoding YM chiptune (emulating playback)...", || {
                    YmSequence::from_ym_data(name, &bytes, clock)
                })?
            } else {
                let content = fs::read_to_string(&input)?;
                (serde_json::from_str(&content)?, 0)
            };

            // Apply step decimation
            let step = step.max(1);
            let limit = sequence.frames.len();
            let mut decimated_frames = Vec::new();
            let mut i = 0;
            while i < limit {
                let window_end = (i + step).min(limit);

                // Peak volume & state detector per channel over step window
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

                // Global registers (noise period, envelope) come from the loudest channel's
                // peak frame so they stay in sync with the most audible moment in the window.
                let dominant_idx = if best_vol_a >= best_vol_b && best_vol_a >= best_vol_c {
                    best_idx_a
                } else if best_vol_b >= best_vol_c {
                    best_idx_b
                } else {
                    best_idx_c
                };

                let mut final_frame = sequence.frames[i].clone();

                // Channel A parameters from peak volume frame
                final_frame.volume_a = sequence.frames[best_idx_a].volume_a;
                final_frame.tone_a = sequence.frames[best_idx_a].tone_a;
                final_frame.tone_enable_a = sequence.frames[best_idx_a].tone_enable_a;
                final_frame.noise_enable_a = sequence.frames[best_idx_a].noise_enable_a;

                // Channel B parameters from peak volume frame
                final_frame.volume_b = sequence.frames[best_idx_b].volume_b;
                final_frame.tone_b = sequence.frames[best_idx_b].tone_b;
                final_frame.tone_enable_b = sequence.frames[best_idx_b].tone_enable_b;
                final_frame.noise_enable_b = sequence.frames[best_idx_b].noise_enable_b;

                // Channel C parameters from peak volume frame
                final_frame.volume_c = sequence.frames[best_idx_c].volume_c;
                final_frame.tone_c = sequence.frames[best_idx_c].tone_c;
                final_frame.tone_enable_c = sequence.frames[best_idx_c].tone_enable_c;
                final_frame.noise_enable_c = sequence.frames[best_idx_c].noise_enable_c;

                // Shared registers from dominant channel's peak frame
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
            let spinner_msg = match compression_level {
                CompressionLevel::Full => "Compiling song (delta + dedup)...",
                CompressionLevel::DeltaOnly => "Compiling song (delta only, no dedup)...",
                CompressionLevel::None => "Compiling song (raw registers, no compression)...",
            };
            let mut compiled = with_spinner(spinner_msg, || {
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
                "; ca65 include generated by ym-song for {}\n\
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
            println!(
                "{} {} (YM_DELAY={}, YM_FINE={})",
                style("CA65 INCLUDE:").bold().green(),
                style(ysi_path.display()).cyan(),
                style(delay_y).cyan(),
                style(delay_x).cyan()
            );

            if digidrum_frames > 0 {
                println!(
                    "{} {} frames contain YM6 digi-drum data (PCM samples) — drums dropped, pitched content preserved",
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
                let pct_display = if pct_change >= 0.0 {
                    style(format!("{:.1}% smaller", pct_change)).green()
                } else {
                    style(format!("{:.1}% larger", -pct_change)).red()
                };
                println!(
                    "{} {} bytes (uncompressed .ym) -> {} bytes (.ysg), {}",
                    style("SIZE:").bold(),
                    style(original_size).cyan(),
                    style(new_size).cyan(),
                    pct_display
                );
            }
        }
        SongCommands::Play {
            input,
            hz,
            via_sequence,
        } => {
            let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");

            if extension == "json" {
                let content = fs::read_to_string(&input)?;
                let mut sequence: YmSequence = serde_json::from_str(&content)?;

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
            } else if extension == "ysg" {
                let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("song");
                let bytes = fs::read(&input)?;
                let mut sequence = YmSequence::from_ysg(name, &bytes)?;

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
                    "{} {} ({} frames @ {} Hz via YmSequence)...",
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
    }

    Ok(())
}
