; ============================================================
; ysg.inc -- YM-2149 Song Format (.ysg)
; ca65 / cc65 toolchain
; ============================================================

.struct TYsgHeader
    pat_size        .byte       ; frames per pattern block
    num_unique      .byte       ; unique pattern count
    seq_len         .byte       ; entries in sequence table
    loop_pat        .byte       ; sequence index to loop back to (255 = no loop)
    frame_rate      .dword      ; music frame rate in Hz (u32 LE, typically 50 or 60)
    master_clk      .dword      ; YM2149 master clock in Hz (u32 LE, e.g. 1789773)
    last_pat_frames .byte       ; real frames in last seq entry (0 = full pattern)
    features        .byte       ; compression feature flags (bit0=RLE, bit1=varlen mask)
.endstruct

.struct TPlayerState
    music_ptr       .addr       ; read pointer into pattern data
    pat_frames      .byte       ; frames remaining in current pattern
    seq_idx         .byte       ; current index into sequence table
    tmp_mask        .word       ; scratch / current frame delta mask
    pat_table       .addr       ; pointer to offset table
    pat_base        .addr       ; pointer to pattern data
    seq_base        .addr       ; pointer to sequence table
    pat_size        .byte       ; frames per pattern (from header)
    seq_len         .byte       ; sequence table length (from header)
    loop_pat        .byte       ; loop sequence index (from header; 255 = no loop)
    last_pat_frames .byte       ; real frames in last seq entry (0 = full pattern)
    features        .byte       ; compression feature flags (from header)
    music_acc       .word       ; rate-conversion accumulator
    music_delta     .word       ; rate-conversion step per display frame
    v_frame         .word       ; display frame counter (for visuals)
.endstruct
