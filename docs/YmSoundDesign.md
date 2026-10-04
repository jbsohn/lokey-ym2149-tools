# YM Sound & Replayer Specification

## Overview

The `lokey-ym-tools` pipeline compiles, auditions, and mixes sound sequences and music streams for the YM2149 / AY-3-8910 Programmable Sound Generator (PSG) directly on a desktop workstation before flashing to target retro hardware.

### Supported Input Formats

- **Music**:
  - `.ym` (Atari ST YM5/YM6 register dumps)
  - `.ysg` (Compiled channel-split target binary)
  - `.json` (Hand-authored music sequence source files)
- **Sound Effects (SFX)**:
  - `.json` (Hand-authored sequence source files)
  - `.csv` (AYFXedit active-high columns visual export)
  - `.afx` (Single AYFX binary effect file)
  - `.afb` (Multi-effect binary sound bank)
  - `.yfx` (Compiled 5-byte target sound effect binary)

---

## Cartridge Binary Formats

Audio assets are compiled into custom target formats (`.ysg` for songs, `.yfx` for sound effects) to minimize cartridge ROM space and 6502 CPU cycles:

- **Music Format (`.ysg`)**: Relocatable 4-stream container (Voice A, Voice B, Voice C, Global). Uses a 20-byte fixed header, sequence index tables, 16-bit relative pattern offset pointers, and 1-byte run-length idle tokens (`0bbbbbbb`). Empty patterns use the `$FF` sentinel byte. All channel Tone/Noise enables are precomputed at build time into the Global track's R7 mixer updates. The 6502 replayer state requires only 14 bytes of Zero Page (`TYsgPlayerState`).
  *(Note: The legacy monolithic 14-register delta-mask format was dropped due to large ROM footprints of ~15 KB – 116 KB; the channel-split format yields 50%–75% better compression, bringing typical songs to ~1.7 KB – 18 KB).*
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

The `ym-core` library decouples ingestion, compilation, and rendering through explicit traits:

### Trait Model

- **Music Traits**:
  - `SongInput`: Implemented by `YmFile` and `YsgFile`. Decodes source files into `YmSequence`.
  - `SongFile`: Implemented by `YsgFile`. Compiles `YmSequence` into a 4-track channel-split `.ysg` payload and writes to disk.
- **SFX Traits**:
  - `SfxInput`: Implemented by `AyfxFile` and `YfxFile`. Decodes effects into `SfxSequence`.
  - `SfxFile`: Implemented by `YfxFile`. Compiles `SfxSequence` into 5-byte `.yfx` frames and writes to disk.

### Software PSG Emulation & Renderers (`player.rs`)

- **`YmSongRenderer`**: Streams music frames to an emulated `Ym2149` chip instance and generates PCM audio buffers.
- **`YmSfxRenderer`**: Renders isolated sound effects on designated channels.
- **`YmMixer`**: Real-time multi-channel mixer supporting dynamic SFX triggering, voice takeover, and priority arbitration.
