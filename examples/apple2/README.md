# Apple II (Mockingboard)

Plays `.ysg` songs compiled by `lym` on an Apple II with a Mockingboard
sound card (AY-3-8910 behind a 6522 VIA).

**Only tested in emulation (AppleWin).** The VIA/AY bus protocol was
verified against AppleWin's Mockingboard emulation source, not against real
hardware — untested on an actual Apple II + Mockingboard card.

## Layout

- `include/mockingboard.inc` — VIA/AY register addresses, bus-control constants.
- `music/ysg_player.s` — the player (ca65 6502 assembly).
- `music/ysg.inc` — shared `.ysg` format struct definitions.
- `music/Makefile` — builds songs and a bootable disk image.

## Requirements

- `cc65` toolchain (`ca65`, `ld65`) on `PATH`.
- [AppleCommander](https://applecommander.github.io/) CLI (`applecommander-ac`) on `PATH`.
- `lym` built in release mode (`make disk` builds it automatically if missing).
- A DOS 3.3 System Master disk image, supplied by you — **not included in
  this repo**. It's Apple's copyrighted software; grab your own copy (it
  ships with [AppleWin](https://github.com/AppleWin/AppleWin), among other
  places) and point `APPLEWIN_TEMPLATE` at it.
- [AppleWin](https://github.com/AppleWin/AppleWin) to actually run it.
  Should work on real Apple II + Mockingboard hardware too, but that's
  unverified — see the note above.

## Build

```sh
cd music
make -k all          # render + link every .ym in tests/fixtures/song
make disk             # build a bootable build/music.dsk (defaults to DISK_SONG=ND-Loader)
make disk DISK_SONG=enchant1
```

Useful variables (see `make help`): `YSG_MAX_BYTES`, `TARGET_CLOCK`,
`DISK_SONG`, `DISK_IMAGE`, `APPLEWIN_TEMPLATE`.

## Run

1. In AppleWin, Configuration → Slot 4 → **Mockingboard**.
2. Boot `build/music.dsk`. It auto-runs the song via a generated `HELLO`.
3. Press Return to stop and return to `]`.

To run a different built song manually from the `]` prompt: `BRUN NAME`
(e.g. `BRUN ENCHANT1`).

## Notes

- Default slot is 4 (`$C400`); override at assemble time with `-D MOCK_SLOT=n`.
- Songs longer than `YSG_MAX_BYTES` get truncated to fit Apple II RAM
  (`$0803`-`$9600`, shared with the ~500-byte player) — the build warns when
  this happens. Truncation cuts at an arbitrary byte boundary, not the
  song's musical loop point, so a truncated song usually can't loop
  correctly: if the original loop point falls past the cutoff, looping is
  disabled and the player just restarts from frame 0 instead, which sounds
  like an abrupt cut rather than a clean loop.
- `TARGET_CLOCK` (default `1020484`, the Apple II's real 6502/AY clock) is
  what pitches get retuned against; `ym-core` otherwise defaults to the
  Atari 7800's clock (`1789773`).
