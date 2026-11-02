# LymReference — CLI Toolchain Manual

`lym` is the unified command-line interface for compiling, auditioning, dumping, and interactively mixing YM-2149 music
streams and sound effects.

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

Compiles a source music song (`.ym`, `.vgm`, `.vgz`, or `.json`) into an optimized `.ysg` binary stream and generates an
accompanying `.ysi` ca65 assembly include file. Furnace Tracker songs export as `.vgm` / `.vgz`; see
[File Formats](FileFormats.md) for the half-clock flag, frame rate, and envelope-retrigger rules.

```bash
lym song render --input <PATH> [OPTIONS]
```

#### Options

| Option             | Flag | Description                                                                                                                                                                                                                                                                                  | Default                          |
|:-------------------|:-----|:---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|:---------------------------------|
| `--input`          | `-i` | **Required**. Path to input song file (`.ym` chiptune, `.vgm` / `.vgz` Furnace VGM log, `.ysg`, or `.json` source).                                                                                                                                                                          | —                                |
| `--output`         | `-o` | Output `.ysg` binary path. If omitted, uses input path with `.ysg` extension.                                                                                                                                                                                                                | Same stem + `.ysg`               |
| `--hz`             |      | Force playback refresh rate override (`50` or `60` Hz).                                                                                                                                                                                                                                      | Source Hz                        |
| `--clock`          |      | Override source/target PSG master clock in Hz (e.g. `1789773` for Atari 7800, `2000000` for Atari ST).                                                                                                                                                                                       | Source clock                     |
| `--target-clock`   |      | Target chip clock in Hz that pitches are retuned for (e.g. `2000000` to keep an Atari ST source at native pitch, `1020484` for Apple II Mockingboard/AppleWin — real Mockingboard hardware clocks the AY-3-8910 off the 6502 clock, so notes play back roughly an octave flat without this). | `1789773` (Atari 7800)           |
| `--step`           | `-s` | Decimation window size for temporal frame reduction (e.g. `--step 2` reduces 50 Hz to 25 Hz).                                                                                                                                                                                                | `1`                              |
| `--pattern-frames` |      | Fixed pattern frame chunk size (e.g. 16, 24, 32, 48, 64, 96, 128). If omitted, `lym` automatically tests all candidate sizes and selects the one that minimizes total stream size.                                                                                                          | Automatic optimal search         |
| `--max-bytes`      |      | Truncate the song, dropping whole patterns from the end, until the compiled `.ysg` fits within `N` bytes. Prints a warning with the number of frames dropped. If the loop point is cut off, the song restarts from the beginning instead.                                                    | No limit                         |

For `.vgm` and `.vgz`, `--clock` replaces the clock in the VGM header and skips the YM2149 pin-26 half-clock divide. `--target-clock` retunes periods from that source clock to the playback clock. The rules are in [File Formats](FileFormats.md).

---

### Compiler Architecture & Optimization Passes

`lym song render` applies several pre-computation passes to maximize compression efficiency while keeping 6502 replayer CPU cost minimal:

1. **Automatic Pattern Frame Exploration**:
   By default, `lym` benchmarks candidate pattern sizes (`[16, 24, 32, 48, 64, 96, 128]`) and automatically selects the size that yields the smallest total compressed binary for the given song. For looping songs, candidate pattern sizes that divide the frame count without remainder and preserve exact loop alignment are prioritized to eliminate silence gaps at loop boundaries. You can override this and lock in a specific pattern size using `--pattern-frames <N>`. The stored length is `ceil(frames / pattern_frames)` steps, at most 255. If a song's length does not divide evenly into the pattern size, silent padding frames (at most `pattern_frames - 1`) are appended to the final pattern, and `lym` prints a warning. A loop point is stored as the pattern step that contains it (`frame / pattern_frames`).

2. **4-Stream Decoupled Architecture**:
   The compiler separates the song into four independent streams: Voice A, Voice B, Voice C, and Global/Envelope. This allows individual channels to repeat and deduplicate patterns independently.

3. **Run-Length Idle Frame Skipping**:
   When a channel sustains a note or remains silent, runs of idle frames (1 to 127 frames) are collapsed into single-byte wait tokens (`0bbbbbbb`). The 6502 replayer handles this in just 12 CPU cycles (`BPL .is_wait`) without reading or writing PSG registers.

4. **Precomputed Hardware Mixer (R7)**:
   All tone and noise enables across channels A, B, and C are resolved at build time into precomputed R7 mixer register values in the Global stream. The 6502 replayer writes R7 directly without performing runtime bitmask operations.

5. **Temporal Frame Decimation (`-s, --step <N>`)**:
   Merges `N`-frame windows by selecting peak volume and tone values per channel while preserving envelope parameters. For long songs exceeding a 32KB flat cartridge ROM budget, `--step 2` reduces the frame count by 50% while scaling the replayer playback rate accumulator step appropriately.

*(For detailed binary specifications of the `.ysg` container layout and stream opcodes, see the [File Formats Specification](FileFormats.md)).*

---

### `lym song dump`

Dumps raw YM2149 register field values for diagnostic inspection and frame analysis.

```bash
lym song dump --input <PATH> [OPTIONS]
```

#### Options

| Option           | Flag | Description                                                                        | Default      |
|:-----------------|:-----|:-----------------------------------------------------------------------------------|:-------------|
| `--input`        | `-i` | **Required**. Path to input song file (`.ym`, `.vgm`, `.vgz`, `.ysg`, or `.json`). | —            |
| `--frames`       | `-f` | Number of sequential frames to inspect.                                            | `100`        |
| `--start`        |      | Starting zero-indexed frame offset.                                                | `0`          |
| `--clock`        |      | Override source PSG master clock in Hz.                                            | File default |
| `--target-clock` |      | Retune pitches to target PSG master clock in Hz.                                   | Native pitch |

---

### `lym song play`

Auditions a music song file directly through the system default audio speaker output via cycle-accurate YM2149
emulation. Supports uncompressed `.ym` chiptunes, Furnace `.vgm` / `.vgz` logs, and compiled `.ysg` binary streams,
enabling direct A/B audio comparison between the source music and the compiled payload that will run on target hardware.

```bash
# Step 1: Audition original uncompressed Atari ST .ym source track:
lym song play --input tests/fixtures/song/ND-Loader.ym

# Step 2: Render & compress into .ysg binary (also auto-generates .ysi ca65 include):
lym song render --input tests/fixtures/song/ND-Loader.ym --output tests/fixtures/song/ND-Loader.ysg

# Step 3: Audition compiled target .ysg cartridge binary stream:
lym song play --input tests/fixtures/song/ND-Loader.ysg

# Furnace Tracker exports VGM. Play the log, compile it, then play the .ysg:
lym song play --input song.vgm
lym song render --input song.vgm --output song.ysg
lym song play --input song.ysg
```

A song with a loop point plays until `q`. Left and right arrow seek by five seconds. The compiled file can run up to one pattern longer than the source if padding is needed, and its loop starts at the beginning of the pattern that contained the source loop.

#### Options

| Option           | Flag | Description                                                                                                   | Default      |
|:-----------------|:-----|:--------------------------------------------------------------------------------------------------------------|:-------------|
| `--input`        | `-i` | **Required**. Path to input song file (`.ym`, `.vgm`, `.vgz`, `.ysg`, or `.json`).                            | —            |
| `--hz`           |      | Playback refresh rate override (`50` or `60` Hz).                                                             | File default |
| `--raw`          |      | Play `.ym` files with the raw YM replayer, without interactive seeking controls. Emits warning on non-`.ym`. | `false`      |
| `--clock`        |      | Override source PSG master clock in Hz.                                                                       | File default |
| `--target-clock` |      | Retune pitches to target PSG master clock in Hz.                                                              | Native pitch |

---

## 2. Sound Effect Subcommands (`lym sfx`)

### `lym sfx render`

Compiles a sound effect source (`.json`, `.csv`, `.afx`, `.afb`) into a 5-byte fixed-width `.yfx` payload and generates
a `.yfi` ca65 include file.

```bash
lym sfx render --input <PATH> [OPTIONS]
```

#### Options

| Option     | Flag | Description                                                                   | Default            |
|:-----------|:-----|:------------------------------------------------------------------------------|:-------------------|
| `--input`  | `-i` | **Required**. Path to input SFX file (`.json`, `.csv`, `.afx`, `.afb`).       | —                  |
| `--output` | `-o` | Output `.yfx` binary path. If omitted, uses input path with `.yfx` extension. | Same stem + `.yfx` |
| `--hz`     |      | Playback rate in Hz (`50` or `60`).                                           | File default       |
| `--clock`  |      | Source chip clock in Hz.                                                      | File default       |
| `--index`  |      | Zero-indexed sound effect selection when rendering multi-effect `.afb` banks. | `0`                |

---

### `lym sfx play`

Auditions a sound effect sequence through system audio via cycle-accurate YM2149 emulation. Supports both raw source
files (`.json`, `.csv`, `.afx`, `.afb`) and compiled target `.yfx` binaries, enabling direct A/B audio comparison before
and after compilation.

```bash
# Step 1: Audition raw JSON sound effect source:
lym sfx play --input tests/fixtures/sfx/blip.json

# Step 2: Render into 5-byte fixed-width .yfx payload (also auto-generates .yfi ca65 include):
lym sfx render --input tests/fixtures/sfx/blip.json --output tests/fixtures/sfx/blip.yfx

# Step 3: Audition compiled .yfx target binary payload:
lym sfx play --input tests/fixtures/sfx/blip.yfx
```

#### Options

| Option    | Flag | Description                                                               | Default      |
|:----------|:-----|:--------------------------------------------------------------------------|:-------------|
| `--input` | `-i` | **Required**. Path to SFX file (`.json`, `.csv`, `.afx`, `.afb`, `.yfx`). | —            |
| `--hz`    |      | Playback rate override (`50` or `60` Hz).                                 | File default |
| `--clock` |      | Source chip clock in Hz.                                                  | File default |
| `--index` |      | Zero-indexed effect selection for `.afb` banks.                           | `0`          |

---

## 3. Interactive Mixer Subcommand (`lym mix`)

Interactively mixes background music playback with keyboard-triggered sound effects for live testing and channel
conflict arbitration.

```bash
lym mix --song <SONG_PATH> --sfx <SFX_PATHS...> [OPTIONS]
```

#### Options

| Option           | Flag | Description                                                                                             | Default                |
|:-----------------|:-----|:--------------------------------------------------------------------------------------------------------|:-----------------------|
| `--song`         | `-s` | **Required**. Background song file (`.ysg`, `.ym`, `.vgm`, `.vgz`, `.json`).                            | —                      |
| `--sfx`          | `-e` | **Required**. One or more sound effect files or banks (`.yfx`, `.json`, `.csv`, `.afx`, `.afb`).        | —                      |
| `--channel`      | `-c` | Preferred primary YM channel for sound effects (`a`, `b`, or `c`).                                      | `c`                    |
| `--hz`           |      | Playback rate override (`50` or `60` Hz).                                                               | Song default           |
| `--clock`        |      | Source chip clock in Hz.                                                                                | `2000000` (Atari ST)   |
| `--target-clock` |      | Target chip clock in Hz to scale pitch for (e.g. `2000000` to keep an Atari ST source at native pitch). | `1789773` (Atari 7800) |

#### Interactive Key Controls

* `1`–`9`, `0`, `SPACE`: Trigger sound effects from loaded bank.
* `←` / `→`: Seek backward / forward by 5-second intervals.
* `q` / `Q`: Mute audio and quit mixer.
