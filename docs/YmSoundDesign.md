# YM Sound & Replayer Specification

## Overview

The `lokey-ym-tools` pipeline compiles, auditions, and mixes sound sequences and music streams for the YM2149 / AY-3-8910 Programmable Sound Generator (PSG) directly on a desktop workstation before flashing to target retro hardware.

### Supported Input Formats

- **Music**:
  - `.ym` (Atari ST YM5/YM6 register dumps)
  - `.vgm` / `.vgz` (VGM register logs, including Furnace Tracker AY-3-8910 / YM2149 exports; `.vgz` and gzipped `.vgm` are inflated first)
  - `.ysg` (Compiled channel-split target binary)
- **Sound Effects (SFX)**:
  - `.csv` (AYFXedit active-high columns visual export, with optional 6th tick-duration column)
  - `.afx` (Single AYFX binary effect file)
  - `.afb` (Multi-effect binary sound bank)
  - `.yfx` (Compiled 5-byte target sound effect binary)

---

## Cartridge Binary Formats

Audio assets are compiled into custom target formats (`.ysg` for songs, `.yfx` for sound effects) to minimize cartridge ROM space and 6502 CPU cycles:

- **Music Format (`.ysg`)**: Relocatable 4-stream container (Voice A, Voice B, Voice C, Global). Uses a 20-byte fixed header, sequence index tables, 16-bit relative pattern offset pointers, and 1-byte run-length idle tokens (`0bbbbbbb`). Empty patterns use the `$FF` sentinel byte. All channel Tone/Noise enables are precomputed at build time into the Global track's R7 mixer updates. The 6502 replayer state requires only 14 bytes of Zero Page (`TYsgPlayerState`).
- **Sound Effects Format (`.yfx`)**: Fixed-width 5-byte frame layout (`PitchLow`, `PitchHigh`, `Volume`, `Control`, `Duration`), enabling low-overhead VBI channel overrides without variable-length parsing.

*(See [File Formats Specification](FileFormats.md) for field offsets and bit allocation tables).*

---

## Playback & Channel Takeover Architecture

The replayer runs during the Vertical Blanking Interval (VBI). Music streams are processed first, and active SFX channels override PSG registers:

```
                             [ Replayer VBI Update ]
                                        │
                                        ▼
                            ┌──────────────────────────┐
                            │    Decode Music Stream   │
                            │      Opcode Bytes        │
                            └────────────┬─────────────┘
                                        │
                                        ▼
                            ┌──────────────────────────┐
                            │   Is SFX Active on any   │
                            │         channel?         │
                            └────────────┬─────────────┘
                                        │
                         ┌──────────────┴──────────────┐
                         │ Yes                         │ No
                         ▼                             ▼
            ┌──────────────────────────┐  ┌──────────────────────────┐
            │   Override Active SFX    │  │   Music Stream Writes    │
            │   Pitch/Volume Registers │  │    Directly to PSG       │
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
            │   Write Resolved State   │
            │      Directly to PSG     │
            │       ($0800/$0801)      │
            └──────────────────────────┘
```

### Register Arbitration Rules

- **Pitch & Volume (R0–R5, R8–R10)**: Overridden unconditionally on the channel assigned to the SFX.
- **Noise Period (R6)**: If an active SFX requests noise, it takes exclusive ownership of R6. Music updates to R6 are suspended until the SFX finishes.
- **Hardware Envelopes (R11–R13)**: Reserved strictly for music. Sound effects use software volume attenuation (R8–R10) over time to avoid retriggering or distorting music envelopes.

---

## Workspace Architecture (`ym-core`)

`ym-core` keeps ingestion, compilation, and mono audio rendering decoupled using concrete data structs and direct functions:

### Concrete Types & Error Handling

Fallible operations return strongly typed `Result<T, YmError>` errors (derived with `thiserror`), providing structured diagnostics (`TruncatedHeader`, `InvalidMagic`, `UnsupportedVersion`, `MissingAy8910Clock`, `UnsupportedVgmCommand`, `BankIndexOutOfBounds`, `SongBinaryTooLarge`, etc.) with chained `std::error::Error::source()` propagation across I/O and format decoders.

- **Music Types**:
  - `YmFile` / `VgmFile`: Parse `.ym` and `.vgm`/`.vgz` sources and convert them to `YmSequence` via `to_sequence`. `.vgm`/`.vgz` logs are sampled to one register frame per tick before conversion, automatically deduplicating redundant trailing loop wraparound frames and normalizing loop offsets.
  - `YsgFile`: Compiles `YmSequence` into a 4-track channel-split `.ysg` payload and decompiles back to `YmSequence`. Optimal pattern size exploration prioritizes candidates that divide the song without silence padding and align cleanly with the loop point.
  - `YmSequence::from_bytes` / `YmSequence::from_file`: Dispatch directly across `.ym`, `.vgm`, `.vgz`, and `.ysg`.
- **SFX Types**:
  - `AyfxFile`: Parses `.afx`, `.afb`, and `.csv` sources into `SfxSequence` (pure register frames only; playback timing and channel routing are provided at playback time).
  - `YfxFile`: Compiles `SfxSequence` into 5-byte `.yfx` frames and decodes back to `SfxSequence`.
  - `SfxSequence::from_bytes` / `SfxSequence::from_file`: Dispatch directly across `.csv`, `.afx`, `.afb`, and `.yfx`.

### Pure Mono Software PSG Emulation (`player.rs`)

- **`YmSongRenderer`**: Streams music frames to an emulated `Ym2149` chip instance and renders mono `f32` PCM buffers.
- **`YmSfxRenderer`**: Renders isolated sound effects on a caller-specified `YmChannel` and `TimingConfig` into mono `f32` PCM buffers.
- **`YmMixer`**: Real-time multi-channel mixer supporting dynamic SFX triggering and channel takeover into mono `f32` PCM buffers.
- **`YmDataRenderer`**: Streams raw `.ym` files via `ym2149-ym-replayer` into mono `f32` PCM buffers.
