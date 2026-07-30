# lokey-ym-tools

A comprehensive developer toolchain for compiling, auditing, and auditioning sound sequences and music streams
targetting the Yamaha YM-2149 Programmable Sound Generator (PSG).

## Crates in the Workspace

- **`ym-core`**: The foundational library containing the platform-agnostic `DeltaCompiler`, YM register configurations,
  frame structures, format decoders, and the real-time audio playback engine.
- **`lym`**: The unified CLI toolchain for compiling, auditioning, dumping, and interactively mixing YM-2149 music songs
  and sound effects.
- **`a78tool`**: Atari 7800 `.a78` ROM header utility (see [lokey-7800-tools](file:///home/john/Projects/lokey-7800-tools)).

---

## Target Platforms & Contributing

While the primary target is currently the **Atari 7800** console (1.789773 MHz clock / ca65 toolchain), the binary formats (`.ysg`, `.yfx`) and compiler tools are platform-agnostic, supporting any YM2149 or AY-3-8910 platform (such as ZX Spectrum, MSX, Amstrad CPC, Atari ST, Atari XL/XE expansion, Apple II Mockingboard, Intellivision, or Vectrex).

Pull requests and merge requests for additional target platform replayers, assembly drivers, and sample projects are graciously accepted!

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

---

## Getting Started

### Prerequisites

Ensure you have the Rust toolchain installed. Since auditioning plays audio directly through your speakers, `cpal` will
bind to your system's default audio host (ALSA on Linux, CoreAudio on macOS, WASAPI on Windows).

### Usage

#### 1. Play standard YM Chiptune or compiled .ysg files:

```bash
cargo run --bin lym -- song play --input tests/fixtures/song/ND-Loader.ysg
```

#### 2. Play custom JSON or compiled .yfx sound effects:

```bash
cargo run --bin lym -- sfx play --input tests/fixtures/sfx/pew-x.yfx
```

#### 3. Render sound effects or songs into compiled YM-2149 binary payloads:

```bash
cargo run --bin lym -- sfx render --input tests/fixtures/test_sfx.json --output tests/fixtures/laser.yfx
cargo run --bin lym -- song render --input tests/fixtures/song/ND-Loader.ym --output tests/fixtures/song/ND-Loader.ysg
```

#### 4. Interactively mix song playback with keyboard-triggered sound effects:

```bash
cargo run --bin lym -- mix --song tests/fixtures/song/ND-Loader.ysg --sfx tests/fixtures/sfx/pew-x.yfx tests/fixtures/sfx/phew.csv --channel c
```

---

## Acknowledgements & Credits

This toolset leverages the excellent **`ym2149-rs`** ecosystem developed by [slippyex](https://github.com/slippyex) for
low-level emulation and chiptune parsing:

- **[`ym2149`](https://crates.io/crates/ym2149)**: Provides the cycle-accurate Yamaha YM-2149 PSG emulator core.
- **[`ym2149-common`](https://crates.io/crates/ym2149-common)**: Outlines player traits and frequency helper types.
- **[`ym2149-ym-replayer`](https://crates.io/crates/ym2149-ym-replayer)**: Performs loader, parser, and decompressed
  vbl-sync playback logic for legacy Atari ST `.ym` music formats.

We extend our deep gratitude to the authors of these crates for providing the cycle-accurate emulation engine that
powers the real-time auditioning tools in this codebase.
