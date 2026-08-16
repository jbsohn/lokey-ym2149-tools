# Atari 7800 Dual-Chip Audio Demo (YM-2149 + TIA)

A complete, self-contained 6502 assembly sample for the **Atari 7800** demonstrating **simultaneous 5-channel dual-chip audio playback** with **zero voice stealing**.

---

## Hardware Architecture & Channel Allocation

```
                        +---------------------------------------+
                        |         Atari 7800 Sound Engine       |
                        +---------------------------------------+
                                           |
                +--------------------------+--------------------------+
                |                                                     |
        [ 3-Voice Music ]                                     [ 2-Voice SFX ]
        Yamaha YM-2149 PSG                                  Atari TIA Sound
   Mapped at $0800 (Addr) / $0801 (Data)                 Mapped in Zero-Page ($15-$1A)
  +-------------------------------------+               +-------------------------------+
  | Ch A: Melody / Lead Arpeggios       |               | Ch 0: Star Raiders Laser      |
  | Ch B: Polyphonic Chords / Harmony   |               |       Arcade Jump Boing       |
  | Ch C: Bassline & PSG Drums          |               |       Two-Tone Coin Chime     |
  +-------------------------------------+               | Ch 1: Crunchy Explosion Blast |
                                                        +-------------------------------+
```

- **YM-2149 Cartridge Expansion ($0800/$0801)**:
  - Streams full 3-channel background music (`ND-Loader.ysg`) continuously.
  - Channels A, B, and C are **never ducked or stolen** by sound effects.
- **Onboard Atari TIA ($15–$1A)**:
  - Streams dedicated sound effects directly into zero-page registers.
  - Takes only **9 to 12 CPU cycles** per channel during VBLANK.

---

## Controller Controls

| Input | Channel | Effect | Description |
| :--- | :--- | :--- | :--- |
| **Button 1 / Fire 1** (`INPT4`) | **TIA Ch 0** | **Star Raiders Laser** | Doug Neubauer *Star Raiders* photon torpedo sweep |
| **Button 2 / Fire 2** (`INPT0`) | **TIA Ch 1** | **Explosion Blast** | White noise impact blast with long decay |
| **Joystick Up** (`SWCHA` bit 4) | **TIA Ch 0** | **Arcade Jump** | Upward Pitfall-style spring/boing jump sweep |
| **Joystick Down** (`SWCHA` bit 5) | **TIA Ch 0** | **Coin Pickup** | High-frequency two-tone chime |

---

## Dynamic Visual Feedback (MARIA)

The sample changes the screen backdrop color (`BKGRND` at `$20`) in real time to visualize audio activity:

- **Deep Midnight Blue (`$06`)**: Idle state (3-channel YM-2149 background music playing).
- **Electric Cyan (`$9A`)**: TIA Channel 0 active (Laser, Jump, or Coin firing).
- **Blazing Orange (`$3A`)**: TIA Channel 1 active (Explosion blast firing).
- **Bright White Flash (`$0F`)**: Both TIA channels active simultaneously!

---

## Building the Sample

### Prerequisites
1. **Rust Toolchain** (`cargo`)
2. **cc65 Toolchain** (`ca65`, `ld65`) on your `PATH`.
3. *(Optional)* [`a78tool`](https://github.com/jbsohn/lokey-7800-tools) for packaging `.a78` emulator headers.

### Build Commands
```bash
# Build the dual-chip sample ROM:
make

# Clean build artifacts:
make clean
```

### Output Files
- **`build/dual_chip_mix.a78`**: Ready-to-play ROM with standard 128-byte A78 header (for BupSystem, A7800, MAME, and flash carts).
- **`build/dual_chip_mix.bin`**: Raw 32 KB binary ROM.
- **`build/dual_chip_mix.rom`**: Signed 32 KB ROM.
