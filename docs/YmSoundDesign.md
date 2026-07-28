# YM Sound & Replayer Specification

## Scope & Ingestion

The SDK compiles chiptune formats into custom, highly-compressed cartridge binary formats. While the primary target is
currently the Atari 7800 (running at 1.789773 MHz), the architecture and compilation tools are designed to remain
platform-agnostic, keeping the door open to easily target other retro platforms using the YM2149 or AY-3-8910 (e.g.
Atari ST, ZX Spectrum, MSX, Amstrad CPC) by simply adjusting clock-scaling and timing configurations. We leverage the **
`ym2149-rs`** workspace to parse, resample, and compile these assets.

### Supported Input Formats

* **Music**:
    * `.ym` (Atari ST YM5/YM6 register dumps) via `ym2149-ym-replayer`.
* **Sound Effects (SFX)**:
    * `.json` (Hand-authored sequence source files).
    * `.csv` (AYFXedit active-high columns visual export).
    * `.afx` (Single AYFX binary effect file).

---

## Cartridge Binary Formats

Audio assets are compiled into optimized formats to fit within cartridge ROM space constraints and run under a tiny 6502
CPU cycle budget on the Atari 7800.

### A. Music Format (`.ysg`)

* **4-Byte Header**: `[PatternSize, NumUnique, SeqLen, LoopPattern]`.
* **Sequence Table**: Array of `SeqLen` bytes defining the order of patterns.
* **Pattern Offset Table**: Array of `NumUnique` 32-bit little-endian pointers (`NumUnique * 4` bytes) relative to the
  pattern data start, preventing 64KB ROM offset wraps.
* **Pattern-based Delta Masking**: The song is divided into fixed-size pattern blocks of `PatternSize` frames.
* **Pattern Independence**: The first frame of every pattern block is fully-loaded using a full `0x3FFF` register mask
  (R0-R13), eliminating inter-pattern dependencies and allowing $O (1)$ seeking, looping, and phase-independent sound
  effects takeover.
* **Looping**: Seamless loop-back to the `LoopPattern` sequence index is natively encoded in the file header.

### B. Sound Effects Format (`.yfx`)

* **5-Byte Fixed-Width Frames**: Each sound effect frame is encoded as exactly 5 bytes:
    1. **Byte 0 (Control Mask)**: Channel enable flags (Tone Enable, Noise Enable).
    2. **Byte 1 (Pitch Low)**: Fine tone divider.
    3. **Byte 2 (Pitch High)**: Coarse tone divider (bits 0-3) and Noise Period (bits 4-8).
    4. **Byte 3 (Volume)**: Channel volume (0-15).
    5. **Byte 4 (Duration)**: Tick count multiplier specifying how long the frame is held.
* This fixed-width format allows extremely rapid 6502 register overrides without parsing variable-length payloads.

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

We have selected the following crates to form the core of our workspace:

* **`ym2149-rs` (slippyex workspace)**: Modular chiptune emulation and parsing stack.
    * `ym2149`: Core cycle-accurate PSG chip emulation.
    * `ym2149-ym-replayer`: Decodes and plays legacy `.ym` files.
* **`cpal`**: Low-level cross-platform audio device stream provider.
* **`serde` & `serde_json`**: For parsing hand-authored `.json` sound effect and song sequence sources.
* **`csv`**: For parsing visual AYFX `.csv` files.

---

## Rust Refactor Roadmap & Core Milestones

The primary development roadmap for the new Rust-based SDK workspace consists of two core milestones:

* **Milestone 1: `lym sfx` (Sound Effects Compiler & Player)**
    * Parse JSON, AYFX `.csv`, binary `.afx`, and multi-effect bank `.afb` files.
    * Implement real-time workstation audio playback previewer using the `ym2149` chip emulator core and `cpal` output streaming.
    * Compile sound effects into optimized `.yfx` target binaries using the 5-byte fixed-width format.
* **Milestone 2: `lym song` & `lym mix` (Music Compiler, Auditioning & Interactive Mixer)**
    * Directly parse legacy `.ym` files (including LHA compressed sources) and `.ysg` streams.
    * Apply compile-time pitch-scaling (Atari ST 2.0MHz $\rightarrow$ 7800 1.789773MHz) and temporal resampling/decimation.
    * Implement **Pattern-based Delta Masking** and sequence packing.
    * Real-time interactive multi-channel song & SFX keyboard mixer with 10 key slots (`1`–`9`, `0`, `SPACE`), polyphonic channel fallback, and arrow-key seeking (`←`/`→`).

---

## "Crazy Stuff We Might Do" (Optional / Highly Drop-Friendly)

If we have too much caffeine or find ourselves with excess spare time, here is the wishlist of features we can easily
throw out the window if reality catches up with us:

* **Software-in-the-Loop (SIL) Matrix Mode**:
    * *The Idea*: Run the actual compiled 6502 replayer code inside a virtual `mos6502` CPU simulator on the
      workstation. The Rust tool runs DASM/MADS in the background, loads the `.bin` into emulated RAM, intercepts memory
      writes to `$0800` / `$0801`, and plays them through the PC speakers.
    * *Steps*:
        1. **Compile**: Rust harness runs DASM/MADS in the background.
        2. **Load**: loads target `.bin` and `.ysg`/`.yfx` assets into virtual `mos6502` RAM.
        3. **Bridge**: Simulates the 6502 CPU and redirects register writes to the emulated `ym2149` PSG core.
        4. **Preview**: Emulated YM PSG core outputs audio PCM samples to the PC speakers via `rodio`/`cpal`.
* **6502 Assembly Unit Testing**:
    * *The Idea*: Write standard Rust unit tests that load specific compiled 6502 subroutines (e.g., bit-unpacking,
      volume scaling, or pointer calculation) into `mos6502` memory. The test sets initial registers/RAM values, steps
      the CPU, and asserts that the resulting register states and memory locations match expected values.
    * *Status*: A highly practical way to debug low-level assembly logic (off-by-ones, register clobbering) headlessly.
