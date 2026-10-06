# lokey-ym2149-tools

Toolchain for compiling, auditing, and auditioning sound sequences and music streams targeting the Yamaha YM-2149 Programmable Sound Generator (PSG).

## Crates

- **`ym-core`**: Core library containing the `.ysg` channel-split music compiler, `.yfx` sound effect compiler, format abstractions (`SongInput`, `SongFile`, `SfxInput`, `SfxFile`), frame decoders (`YmFile`, `AyfxFile`), and real-time software PSG emulation (`player`).
- **`lym`**: CLI toolchain for compiling, auditioning, dumping, and interactively mixing YM-2149 music and sound effects.

---

## Target Platforms

The **Atari 7800** (1.789773 MHz, `ca65` toolchain) is the primary target. The binary formats (`.ysg`, `.yfx`) and compiler tools are platform-agnostic and support any YM2149 or AY-3-8910 system (e.g. Apple II Mockingboard, Atari ST, MSX, ZX Spectrum, Amstrad CPC) by configuring clock ratios and frame rates.

### Example Projects

Self-contained assembly replayer samples driving `.ysg`/`.yfx` streams:

| Platform | Directory | Assembler Toolchain | Role |
|---|---|---|---|
| Atari 7800 | [examples/7800](examples/7800) | [`cc65`](https://cc65.github.io/) (`ca65`, `ld65`) | Primary target platform |
| Apple II (Mockingboard) | [examples/apple2](examples/apple2) | [`cc65`](https://cc65.github.io/) (`ca65`, `ld65`) | Cross-platform proof of concept |
| Atari ST | [examples/st](examples/st) | [`rmac`](https://github.com/reboot-projects/rmac) (68000 mode) | Cross-platform proof of concept |

---

## Architecture: Producers & Streaming Consumer

This toolchain adopts a **host-precomputed producer/consumer pipeline**:

- **Producers**:
  - **Atari ST**: Decades of chiptune compositions authored in popular ST trackers like [Music-Mon](https://demozoo.org/productions/71415/) and [maxYMiser](https://github.com/ggnkua/maxYMiser), exported as `.ym` register streams.
  - **Modern Workstations (macOS, Windows, Linux)**:
    - **[Furnace](https://github.com/tildearrow/furnace)**: Modern multi-system tracker exporting `.ym` register streams.
    - **[Arkos Tracker](https://www.julien-nevo.com/arkostracker/)**: Cross-platform tracker for AY-3-8910/YM2149 music and sound effects.
    - **ayFX Edit**: Sound effect designer for AY/YM PSG chips ([ayfxedit-improved](https://github.com/Threetwosevensixseven/ayfxedit-improved) for Windows/Wine, original by [Shiru](https://shiru.untergrund.net/software.shtml), or [Web version by Remy Sharp](https://zx.remysharp.com/audio/#src=MjA4LDMy)) exporting `.afx` effects, `.afb` banks, and `.csv` sequences.
    - **Hand-authored JSON**: Direct text-editable sequence definitions for rapid iteration.
- **Consumer (Atari 7800 & 8-Bit Targets)**: The workstation pre-compiles music into channel-split `.ysg` opcode streams and sound effects into 5-byte `.yfx` frames. The 6502 simply consumes and streams opcodes directly to the YM2149 during VBLANK, keeping CPU overhead near zero for gameplay.

### What Is Stripped During Compilation

- **PCM Digi-Drums**: High-rate (4–10 kHz) 8-bit PCM sample buffers in YM6 files are stripped because 8-bit target CPUs cannot stream PCM during active gameplay. Pitched PSG channels (square waves, white noise, hardware envelopes) are preserved.
- **Inaudible Register Sweeps**: Register changes on muted channels (volume `0` or disabled in mixer `R7`) are normalized.
- **Redundant Register Writes**: Unchanged values are omitted via per-voice opcode bit flags.

### Compression & ROM Footprint

The channel-split `.ysg` format decouples voice streams, compresses idle runs into 1-byte wait tokens (`0bbbbbbb`), precomputes mixer (`R7`) values into the Global stream, and benchmarks candidate pattern sizes. Typical streams compile to **~1.7 KB – 18 KB** (e.g., `ND-Loader` is 1.7 KB), with **near-zero CPU overhead** on the target microprocessor.

---

## Quick Start & Workflow

### 1. Music Workflow (`lym song`)

```bash
# Audition original uncompressed .ym track
cargo run --bin lym -- song play --input tests/fixtures/song/ND-Loader.ym

# Render & compress into .ysg binary (generates .ysi ca65 include)
cargo run --bin lym -- song render --input tests/fixtures/song/ND-Loader.ym --output tests/fixtures/song/ND-Loader.ysg

# Audition compiled .ysg cartridge stream
cargo run --bin lym -- song play --input tests/fixtures/song/ND-Loader.ysg
```

### 2. Sound Effects Workflow (`lym sfx`)

```bash
# Audition raw sound effect source (.json, .csv, .afx, .afb)
cargo run --bin lym -- sfx play --input tests/fixtures/sfx/blip.json

# Render into 5-byte fixed-width .yfx payload (generates .yfi ca65 include)
cargo run --bin lym -- sfx render --input tests/fixtures/sfx/blip.json --output tests/fixtures/sfx/blip.yfx

# Audition compiled .yfx binary
cargo run --bin lym -- sfx play --input tests/fixtures/sfx/blip.yfx
```

### 3. Interactive Keyboard Mixer (`lym mix`)

Test channel takeover and priority arbitration in real time (`1`–`9`, `0`, `SPACE` to trigger SFX):

```bash
cargo run --bin lym -- mix --song tests/fixtures/song/ND-Loader.ysg --sfx tests/fixtures/sfx/pew-x.yfx tests/fixtures/sfx/phew.csv --channel c
```

---

## Documentation

- **[LYM CLI Reference Guide](docs/LymReference.md)** — Command-line manual for `song`, `sfx`, and `mix` subcommands.
- **[File Formats Specification](docs/FileFormats.md)** — Specifications for `.ysg`, `.yfx`, `.ysi`, `.yfi`, `.afx`, `.afb`, `.json`, `.csv`, `.ym`.
- **[YM Sound & Replayer Specification](docs/YmSoundDesign.md)** — Architecture, channel arbitration, and playback design.
- **[Musical Credits & Test Assets](docs/Musicians.md)** — Composer attributions for test fixtures.

---

## Acknowledgements & Credits

- **[Arkos Tracker](https://www.julien-nevo.com/arkostracker/)**: The `.ysg` channel-split streaming format was inspired by the **AKY** format designed by Julien Névo (Targhan).
- **[Shiru](https://shiru.untergrund.net/software.shtml)**: Designer of the **ayFX** sound effect formats (`.afx`, `.afb`) and author of the original `ayFX Edit` utility.
- **Arnaud Carré (Leonard/OXG)**: Creator of the **`.ym` file format specifications** (YM2 through YM6) and curator of the [StSound](http://leonard.oxg.free.fr/stsound/stsound.html) chiptune library and archive.
- **`ym2149-rs` Ecosystem**: Low-level PSG emulation and chiptune parsing leverage crates by [slippyex](https://github.com/slippyex):
  - **[`ym2149`](https://crates.io/crates/ym2149)**: Yamaha YM-2149 PSG emulator core.
  - **[`ym2149-common`](https://crates.io/crates/ym2149-common)**: Player traits and frequency helper types.
  - **[`ym2149-ym-replayer`](https://crates.io/crates/ym2149-ym-replayer)**: Atari ST `.ym` music parser and player.
- **Musical Composers**: Benchmark songs in `tests/fixtures/song/` were sourced from the StSound collection. Track composers include:
  - **Jochen Hippel (Mad Max)** (*An Cool*, *Enchanted Lands*, *Jess*, *Scout*)
  - **Jeroen Tel (Maniacs of Noise)** (*Scout* original composition)
  - **TAO / ACF** (*Nostalgic-O-Demo*)
  - **Jedi / Sector One** and **Furax / Equinox** (*Virtual Escape*)
  *(Detailed track attributions are listed in [docs/Musicians.md](docs/Musicians.md)).*

---

## License

Licensed under the [MIT License](LICENSE). Test song fixtures in `tests/fixtures/song/` remain copyright of their original composers (see [Musical Credits & Test Assets](docs/Musicians.md)).
