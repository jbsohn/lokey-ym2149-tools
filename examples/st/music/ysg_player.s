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

        move.w  music_delta,d0
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
        clr.b   seq_step
        clr.b   pat_frame
        clr.b   wait_a
        clr.b   wait_b
        clr.b   wait_c
        clr.b   wait_glob
        clr.w   music_acc

        move.w  #MUSIC_DELTA,music_delta

        lea     music_data,a0
        move.b  YSG_PAT_FRAMES(a0),pat_frames_def
        move.b  YSG_SEQ_LEN(a0),seq_len_def
        move.b  YSG_LOOP_STEP(a0),loop_step_def

        ; Adapt music_delta dynamically if header frame_rate_hz is known
        move.b  YSG_FRAME_RATE_HZ+1(a0),d0
        lsl.w   #8,d0
        move.b  YSG_FRAME_RATE_HZ(a0),d0
        cmp.w   #50,d0
        bne.s   .not_50
        move.w  #((50*65536)/DISPLAY_HZ)&$ffff,music_delta
        bra.s   .delta_done
.not_50:
        cmp.w   #60,d0
        bne.s   .not_60
        move.w  #((60*65536)/DISPLAY_HZ)&$ffff,music_delta
        bra.s   .delta_done
.not_60:
        cmp.w   #25,d0
        bne.s   .not_25
        move.w  #((25*65536)/DISPLAY_HZ)&$ffff,music_delta
        bra.s   .delta_done
.not_25:
        cmp.w   #30,d0
        bne.s   .delta_done
        move.w  #((30*65536)/DISPLAY_HZ)&$ffff,music_delta
.delta_done:
        rts

; ----------------------------------------------------------
; load_step_patterns -- resolves track pointers for current seq_step
; ----------------------------------------------------------
load_step_patterns:
        lea     music_data,a0

        ; Track A
        move.b  YSG_OFFSET_TRACK_A+1(a0),d3
        lsl.w   #8,d3
        move.b  YSG_OFFSET_TRACK_A(a0),d3
        moveq   #8,d2                   ; Volume reg R8
        lea     ptr_a,a3
        lea     wait_a,a4
        bsr.s   setup_voice_track

        ; Track B
        lea     music_data,a0
        move.b  YSG_OFFSET_TRACK_B+1(a0),d3
        lsl.w   #8,d3
        move.b  YSG_OFFSET_TRACK_B(a0),d3
        moveq   #9,d2                   ; Volume reg R9
        lea     ptr_b,a3
        lea     wait_b,a4
        bsr.s   setup_voice_track

        ; Track C
        lea     music_data,a0
        move.b  YSG_OFFSET_TRACK_C+1(a0),d3
        lsl.w   #8,d3
        move.b  YSG_OFFSET_TRACK_C(a0),d3
        moveq   #10,d2                  ; Volume reg R10
        lea     ptr_c,a3
        lea     wait_c,a4
        bsr.s   setup_voice_track

        ; Track Global
        lea     music_data,a0
        move.b  YSG_OFFSET_TRACK_GLOB+1(a0),d3
        lsl.w   #8,d3
        move.b  YSG_OFFSET_TRACK_GLOB(a0),d3
        bsr.s   setup_global_track

        rts

; ----------------------------------------------------------
; setup_voice_track
; in:  d2.w = YM volume register (8, 9, or 10)
;      d3.w = track descriptor offset from music_data (unsigned)
;      a3   = pointer to ptr_x (long)
;      a4   = pointer to wait_x (byte)
; ----------------------------------------------------------
setup_voice_track:
        lea     music_data,a1
        andi.l  #$ffff,d3
        adda.l  d3,a1                   ; a1 = track descriptor base

        moveq   #0,d0
        move.b  seq_step,d0
        move.b  1(a1,d0.w),d1           ; d1 = pattern index for this step

        cmp.b   #$ff,d1
        bne.s   .has_pattern

        ; Sentinel empty pattern: silence voice and wait full pattern
        move.b  d2,PSG_SELECT.w
        clr.b   PSG_DATA.w
        move.b  pat_frames_def,(a4)
        suba.l  a0,a0
        move.l  a0,(a3)
        rts

.has_pattern:
        ; Pattern offset table starts at a1 + 1 + seq_len_def
        lea     1(a1),a2
        moveq   #0,d0
        move.b  seq_len_def,d0
        adda.l  d0,a2                   ; a2 = start of pattern offset table

        moveq   #0,d0
        move.b  d1,d0
        lsl.l   #1,d0                   ; d0 = pat_idx * 2
        adda.l  d0,a2                   ; a2 = entry pointer

        move.b  1(a2),d0
        lsl.w   #8,d0
        move.b  0(a2),d0
        andi.l  #$ffff,d0
        add.l   a1,d0                   ; d0 = absolute stream address

        move.l  d0,(a3)
        clr.b   (a4)
        rts

; ----------------------------------------------------------
; setup_global_track
; in:  d3.w = global track descriptor offset from music_data
; ----------------------------------------------------------
setup_global_track:
        lea     music_data,a1
        andi.l  #$ffff,d3
        adda.l  d3,a1                   ; a1 = global track descriptor base

        moveq   #0,d0
        move.b  seq_step,d0
        move.b  1(a1,d0.w),d1           ; d1 = pattern index

        lea     1(a1),a2
        moveq   #0,d0
        move.b  seq_len_def,d0
        adda.l  d0,a2                   ; a2 = pattern offset table

        moveq   #0,d0
        move.b  d1,d0
        lsl.l   #1,d0
        adda.l  d0,a2

        move.b  1(a2),d0
        lsl.w   #8,d0
        move.b  0(a2),d0
        andi.l  #$ffff,d0
        add.l   a1,d0

        move.l  d0,ptr_glob
        clr.b   wait_glob
        rts

; ----------------------------------------------------------
; play_frame -- advance one music frame, write YM2149 regs
; ----------------------------------------------------------
play_frame:
        tst.b   pat_frame
        bne.s   .no_advance

        ; Check if sequence ended
        move.b  seq_step,d0
        cmp.b   seq_len_def,d0
        bcs.s   .advance_step

        ; Sequence exhausted -- loop or restart
        move.b  loop_step_def,d0
        cmp.b   #$ff,d0
        bne.s   .do_loop
        clr.b   d0                      ; no loop point: restart from 0
.do_loop:
        move.b  d0,seq_step

.advance_step:
        bsr     load_step_patterns
        addq.b  #1,seq_step
        move.b  pat_frames_def,pat_frame

.no_advance:
        subq.b  #1,pat_frame

        ; ----------------------------------------------------
        ; Track Global: dispatched FIRST (R7 Mixer, R6 Noise, R11-R13 Env)
        ; ----------------------------------------------------
        tst.b   wait_glob
        beq.s   .read_glob
        subq.b  #1,wait_glob
        bra.s   .tick_a

.read_glob:
        movea.l ptr_glob,a0
        move.b  (a0)+,d0                ; opcode byte

        ; Check bit 7: 0 = wait run, 1 = register update
        bpl.s   .wait_glob_token

        ; Bit 0: R6 Noise Period
        btst    #0,d0
        beq.s   .chk_r7
        move.b  #6,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w

.chk_r7:
        ; Bit 1: R7 Mixer
        btst    #1,d0
        beq.s   .chk_r11
        move.b  #7,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w

.chk_r11:
        ; Bit 2: R11 Env Period LSB
        btst    #2,d0
        beq.s   .chk_r12
        move.b  #11,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w

.chk_r12:
        ; Bit 3: R12 Env Period MSB
        btst    #3,d0
        beq.s   .chk_r13
        move.b  #12,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w

.chk_r13:
        ; Bit 4: R13 Env Shape Retrigger
        btst    #4,d0
        beq.s   .glob_done
        move.b  #13,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w

.glob_done:
        move.l  a0,ptr_glob
        bra.s   .tick_a

.wait_glob_token:
        tst.b   d0
        beq.s   .set_wait_glob
        subq.b  #1,d0
.set_wait_glob:
        move.b  d0,wait_glob
        move.l  a0,ptr_glob

        ; ----------------------------------------------------
        ; Voice Tracks A, B, C
        ; ----------------------------------------------------
.tick_a:
        moveq   #0,d1                   ; R0
        moveq   #1,d2                   ; R1
        moveq   #8,d3                   ; R8
        lea     ptr_a,a1
        lea     wait_a,a2
        bsr.s   tick_voice

.tick_b:
        moveq   #2,d1                   ; R2
        moveq   #3,d2                   ; R3
        moveq   #9,d3                   ; R9
        lea     ptr_b,a1
        lea     wait_b,a2
        bsr.s   tick_voice

.tick_c:
        moveq   #4,d1                   ; R4
        moveq   #5,d2                   ; R5
        moveq   #10,d3                  ; R10
        lea     ptr_c,a1
        lea     wait_c,a2
        bsr.s   tick_voice

        rts

; ----------------------------------------------------------
; tick_voice
; in:  d1.w = Tone Low Reg (0, 2, 4)
;      d2.w = Tone High Reg (1, 3, 5)
;      d3.w = Volume Reg (8, 9, 10)
;      a1   = pointer to ptr_x
;      a2   = pointer to wait_x
; ----------------------------------------------------------
tick_voice:
        tst.b   (a2)
        beq.s   .read_voice
        subq.b  #1,(a2)
        rts

.read_voice:
        move.l  (a1),d0
        beq.s   .voice_done             ; NULL pointer (e.g. empty sentinel pattern)
        movea.l d0,a0
        move.b  (a0)+,d0                ; opcode byte

        bpl.s   .wait_voice_token

        ; Bit 6 (L): Tone Low
        btst    #6,d0
        beq.s   .chk_tone_hi
        move.b  d1,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w

.chk_tone_hi:
        ; Bit 5 (M): Tone High
        btst    #5,d0
        beq.s   .chk_vol
        move.b  d2,PSG_SELECT.w
        move.b  (a0)+,PSG_DATA.w

.chk_vol:
        ; Bit 4 (E): Envelope mode vs Volume nibble
        btst    #4,d0
        bne.s   .env_mode
        andi.b  #$0f,d0                 ; volume 0..15
        bra.s   .write_vol
.env_mode:
        moveq   #$10,d0                 ; hardware envelope bit
.write_vol:
        move.b  d3,PSG_SELECT.w
        move.b  d0,PSG_DATA.w

        move.l  a0,(a1)
        rts

.wait_voice_token:
        tst.b   d0
        beq.s   .set_wait_voice
        subq.b  #1,d0
.set_wait_voice:
        move.b  d0,(a2)
        move.l  a0,(a1)
.voice_done:
        rts

; ----------------------------------------------------------
; Player state
; ----------------------------------------------------------
        .bss

last_vbl:           ds.l 1

ptr_a:              ds.l 1
ptr_b:              ds.l 1
ptr_c:              ds.l 1
ptr_glob:           ds.l 1

wait_a:             ds.b 1
wait_b:             ds.b 1
wait_c:             ds.b 1
wait_glob:          ds.b 1

pat_frame:          ds.b 1
seq_step:           ds.b 1
pat_frames_def:     ds.b 1
seq_len_def:        ds.b 1
loop_step_def:      ds.b 1

                    .even
music_delta:        ds.w 1
music_acc:          ds.w 1

saved_palette:      ds.w 16
