use clap::Parser;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "a78gen",
    version,
    about = "Generate Atari 7800 .a78 ROM image with header",
    long_about = "Wraps a raw ROM binary in the 128-byte Atari 7800 .a78 header \
                  recognised by emulators (ProSystem, A7800, etc.).\n\n\
                  Pass --config to load header fields from a JSON file, or use \
                  the individual flags to override specific fields."
)]
struct Cli {
    /// Raw ROM binary input (.bin or .rom)
    #[arg(short, long)]
    input: PathBuf,

    /// Output .a78 file path
    #[arg(short, long)]
    output: PathBuf,

    /// JSON config file for header fields (see --help for format)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Cart title (up to 32 ASCII characters)
    #[arg(long)]
    title: Option<String>,

    /// Mapper: 0 = linear/fixed 32K, 1 = YM-IOA banked 128K/256K
    #[arg(long)]
    mapper: Option<u8>,

    /// Cart type word (high/low bytes at header offsets 53/54)
    #[arg(long, default_value_t = 0)]
    cart_type: u16,

    /// Audio word — default 0x0800 (YM2149 enabled, pokey disabled)
    #[arg(long)]
    audio: Option<u16>,

    /// TV type: 0 = NTSC, 1 = PAL
    #[arg(long, default_value_t = 0)]
    tv_type: u8,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Config {
    title: Option<String>,
    #[serde(default = "default_version")]
    version: u8,
    cart_type: u16,
    controller_1: u8,
    controller_2: u8,
    tv_type: u8,
    save_device: u8,
    slot_passthrough: u8,
    mapper: u8,
    mapper_opts: u8,
    #[serde(default = "default_audio")]
    audio: u16,
    interrupt: u16,
}

fn default_version() -> u8 {
    4
}
fn default_audio() -> u16 {
    0x0800
}

fn main() {
    let cli = Cli::parse();

    // Load base config from JSON if provided, otherwise start from defaults.
    let mut cfg = if let Some(cfg_path) = &cli.config {
        let json = fs::read_to_string(cfg_path).unwrap_or_else(|e| {
            fatal(&format!("Cannot read config '{}': {e}", cfg_path.display()))
        });
        serde_json::from_str::<Config>(&json)
            .unwrap_or_else(|e| fatal(&format!("Invalid config JSON: {e}")))
    } else {
        Config {
            audio: default_audio(),
            version: default_version(),
            ..Default::default()
        }
    };

    // CLI flags override config file values.
    if let Some(t) = cli.title {
        cfg.title = Some(t);
    }
    if let Some(m) = cli.mapper {
        cfg.mapper = m;
    }
    if let Some(a) = cli.audio {
        cfg.audio = a;
    }
    if cli.cart_type != 0 {
        cfg.cart_type = cli.cart_type;
    }
    cfg.tv_type = cli.tv_type;

    // Default controller to joystick (1) when not set via config.
    if cfg.controller_1 == 0 {
        cfg.controller_1 = 1;
    }
    if cfg.controller_2 == 0 {
        cfg.controller_2 = 1;
    }

    let raw = fs::read(&cli.input)
        .unwrap_or_else(|e| fatal(&format!("Cannot read '{}': {e}", cli.input.display())));

    let rom_data: Vec<u8> = match cfg.mapper {
        1 => {
            // YM-IOA banked: pass the full image through unchanged.
            if raw.len() != 128 * 1024 && raw.len() != 256 * 1024 {
                fatal(&format!(
                    "Mapper 1 (YM-IOA banked) requires a 128 KB or 256 KB input binary, got {} bytes.",
                    raw.len()
                ));
            }
            raw
        }
        _ => {
            // Linear / fixed 32K: keep only the last (top) 32 KB.
            if raw.len() > 32768 {
                eprintln!(
                    "Warning: input is {} bytes but mapper is 0 (linear/fixed 32K) — \
                     only the top 32 KB will be kept. Use --mapper 1 for a banked image.",
                    raw.len()
                );
            }
            let mut rom = vec![0xFFu8; 32768];
            let copy_len = raw.len().min(32768);
            let dst_start = 32768 - copy_len;
            let src_start = raw.len() - copy_len;
            rom[dst_start..].copy_from_slice(&raw[src_start..]);
            rom
        }
    };

    // Build the 128-byte .a78 header.
    let mut header = vec![0u8; 128];

    header[0] = cfg.version;
    let magic = b"ATARI7800";
    header[1..1 + magic.len()].copy_from_slice(magic);

    let title = cfg.title.as_deref().unwrap_or("YM2149 CART");
    let title_bytes: Vec<u8> = title.bytes().take(32).collect();
    let pad = 32 - title_bytes.len();
    header[17..17 + title_bytes.len()].copy_from_slice(&title_bytes);
    header[17 + title_bytes.len()..17 + 32].fill(0x20); // space-pad
    let _ = pad;

    let rom_size = rom_data.len() as u32;
    header[49] = (rom_size >> 24) as u8;
    header[50] = (rom_size >> 16) as u8;
    header[51] = (rom_size >> 8) as u8;
    header[52] = rom_size as u8;

    header[53] = (cfg.cart_type >> 8) as u8;
    // Bit 2 of the cart type low byte signals YM2149 presence to the a7800/js7800
    // emulator forks. The old detection (bit 6 of offset 66) only worked when the
    // chip was at $4000; the $0800 mapping sets offset 66 = $08, missing that bit.
    header[54] = cfg.cart_type as u8 | 0x04;

    header[55] = cfg.controller_1;
    header[56] = cfg.controller_2;
    header[57] = cfg.tv_type;
    header[58] = cfg.save_device;
    header[63] = cfg.slot_passthrough;
    header[64] = cfg.mapper;
    header[65] = cfg.mapper_opts;
    header[66] = (cfg.audio >> 8) as u8;
    header[67] = cfg.audio as u8;
    header[68] = (cfg.interrupt >> 8) as u8;
    header[69] = cfg.interrupt as u8;

    let end_magic = b"ACTUAL CART DATA STARTS HERE";
    header[100..100 + end_magic.len()].copy_from_slice(end_magic);

    // Write .a78 file: header + ROM data.
    let mut out: Vec<u8> = Vec::with_capacity(128 + rom_data.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&rom_data);

    fs::write(&cli.output, &out)
        .unwrap_or_else(|e| fatal(&format!("Cannot write '{}': {e}", cli.output.display())));

    println!(
        "Generated {} ({} header + {} KB ROM)",
        cli.output.display(),
        128,
        rom_data.len() / 1024
    );
}

fn fatal(msg: &str) -> ! {
    eprintln!("Error: {msg}");
    std::process::exit(1);
}
