; ============================================================
; tfx.inc -- Atari TIA Sound Effect Format (.tfx) & Replayer
; ca65 / cc65 toolchain
; ============================================================

.struct TTiaSfxState
    ptr         .addr       ; Pointer to current 3-byte frame [AUDF, AUDC, AUDV]
    active      .byte       ; 1 = active, 0 = idle
    frames_left .byte       ; Remaining frames in effect
.endstruct
