# YM Sound & Replayer Specification

## Scope & Philosophy: Workstation-First Pre-Production

The `lokey-ym-tools` SDK is built around a **workstation-first audio pre-production workflow**. Instead of debugging
audio routines on physical target hardware or hardware emulators, developers can author, ingest, compress, audition, and
interactively mix their entire soundtrack directly on their PC workstation:

1. **Multi-Format Ingestion**: Convert hand-authored `.json`, visual AYFX `.csv` exports, binary `.afx` effects, or
   multi-effect `.afb` banks into optimized 5-byte fixed-width `.yfx` VBI overrides.
2. **16-Bit $\rightarrow$ 8-Bit Chiptune Conversion**: Pre-compile complex Atari ST `.ym` tracks into low-CPU-overhead
   `.ysg` streams, offloading all pitch scaling, envelope calculations, and channel-split opcode streaming at build time.
3. **Desktop Audition & Live Keyboard Mixing**: Preview songs and sound effects through cycle-accurate YM2149 emulation
   and `cpal` speakers, interactive key-triggering (`1`–`9`, `0`, `SPACE`) to test channel takeover and priority
   arbitration live before writing a single line of target assembly.

### Supported Input Formats

* **Music**:
    * `.ym` (Atari ST YM5/YM6 register dumps) via `ym2149-ym-replayer`.
    * `.ysg` (Compiled channel-split target binary) via `YsgFile`.
    * `.json` (Hand-authored music sequence source files).
* **Sound Effects (SFX)**:
    * `.json` (Hand-authored sequence source files).
    * `.csv` (AYFXedit active-high columns visual export).
    * `.afx` (Single AYFX binary effect file).
    * `.afb` (Multi-effect binary sound bank).
    * `.yfx` (Compiled 5-byte target sound effect binary).

---

## Cartridge Binary Formats

Audio assets are compiled into custom target formats (`.ysg` for songs, `.yfx` for sound effects) to fit within
cartridge ROM space constraints and execute within a minimal 6502 CPU cycle budget.

* **Music Format (`.ysg`)**: Uses a 20-byte fixed header, four decoupled track streams (Voice A, Voice B, Voice C,
  and Global), sequence index tables, 16-bit relative pattern offset pointers, and run-length idle frame tokens
  (`0bbbbbbb`). Empty pattern blocks are referenced using the `$FF` sequence sentinel byte to avoid storing empty
  bytes, and all channel Tone and Noise enables are precomputed at build time into the Global track's R7 mixer updates.
  The target 6502 replayer maintains an ultra-compact 14-byte Zero-Page playback state (`TYsgPlayerState`).
* **Sound Effects Format (`.yfx`)**: Uses a 5-byte fixed-width frame representation (`PitchLow`, `PitchHigh`, `Volume`,
  `Control`, `Duration`), allowing rapid VBI channel overrides without variable-length parsing overhead.

*(For exact byte layout, field offsets, and bit allocation tables, see
the [File Formats Specification](FileFormats.md)).*

---

## Playback & Channel Takeover Architecture

The replayer driver decodes the music stream into a 14-byte working RAM buffer unconditionally on every VBI tick,
ensuring seamless resume when a sound effect ends.

```
                             [ Replayer VBI Update ]
                                        │
                                        ▼
                           ┌──────────────────────────┐
                           │ Decode Music to 14-byte  │
                           │     RAM Buffer (0-13)    │
                           └────────────┬─────────────┘
                                        │
                                        ▼
                           ┌──────────────────────────┐
                           │   Is SFX Active on any   │
                           │         channels?        │
                           └────────────┬─────────────┘
                                        │
                         ┌──────────────┴──────────────┐
                         │ Yes                         │ No
                         ▼                             ▼
            ┌──────────────────────────┐  ┌──────────────────────────┐
            │ Substitute Pitch/Volume/ │  │ Write 14-byte RAM Buffer │
            │ Mixer bits in RAM Buffer │  │    Directly to YM PSG    │
            └────────────┬─────────────┘  │      ($0800/$0801)       │
                         │                └──────────────────────────┘
                         ▼
            ┌──────────────────────────┐
            │ Resolve Global Conflicts │
            │ (Noise Period / Envelope)│
            └────────────┬─────────────┘
                         │
                         ▼
            ┌──────────────────────────┐
            │ Write 14-byte RAM Buffer │
            │    Directly to YM PSG    │
            │      ($0800/$0801)       │
            └──────────────────────────┘
```

### Global Register Arbitration

* **Pitch & Volume (Channel-Isolated)**: Overridden unconditionally per active channel.
* **Noise Period (R6)**: If an active SFX channel requests noise, it takes exclusive ownership of the global Noise
  Period (R6). The replayer suspends writing the music's R6 values and writes the SFX's requested R6 value instead.
* **Envelopes (R11-R13)**: The hardware envelope generator remains reserved for music. Sound effects are restricted to
  software volume envelopes (manipulating volume R8-R10 over time) to prevent global audio distortion.

---

## Rust Workspace & Crates Selected

We have selected the following crates and module structures to form the core of our workspace:

* **`ym2149-rs` (slippyex workspace)**: Modular chiptune emulation and parsing stack.
    * `ym2149`: Core cycle-accurate PSG chip emulation.
    * `ym2149-ym-replayer`: Decodes and plays `.ym` files.
* **`cpal`**: Low-level cross-platform audio device stream provider, used by the `lym` CLI crate only — `ym-core` stays
  a pure rendering engine with no device/OS dependencies.
* **`serde` & `serde_json`**: For parsing hand-authored `.json` sound effect and song sequence sources.
* AYFX `.csv` parsing is hand-rolled rather than via a dedicated `csv` crate.

### Trait & File Format Architecture (`ym-core`)

The core library decouples format ingestion, optimization, and serialization via explicit traits:

* **Music Traits**:
    * `SongInput`: Implemented by `YmFile` (Atari ST YM5/YM6) and `YsgFile` (target binary). Decodes files into a unified `YmSequence`.
    * `SongFile`: Implemented by `YsgFile`. Compiles a `YmSequence` into a 4-track channel-split payload and serializes to disk.
* **SFX Traits**:
    * `SfxInput`: Implemented by `AyfxFile` (`.afx`, `.afb`, `.csv`) and `YfxFile` (`.yfx`). Decodes sound effects into `SfxSequence`.
    * `SfxFile`: Implemented by `YfxFile`. Compiles an `SfxSequence` into 5-byte fixed-width VBI frames and writes to disk.
* **Software PSG Renderers (`ym-core/src/player.rs`)**:
    * `YmSongRenderer`: Streams frames to an emulated `Ym2149` chip instance and outputs PCM audio buffers.
    * `YmSfxRenderer`: Renders isolated sound effects across selected channels.
    * `YmMixer`: Real-time interactive multi-channel mixer supporting dynamic SFX triggering, voice takeover, and conflict resolution.

---

## Rust Workspace Architecture & Implemented Milestones

The core SDK workspace provides full implementation of sound effect and music toolchains across two primary feature
suites:

* **Milestone 1: `lym sfx` (Sound Effects Compiler & Player)** `[COMPLETED]`
    * Parse JSON, AYFX `.csv`, binary `.afx`, and multi-effect bank `.afb` files.
    * Real-time workstation audio playback previewer using the `ym2149` chip emulator core and `cpal` output streaming.
    * Compile sound effects into optimized `.yfx` target binaries using the 5-byte fixed-width format and auto-generate
      `.yfi` ca65 include headers.
* **Milestone 2: `lym song` & `lym mix` (Music Compiler, Auditioning & Interactive Mixer)** `[COMPLETED]`
    * Directly parse `.ym` files (including LHA compressed sources) and `.ysg` streams.
    * Apply compile-time pitch-scaling via `--target-clock` (e.g. Atari ST 2.0MHz $\rightarrow$ 7800 1.789773MHz,
      or $\rightarrow$ Apple II Mockingboard ~1.02MHz) and temporal resampling/decimation (`--step`).
    * Implement **Channel-Split Pattern Compression**, 4-track decoupled encoding (Voices A, B, C, Global), RLE idle-run tokens, sequence packing, and optimal candidate exploration (`--pattern-frames`) producing `.ysg` binaries and `.ysi` ca65 include headers.
    * Real-time interactive multi-channel song & SFX keyboard mixer with 10 key slots (`1`–`9`, `0`, `SPACE`),
      polyphonic channel fallback, and arrow-key seeking (`←`/`→`).

---

## "Crazy Stuff We Might Do" (Optional / Highly Drop-Friendly)

If we have too much caffeine or find ourselves with excess spare time, here is the wishlist of features we can easily
throw out the window if reality catches up with us:

* **Software-in-the-Loop (SIL) Matrix Mode**:
    * *The Idea*: Run the actual compiled 6502 replayer code inside a virtual `mos6502` CPU simulator on the
      workstation. The Rust tool runs DASM/MADS in the background, loads the `.bin` into emulated RAM, intercepts memory
      writes to `$0800` / `$0801`, and plays them through the PC speakers.
    * *Steps*:
      1. **Compile**: Rust harness runs DASM/MADS in the background. 2. **Load**: loads target `.bin` and `.ysg`/`.yfx`
      assets into virtual `mos6502` RAM. 3. **Bridge**: Simulates the 6502 CPU and redirects register writes to the
      emulated `ym2149` PSG core. 4. **Preview**: Emulated YM PSG core outputs audio PCM samples to the PC speakers via
      `rodio`/`cpal`.
* **6502 Assembly Unit Testing**:
    * *The Idea*: Write standard Rust unit tests that load specific compiled 6502 subroutines (e.g., bit-unpacking,
      volume scaling, or pointer calculation) into `mos6502` memory. The test sets initial registers/RAM values, steps
      the CPU, and asserts that the resulting register states and memory locations match expected values.
    * *Status*: A highly practical way to debug low-level assembly logic (off-by-ones, register clobbering) headlessly.
