# lokey-ym-tools

A comprehensive developer toolchain for compiling, auditing, and auditioning sound sequences and music streams targeting the Yamaha YM-2149 Programmable Sound Generator (PSG).

> [!IMPORTANT]
> **Workstation-First Pre-Production & Auditioning Pipeline**: Perfect 99% of your game's soundtrack on your PC workstation before flashing a single byte to retro console hardware! Ingest multi-format sound effect assets (`.json`, `.csv`, `.afx`, `.afb`), compress 16-bit Atari ST `.ym` tracks into zero-CPU-overhead `.ysg` streams, and interactively mix music and SFX with real-time keyboard controls—all before touching target 6502 assembly code.

## Crates in the Workspace

- **`ym-core`**: The foundational library containing the platform-agnostic `DeltaCompiler`, YM register configurations,
  frame structures, format decoders, and the real-time audio playback engine.
- **`lym`**: The unified CLI toolchain for compiling, auditioning, dumping, and interactively mixing YM-2149 music songs
  and sound effects.
- **`a78tool`**: Atari 7800 `.a78` ROM header utility (see [lokey-7800-tools](file:///home/john/Projects/lokey-7800-tools)).

---

## Target Platforms & Contributing

The **Atari 7800** console (1.789773 MHz clock / ca65 toolchain) is the main, actively-developed target — it's what the tooling is built and tuned for day to day. That said, the binary formats (`.ysg`, `.yfx`) and compiler tools are designed to be platform-agnostic, supporting any YM2149 or AY-3-8910 platform (such as ZX Spectrum, MSX, Amstrad CPC, Atari ST, Atari XL/XE expansion, Apple II Mockingboard, Intellivision, or Vectrex).

Pull requests and merge requests for additional target platform replayers, assembly drivers, and sample projects are graciously accepted!

### Example Projects

Each platform under [`examples/`](examples/) is a self-contained assembly sample driving the `.ysg`/`.yfx` streams produced by `lym`. They require a platform-specific assembler toolchain on `PATH`, in addition to the Rust toolchain:

| Platform | Directory | Assembler Toolchain | Role |
| --- | --- | --- | --- |
| Atari 7800 | [`examples/7800`](examples/7800) | [`cc65`](https://cc65.github.io/) (`ca65`, `ld65`) | Main platform |
| Apple II (Mockingboard) | [`examples/apple2`](examples/apple2) | [`cc65`](https://cc65.github.io/) (`ca65`, `ld65`) | Cross-platform proof of concept |
| Atari ST | [`examples/st`](examples/st) | [`rmac`](https://github.com/reboot-projects/rmac) (68000 mode) | Cross-platform proof of concept |

The Apple II and Atari ST samples exist to prove out the "platform-agnostic" claim above, not as fully supported targets in their own right — porting to them exercised the shared `ym-core` code on real hardware assumptions the 7800 alone never triggered, and turned up genuine bugs (like clock-scaling and format-detection issues) that were fixed upstream for every platform.

---

## Architecture & Philosophy: 16-Bit Audio Streaming on 8-Bit Consoles

This toolchain adopts a **host-precomputed streaming architecture**:

- **Offloaded Heavy Compute**: Complex 16-bit computer music (e.g. Atari ST `.ym` tracks) is pre-compiled into pattern-deduplicated register delta streams (`.ysg`) on your workstation PC. All pitch scaling, envelope calculations, and frame diffing happen at build time.
- **What Is Stripped During Compilation**:
  - **PCM Digi-Drum Sample Data**: High-rate (4kHz–10kHz) 8-bit PCM sample buffers embedded in Atari ST YM6 files are automatically stripped because 8-bit CPUs cannot stream 4,000+ Hz PCM bytes during active gameplay. All pitched 3-channel PSG music (square waves, white noise, and hardware envelopes) is preserved 100%. *(Note: While most chiptunes sound complete without digi-drums because the melody and bass carry the track, drum-heavy songs like `ND-Loader` will sound noticeably different).*
  - **Inaudible Register Sweeps**: Pitch/noise register changes occurring on channels with volume `0` or disabled mixer outputs (`R7`) are normalized, eliminating unhearable data from the stream.
  - **Redundant Register Writes**: Consecutive unchanged register values across frames are diffed out via 16-bit delta bitmasks, and idle frame runs are compressed via RLE tokens.
- **The Trade-Off**: Pre-compiled `.ysg` streams require larger ROM storage (~15 KB – 28 KB vs 2 KB – 5 KB for native trackers). However, in exchange for ROM space, **6502 CPU overhead is reduced to near zero** during gameplay — the 6502 simply reads pre-diffed bytes during VBLANK interrupts and writes directly to PSG register ports.
- **Full Hardware Audio Parity**: 8-bit retro systems (like the Atari 7800 with YM2149 expansion hardware) can stream rich 16-bit Atari ST chiptunes with 100% audio fidelity while preserving virtually all CPU cycles for game graphics, collision detection, and logic.

> [!TIP]
> **Why precompute on your PC?** By baking pitch-scaling, delta bitmasks, and pattern deduplication into the `.ysg` binary at build time, your retro game loop only pays a tiny VBLANK register write budget—giving your 8-bit game the voice of a 16-bit Atari ST.

---

## Quick Start & Auditioning Workflow

### Prerequisites

Ensure you have the Rust toolchain installed. Since auditioning plays audio directly through your speakers, `cpal` will bind to your system's default audio host (ALSA on Linux, CoreAudio on macOS, WASAPI on Windows).

### Usage

#### 1. Song Workflow: Audition Source, Render, and Audition Target

Audition the uncompressed 16-bit Atari ST source track, compile it into an optimized `.ysg` cartridge binary payload, and audition the compiled stream to verify audio parity:

```bash
# Step 1: Audition original uncompressed Atari ST .ym source track:
cargo run --bin lym -- song play --input tests/fixtures/song/ND-Loader.ym

# Step 2: Render & compress into .ysg binary (also auto-generates .ysi ca65 include):
cargo run --bin lym -- song render --input tests/fixtures/song/ND-Loader.ym --output tests/fixtures/song/ND-Loader.ysg

# Step 3: Audition compiled target .ysg cartridge binary stream:
cargo run --bin lym -- song play --input tests/fixtures/song/ND-Loader.ysg
```

#### 2. Sound Effects Workflow: Audition Source, Render, and Audition Target

Audition raw sound effect sources (`.json`, `.csv`, `.afx`, `.afb`), compile them into 5-byte fixed-width `.yfx` binaries, and audition the target payload:

```bash
# Step 1: Audition raw JSON sound effect source:
cargo run --bin lym -- sfx play --input tests/fixtures/sfx/blip.json

# Step 2: Render into 5-byte fixed-width .yfx payload (also auto-generates .yfi ca65 include):
cargo run --bin lym -- sfx render --input tests/fixtures/sfx/blip.json --output tests/fixtures/sfx/blip.yfx

# Step 3: Audition compiled 5-byte fixed-width target payload:
cargo run --bin lym -- sfx play --input tests/fixtures/sfx/blip.yfx
```

#### 3. Live Interactive Keyboard Mixer

Jam out with song playback while firing sound effects in real time using your keyboard (`1`–`9`, `0`, `SPACE`) to test channel takeover and conflict arbitration:

```bash
cargo run --bin lym -- mix --song tests/fixtures/song/ND-Loader.ysg --sfx tests/fixtures/sfx/pew-x.yfx tests/fixtures/sfx/phew.csv --channel c
```

---

## Documentation

- **[LYM CLI Reference Guide](docs/LymReference.md)** — Complete command-line manual for `lym song`, `lym sfx`, and `lym mix` subcommands and options.
- **[File Formats Specification](docs/FileFormats.md)** — Specifications for `.ysg`, `.yfx`, `.ysi`, `.yfi`, `.afx`, `.afb`, `.json`, `.csv`, `.ym`.
- **[YM Sound & Replayer Specification](docs/YmSoundDesign.md)** — Internal design, compiler architecture, channel takeover arbitration, and hardware specs.
- **[Musical Credits & Test Assets](docs/Musicians.md)** — Composers and attributions for test song fixtures.

---

## Acknowledgements & Credits

This toolset leverages the excellent **`ym2149-rs`** ecosystem developed by [slippyex](https://github.com/slippyex) for
low-level emulation and chiptune parsing:

- **[`ym2149`](https://crates.io/crates/ym2149)**: Provides the cycle-accurate Yamaha YM-2149 PSG emulator core.
- **[`ym2149-common`](https://crates.io/crates/ym2149-common)**: Outlines player traits and frequency helper types.
- **[`ym2149-ym-replayer`](https://crates.io/crates/ym2149-ym-replayer)**: Performs loader, parser, and decompressed
  vbl-sync playback logic for Atari ST `.ym` music formats.

We extend our deep gratitude to the authors of these crates for providing the cycle-accurate emulation engine that
powers the real-time auditioning tools in this codebase.

---

## License

This project is licensed under the [MIT License](LICENSE).

*Note: Test song fixtures included in `tests/fixtures/song/` remain the copyright of their respective original composers (see [Musical Credits & Test Assets](docs/Musicians.md)).*
