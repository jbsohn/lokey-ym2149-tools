; =========================================================
; ysg_player.s -- Standalone YSG Music Player for Atari ST
; rmac (68000 mode), producing a .tos directly with -p
; =========================================================
; Plays YSG background music streams rendered from .ym tracks.
; Pressing 'Q' quits cleanly to the GEMDOS desktop.
; =========================================================

        .include "../include/st.inc"
        .include "../include/ym2149.inc"
        .include "ysg.inc"

; PLAYER_HZ must match the rate the song was rendered at (the
; Makefile renders at 50Hz, `lym`'s default). DISPLAY_HZ is the
; physical VBL rate: 60Hz on an NTSC ST, 50Hz on PAL. Override either
; at assemble time with -dPLAYER_HZ=n / -dDISPLAY_HZ=n.
        .if !(^^defined PLAYER_HZ)
PLAYER_HZ       equ 50
        .endif
        .if !(^^defined DISPLAY_HZ)
DISPLAY_HZ      equ 60
        .endif

; Rate-conversion step per display frame; 0 is the sentinel for
; "play every frame" (PLAYER_HZ == DISPLAY_HZ). Masked to 16 bits
; because PLAYER_HZ == DISPLAY_HZ makes the ratio exactly 65536,
; which must wrap around to the 0 sentinel.
MUSIC_DELTA     equ ((PLAYER_HZ*65536)/DISPLAY_HZ)&$ffff

        .text
        .globl  start

start:
        ; Super(0L) -- GEMDOS call $20, switch to supervisor mode and stay
        ; there (we never return to user code; Q exits via Pterm0).
        clr.l   -(sp)
        move.w  #$20,-(sp)
        trap    #1
        addq.l  #6,sp

        ; Save original 16-color Shifter palette before modifying BORDER_COL
        movem.l PALETTE_BASE.w,d0-d7
        movem.l d0-d7,saved_palette

        bclr    #2,CONTERM.w            ; keyclick off

        ; Stop TOS's own IKBD interrupt handler from consuming raw keyboard bytes
        bclr    #6,MFP_IERB.w
        bclr    #6,MFP_IMRB.w

        ; Send "Disable Mouse" ($12) to IKBD to prevent mouse movement packets
        ; from generating spurious scancodes in our polling loop.
.wait_tx:
        btst    #1,KBD_ACIA_CTRL.w      ; check TDRE (Transmit Data Register Empty)
        beq.s   .wait_tx
        move.b  #$12,KBD_ACIA_DATA.w

        bsr     silence_psg
        bsr     init_music
        move.l  VBLCLOCK,last_vbl

main_loop:
        bsr     sync_vbl
        bsr     poll_keyboard
        bsr     update_visuals

        move.w  #MUSIC_DELTA,d0
        beq.s   .play_now
        add.w   d0,music_acc
        bcc.s   .skip_play
.play_now:
        bsr     play_frame
.skip_play:
        bra     main_loop

; ----------------------------------------------------------
silence_psg:
        moveq   #NUM_REGS-1,d0
.loop:  move.b  d0,PSG_SELECT.w
        clr.b   PSG_DATA.w
        dbra    d0,.loop
        rts

; ----------------------------------------------------------
; sync_vbl -- block until TOS's VBL counter ticks over
; ----------------------------------------------------------
sync_vbl:
        move.l  last_vbl,d0
.wait:  cmp.l   VBLCLOCK,d0
        beq.s   .wait
        move.l  VBLCLOCK,last_vbl
        rts

; ----------------------------------------------------------
; poll_keyboard -- drain IKBD scancodes; Q quits to desktop
; ----------------------------------------------------------
poll_keyboard:
.loop:  btst    #0,KBD_ACIA_CTRL.w
        beq.s   .done
        move.b  KBD_ACIA_DATA.w,d0     ; bit7=0 press / 1 release, low 7 bits = scancode
        move.b  d0,d1
        andi.b  #$7f,d1

        cmp.b   #SC_Q,d1
        bne.s   .loop
        tst.b   d0
        bpl     quit_to_desktop         ; press only (bit7 clear); never returns
        bra.s   .loop

.done:  rts

; ----------------------------------------------------------
; quit_to_desktop -- silence PSG and Pterm0() back to GEMDOS
; ----------------------------------------------------------
quit_to_desktop:
        bsr     silence_psg

        ; Restore original Shifter palette for GEMDOS desktop
        movem.l saved_palette,d0-d7
        movem.l d0-d7,PALETTE_BASE.w

.wait_tx:
        btst    #1,KBD_ACIA_CTRL.w      ; check TDRE (Transmit Data Register Empty)
        beq.s   .wait_tx
        move.b  #$08,KBD_ACIA_DATA.w    ; restore relative mouse mode ($08) for TOS/GEM

        bset    #6,MFP_IERB.w           ; restore keyboard/MIDI ACIA interrupt
        bset    #6,MFP_IMRB.w
        clr.w   -(sp)
        trap    #1                      ; Pterm0() -- does not return

; ----------------------------------------------------------
; update_visuals -- cycle border color to indicate active playback
; ----------------------------------------------------------
update_visuals:
        move.l  VBLCLOCK,d0
        lsr.l   #2,d0                   ; slow down color transition
        andi.w  #$0007,d0
        lsl.w   #8,d0                   ; shift into red component of $0R00
        ori.w   #$0020,d0               ; add constant subtle green component
        move.w  d0,BORDER_COL.w
        rts

; ----------------------------------------------------------
; init_music -- initialize player state from YSG header
; ----------------------------------------------------------
init_music:
        clr.b   seq_idx
        clr.b   pat_frames
        clr.b   rle_count
        clr.w   music_acc

        lea     music_data,a0
        move.b  YSG_PAT_SIZE(a0),pat_size
        move.b  YSG_SEQ_LEN(a0),seq_len
        move.b  YSG_LOOP_PAT(a0),loop_pat
        move.b  YSG_LAST_PAT_FRAMES(a0),last_pat_frames
        move.b  YSG_FEATURES(a0),features

        lea     YSG_HEADER_SIZE(a0),a1  ; seq_base = music_data + header size
        move.l  a1,seq_base

        moveq   #0,d0
        move.b  seq_len,d0
        adda.l  d0,a1                   ; pat_table = seq_base + seq_len
        move.l  a1,pat_table

        moveq   #0,d0
        move.b  YSG_NUM_UNIQUE(a0),d0
        lsl.w   #2,d0                   ; d0 = num_unique * 4 (4-byte offset entries)
        move.l  pat_table,a1
        adda.w  d0,a1                   ; pat_base = pat_table + num_unique*4
        move.l  a1,pat_base
        rts

; ----------------------------------------------------------
; play_frame -- advance one music frame, write YM2149 regs
; ----------------------------------------------------------
play_frame:
        tst.b   rle_count
        beq.s   .not_rle_idle
        subq.b  #1,rle_count
        subq.b  #1,pat_frames
        rts

.not_rle_idle:
        tst.b   pat_frames
        bne     .do_play

        moveq   #0,d0
        move.b  seq_idx,d0
        moveq   #0,d1
        move.b  seq_len,d1
        cmp.w   d1,d0
        bcs.s   .load_pattern

        ; Sequence exhausted -- loop or restart
        move.b  loop_pat,d0
        cmp.b   #$ff,d0
        bne.s   .do_loop
        bsr     init_music              ; no loop point: restart from beginning
        rts
.do_loop:
        move.b  d0,seq_idx

.load_pattern:
        move.l  seq_base,a0
        moveq   #0,d1
        move.b  seq_idx,d1
        move.b  (a0,d1.w),d2            ; pattern index
        addq.b  #1,seq_idx

        moveq   #0,d1
        move.b  d2,d1
        lsl.w   #2,d1                   ; d1 = idx * 4 (4-byte offset entries)
        move.l  pat_table,a0
        adda.w  d1,a0
        bsr     read_le32               ; d0.l = pattern byte offset; a0 += 4
        add.l   pat_base,d0
        move.l  d0,music_ptr

        ; Use last_pat_frames for the final sequence entry, pat_size otherwise.
        tst.b   last_pat_frames
        beq.s   .use_pat_size
        moveq   #0,d0
        move.b  seq_idx,d0
        moveq   #0,d1
        move.b  seq_len,d1
        cmp.w   d1,d0
        bne.s   .use_pat_size
        move.b  last_pat_frames,pat_frames
        bra     .do_play
.use_pat_size:
        move.b  pat_size,pat_frames

.do_play:
        subq.b  #1,pat_frames

        move.l  music_ptr,a0
        bsr     read_le16               ; d0.w = delta mask; a0 += 2
        move.w  d0,d3
        move.l  a0,music_ptr

        ; RLE token: features bit0 enables it, mask bit15 flags it.
        btst    #0,features
        beq.s   .rle_done
        btst    #15,d3
        beq.s   .rle_done

        ; Count byte N -- N further idle frames beyond the current frame
        move.l  music_ptr,a0
        move.b  (a0)+,d0
        move.l  a0,music_ptr
        move.b  d0,rle_count
        rts

.rle_done:
        ; Bit n of the mask corresponds directly to register n
        moveq   #0,d4
        move.l  music_ptr,a0
.reg_loop:
        btst    d4,d3
        beq.s   .reg_next
        move.b  d4,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w
.reg_next:
        addq.b  #1,d4
        cmp.b   #NUM_REGS,d4
        bne.s   .reg_loop

        move.l  a0,music_ptr
        rts

; ----------------------------------------------------------
; read_le16 -- little-endian 16-bit read
; in:  a0 = pointer      out: d0.w = value, a0 advanced by 2
; ----------------------------------------------------------
read_le16:
        moveq   #0,d0
        move.b  1(a0),d0
        lsl.w   #8,d0
        move.b  0(a0),d0
        addq.l  #2,a0
        rts

; ----------------------------------------------------------
; read_le32 -- little-endian 32-bit read
; in:  a0 = pointer      out: d0.l = value, a0 advanced by 4
; ----------------------------------------------------------
read_le32:
        moveq   #0,d0
        move.b  3(a0),d0
        lsl.l   #8,d0
        move.b  2(a0),d0
        lsl.l   #8,d0
        move.b  1(a0),d0
        lsl.l   #8,d0
        move.b  0(a0),d0
        addq.l  #4,a0
        rts

; ----------------------------------------------------------
; Player state
; ----------------------------------------------------------
        .bss

last_vbl:           ds.l 1

music_ptr:          ds.l 1
pat_table:          ds.l 1
pat_base:           ds.l 1
seq_base:           ds.l 1
pat_frames:         ds.b 1
seq_idx:            ds.b 1
pat_size:           ds.b 1
seq_len:            ds.b 1
loop_pat:           ds.b 1
last_pat_frames:    ds.b 1
features:           ds.b 1
rle_count:          ds.b 1
music_acc:          ds.w 1

saved_palette:      ds.w 16
