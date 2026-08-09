; ============================================================
; ysg.inc -- YM-2149 Song Format (.ysg) -- 68000 / rmac
; ============================================================

        .if !(^^defined YSG_INC)
YSG_INC equ 1

        .abs
YSG_PAT_SIZE:            ds.b 1     ; frames per pattern block
YSG_NUM_UNIQUE:          ds.b 1     ; unique pattern count
YSG_SEQ_LEN:             ds.b 1     ; entries in sequence table
YSG_LOOP_PAT:            ds.b 1     ; sequence index to loop back to (255 = no loop)
YSG_FRAME_RATE:          ds.l 1     ; music frame rate in Hz (u32 LE, unused on-target)
YSG_MASTER_CLK:          ds.l 1     ; YM2149 master clock in Hz (u32 LE, unused on-target)
YSG_LAST_PAT_FRAMES:     ds.b 1     ; real frames in last seq entry (0 = full pattern)
YSG_FEATURES:            ds.b 1     ; compression feature flags (bit0=RLE)
YSG_HEADER_SIZE equ ^^abscount      ; = 14
        .text

        .endif
