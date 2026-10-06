; ============================================================
; ysg.inc -- YM-2149 Song Format (.ysg) -- 68000 / rmac
; ============================================================

        .if !(^^defined YSG_INC)
YSG_INC equ 1

; --- YSG 20-byte file container header ---
        .abs
YSG_MAGIC:              ds.b 2     ; "YS" (0x59, 0x53) or "YC"
YSG_VERSION:            ds.b 1     ; 0x01
YSG_PAT_FRAMES:         ds.b 1     ; frames per pattern block (e.g. 48)
YSG_SEQ_LEN:            ds.b 1     ; entries in sequence table
YSG_LOOP_STEP:          ds.b 1     ; sequence step to loop back to (255 = no loop)
YSG_FRAME_RATE_HZ:      ds.w 1     ; music frame rate in Hz (u16 LE, 50 or 60)
YSG_MASTER_CLOCK_HZ:    ds.l 1     ; YM2149 master clock in Hz (u32 LE)
YSG_OFFSET_TRACK_A:     ds.w 1     ; relative offset to Track A descriptor
YSG_OFFSET_TRACK_B:     ds.w 1     ; relative offset to Track B descriptor
YSG_OFFSET_TRACK_C:     ds.w 1     ; relative offset to Track C descriptor
YSG_OFFSET_TRACK_GLOB:  ds.w 1     ; relative offset to Track Global descriptor
YSG_HEADER_SIZE equ ^^abscount     ; = 20
        .text

        .endif
