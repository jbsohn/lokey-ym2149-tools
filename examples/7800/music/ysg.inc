; ============================================================
; ysg.inc -- YM-2149 Song Format (.ysg)
; ca65 / cc65 toolchain
; ============================================================

; --- YSG 20-byte file container header ---
.struct TYsgHeader
    magic           .byte 2     ; "YS" (0x59, 0x53) or "YC"
    version         .byte       ; 0x01
    pattern_frames  .byte       ; frames per pattern block (e.g. 48)
    seq_len         .byte       ; entries in sequence table
    loop_step       .byte       ; sequence step to loop back to (255 = no loop)
    frame_rate_hz   .word       ; music frame rate in Hz (u16 LE, 50 or 60)
    master_clock_hz .dword      ; YM2149 master clock in Hz (u32 LE)
    offset_track_a  .word       ; relative offset to Track A descriptor
    offset_track_b  .word       ; relative offset to Track B descriptor
    offset_track_c  .word       ; relative offset to Track C descriptor
    offset_track_glob .word     ; relative offset to Track Global descriptor
.endstruct

; --- YSG 14-byte Zero-Page State ---
.struct TYsgPlayerState
    ptr_a           .addr       ; Stream read pointer for Track A (2 bytes)
    ptr_b           .addr       ; Stream read pointer for Track B (2 bytes)
    ptr_c           .addr       ; Stream read pointer for Track C (2 bytes)
    ptr_glob        .addr       ; Stream read pointer for Track Global (2 bytes)
    wait_a          .byte       ; Run-length wait counter for Track A (1 byte)
    wait_b          .byte       ; Run-length wait counter for Track B (1 byte)
    wait_c          .byte       ; Run-length wait counter for Track C (1 byte)
    wait_glob       .byte       ; Run-length wait counter for Track Global (1 byte)
    pat_frame       .byte       ; Frame countdown within current pattern (1 byte)
    seq_step        .byte       ; Current sequence step index (1 byte)
.endstruct
