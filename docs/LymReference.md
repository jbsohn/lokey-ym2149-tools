# LymReference — CLI Toolchain Manual

`lym` is the unified command-line interface for compiling, auditioning, dumping, and interactively mixing YM-2149 music streams and sound effects.

---

## Command Overview

```
lym <COMMAND>

Commands:
  song  Music song tools (compile, play, dump)
  sfx   Sound effect tools (compile, play)
  mix   Real-time interactive music & sound effect keyboard mixer
```

---

## 1. Song Subcommands (`lym song`)

### `lym song render`

Compiles a source music song (`.ym` or `.json`) into an optimized `.ysg` binary stream and generates an accompanying `.ysi` ca65 assembly include file.

```bash
lym song render --input <PATH> [OPTIONS]
```

#### Options:

| Option | Flag | Description | Default |
|:---|:---|:---|:---|
| `--input` | `-i` | **Required**. Path to input song file (`.ym` chiptune or `.json` source). | — |
| `--output` | `-o` | Output `.ysg` binary path. If omitted, uses input path with `.ysg` extension. | Same stem + `.ysg` |
| `--hz` | | Force playback refresh rate override (`50` or `60` Hz). | Source Hz |
| `--clock` | | Override source/target PSG master clock in Hz (e.g. `1789773` for Atari 7800, `2000000` for Atari ST). | Source clock |
| `--step` | `-s` | Decimation window size for temporal frame reduction. | `1` |
| `--compression` | `-c` | Compression level: `full` (delta + pattern dedup), `delta-only` (no pattern dedup), or `none` (raw 14-register frames). | `full` |
| `--no-dedup` | | Disable pattern block deduplication. | `false` |
| `--no-rle` | | Disable run-length encoding (RLE) for idle frame runs. | `false` |
| `--max-bytes` | | Truncate trailing patterns to enforce a strict byte limit for tight ROM constraints. | Unbounded |

---

### Compiler Options & Optimization Passes

`lym song render` provides fine-grained control over compiler optimization passes. You can toggle individual techniques or select preset compression levels to balance ROM footprint against 6502 replayer CPU budget:

1. **Preset Compression Levels (`-c, --compression <LEVEL>`)**:
   * **`full` (Default)**: Enables delta bitmasking, pattern block deduplication, and RLE idle-frame tokens (~60%–85% compression vs raw `.ym`).
   * **`delta-only`**: Applies delta bitmasking across consecutive frames without pattern block deduplication. Useful for isolating pattern boundary issues.
   * **`none`**: Disables all compression, emitting raw 14-register values every single frame for baseline diagnostics.

2. **Pattern Block Deduplication (`--no-dedup`)**:
   * Chunks the song into fixed-length pattern blocks (benchmarking sizes from 8 to 255 frames). Identical blocks are deduplicated into a unique pattern table and indexed by an 8-bit sequence array.
   * First frame of every pattern block is unconditionally encoded with a full `0x3FFF` register mask for $O(1)$ seeking and clean SFX recovery. Pass `--no-dedup` to disable.

3. **Idle-Frame Run-Length Encoding (RLE) (`--no-rle`)**:
   * Collapses runs of 2+ consecutive idle frames into 3-byte RLE tokens (`[0x00, 0x80, N]`), allowing the 6502 replayer to skip register writes during silent/idle runs. Pass `--no-rle` to disable.

4. **Temporal Frame Decimation (`-s, --step <N>`)**:
   * Merges `N`-frame windows by selecting peak volume and tone values per channel while picking dominant noise and envelope parameters. E.g. `--step 2` downsamples 60 Hz songs to 30 Hz.

5. **Target ROM Size Truncation (`--max-bytes <BYTES>`)**:
   * Automatically truncates trailing pattern blocks if the compiled payload exceeds a target ROM size constraint.

*(For detailed binary specifications of the `.ysg` bitmask structure and header layouts, see the [File Formats Specification](FileFormats.md)).*

---

### `lym song dump`

Dumps raw YM2149 register field values for diagnostic inspection and frame analysis.

```bash
lym song dump --input <PATH> [OPTIONS]
```

#### Options:

| Option | Flag | Description | Default |
|:---|:---|:---|:---|
| `--input` | `-i` | **Required**. Path to input song file (`.ym`, `.ysg`, or `.json`). | — |
| `--frames` | `-f` | Number of sequential frames to inspect. | `100` |
| `--start` | | Starting zero-indexed frame offset. | `0` |

---

### `lym song play`

Auditions a music song file directly through the system default audio speaker output via cycle-accurate YM2149 emulation. Supports both original uncompressed `.ym` chiptune files and compiled `.ysg` binary streams, enabling direct A/B audio comparison between the source music and the compiled payload target that will run on target hardware.

```bash
# Step 1: Audition original uncompressed Atari ST .ym source track:
lym song play --input tests/fixtures/song/ND-Loader.ym

# Step 2: Render & compress into .ysg binary (also auto-generates .ysi ca65 include):
lym song render --input tests/fixtures/song/ND-Loader.ym --output tests/fixtures/song/ND-Loader.ysg

# Step 3: Audition compiled target .ysg cartridge binary stream:
lym song play --input tests/fixtures/song/ND-Loader.ysg
```

#### Options:

| Option | Flag | Description | Default |
|:---|:---|:---|:---|
| `--input` | `-i` | **Required**. Path to input song file (`.ym`, `.ysg`, or `.json`). | — |
| `--hz` | | Playback refresh rate override (`50` or `60` Hz). | File default |
| `--via-sequence` | | Force `.ym` files to decode through the `YmSequence` pipeline rather than raw VBL sync playback. | `false` |

---

## 2. Sound Effect Subcommands (`lym sfx`)

### `lym sfx render`

Compiles a sound effect source (`.json`, `.csv`, `.afx`, `.afb`) into a 5-byte fixed-width `.yfx` payload and generates a `.yfi` ca65 include file.

```bash
lym sfx render --input <PATH> [OPTIONS]
```

#### Options:

| Option | Flag | Description | Default |
|:---|:---|:---|:---|
| `--input` | `-i` | **Required**. Path to input SFX file (`.json`, `.csv`, `.afx`, `.afb`). | — |
| `--output` | `-o` | Output `.yfx` binary path. If omitted, uses input path with `.yfx` extension. | Same stem + `.yfx` |
| `--hz` | | Playback rate in Hz (`50` or `60`). | File default |
| `--clock` | | Source chip clock in Hz. | File default |
| `--index` | | Zero-indexed sound effect selection when rendering multi-effect `.afb` banks. | `0` |

---

### `lym sfx play`

Auditions a sound effect sequence through system audio via cycle-accurate YM2149 emulation. Supports both raw source files (`.json`, `.csv`, `.afx`, `.afb`) and compiled target `.yfx` binaries, enabling direct A/B audio comparison before and after compilation.

```bash
# Step 1: Audition raw JSON sound effect source:
lym sfx play --input tests/fixtures/sfx/blip.json

# Step 2: Render into 5-byte fixed-width .yfx payload (also auto-generates .yfi ca65 include):
lym sfx render --input tests/fixtures/sfx/blip.json --output tests/fixtures/sfx/blip.yfx

# Step 3: Audition compiled .yfx target binary payload:
lym sfx play --input tests/fixtures/sfx/blip.yfx
```

#### Options:

| Option | Flag | Description | Default |
|:---|:---|:---|:---|
| `--input` | `-i` | **Required**. Path to SFX file (`.json`, `.csv`, `.afx`, `.afb`, `.yfx`). | — |
| `--hz` | | Playback rate override (`50` or `60` Hz). | File default |
| `--clock` | | Source chip clock in Hz. | File default |
| `--index` | | Zero-indexed effect selection for `.afb` banks. | `0` |

---

## 3. Interactive Mixer Subcommand (`lym mix`)

Interactively mixes background music playback with keyboard-triggered sound effects for live testing and channel conflict arbitration.

```bash
lym mix --song <SONG_PATH> --sfx <SFX_PATHS...> [OPTIONS]
```

#### Options:

| Option | Flag | Description | Default |
|:---|:---|:---|:---|
| `--song` | `-s` | **Required**. Background song file (`.ysg`, `.ym`, `.json`). | — |
| `--sfx` | `-e` | **Required**. One or more sound effect files or banks (`.yfx`, `.json`, `.csv`, `.afx`, `.afb`). | — |
| `--channel` | `-c` | Preferred primary YM channel for sound effects (`a`, `b`, or `c`). | `c` |
| `--hz` | | Playback rate override (`50` or `60` Hz). | Song default |
| `--clock` | | Master clock override in Hz. | Song default |

#### Interactive Key Controls:

* `1`–`9`, `0`, `SPACE`: Trigger sound effects from loaded bank.
* `←` / `→`: Seek backward / forward by 5-second intervals.
* `q` / `Q`: Mute audio and quit mixer.

> [!TIP]
> **Jam Session Workflow**: Use `lym mix` as your interactive audio sandbox. Tap keys `1`-`9` while your track loops to make sure laser and explosion SFX cleanly override music channels without causing envelope clicks or channel distortion.

