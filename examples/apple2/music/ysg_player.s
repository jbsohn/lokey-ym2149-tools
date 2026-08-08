; =========================================================
; ysg_player.s -- YSG Music Player for Apple II Mockingboard
; ca65 / cc65 toolchain
; =========================================================
; Build (example):
;   ca65 ysg_player.s -o player.o
;   ca65 music.s -o music.o          ; see music.s.template
;   ld65 -C apple2.cfg player.o music.o -o song.bin
;
; Output is a raw binary loadable at $0803:
;   BLOAD song.bin
;   CALL 2051
; =========================================================

.include "mockingboard.inc"
.include "ysg.inc"

.segment "CODE"

PLAYER_ZP_BASE  = $80
CPU_HZ          = 60            ; polling rate driven by the VIA Timer 1 tick
KBD             = $C000
KBDSTRB         = $C010
DOS_WARM        = $3D0          ; DOS 3.3 warm-start (re-enter "]" prompt)
APPLE2_CLK_HZ   = 1020484
TICK_CYCLES     = (APPLE2_CLK_HZ / CPU_HZ) - 2

.ifndef PLAYER_HZ
PLAYER_HZ       = 50
.endif

music_ptr      = PLAYER_ZP_BASE + TPlayerState::music_ptr
pat_frames     = PLAYER_ZP_BASE + TPlayerState::pat_frames
seq_idx        = PLAYER_ZP_BASE + TPlayerState::seq_idx
tmp_mask       = PLAYER_ZP_BASE + TPlayerState::tmp_mask
pat_table      = PLAYER_ZP_BASE + TPlayerState::pat_table
pat_base       = PLAYER_ZP_BASE + TPlayerState::pat_base
seq_base       = PLAYER_ZP_BASE + TPlayerState::seq_base
pat_size       = PLAYER_ZP_BASE + TPlayerState::pat_size
seq_len        = PLAYER_ZP_BASE + TPlayerState::seq_len
loop_pat       = PLAYER_ZP_BASE + TPlayerState::loop_pat
last_pat_frames = PLAYER_ZP_BASE + TPlayerState::last_pat_frames
features       = PLAYER_ZP_BASE + TPlayerState::features
music_acc      = PLAYER_ZP_BASE + TPlayerState::music_acc
music_delta    = PLAYER_ZP_BASE + TPlayerState::music_delta
v_frame        = PLAYER_ZP_BASE + TPlayerState::v_frame

.import music_data

; ----------------------------------------------------------

start:
        cld

        lda #$FF
        sta AY_DDRA             ; data bus (ORA) = outputs
        sta AY_DDRB             ; control lines (ORB) = outputs
        lda #AY_RESET_HOLD
        sta AY_CTRL              ; pulse AY /RESET low
        lda #AY_MODE_INACTIVE
        sta AY_CTRL              ; release /RESET, go inactive

        lda #AY_ACR_T1_FREERUN
        sta AY_ACR
        lda #<TICK_CYCLES
        sta AY_T1LL
        sta AY_T1CL
        lda #>TICK_CYCLES
        sta AY_T1LH
        sta AY_T1CH               ; loads counter from latch and starts it

        ldx #NUM_REGS-1
cl_y:   lda #0
        jsr ay_write
        dex
        bpl cl_y

        jsr init_music

main_loop:
        jsr wait_tick

        lda music_delta
        ora music_delta+1
        beq play_now

        clc
        lda music_acc
        adc music_delta
        sta music_acc
        lda music_acc+1
        adc music_delta+1
        sta music_acc+1
        bcc skip_play

play_now:
        jsr play_frame
skip_play:

        lda KBD
        bpl main_loop
        bit KBDSTRB
        cmp #$8D                ; RETURN (works reliably on all emulators)
        bne main_loop

        ldx #NUM_REGS-1
silence:
        lda #0
        jsr ay_write
        dex
        bpl silence

        jmp DOS_WARM             ; re-enter DOS/BASIC cleanly
        
; ----------------------------------------------------------
; ay_write -- write one AY-3-8910 register via the VIA bus
;   in: X = register number, A = value
;   Drives BC1/BDIR through LATCH ADDRESS then WRITE DATA, each
;   followed by INACTIVE, per the AY-3-8910 bus protocol.
; ----------------------------------------------------------
ay_write:
        pha
        txa
        sta AY_BUS
        lda #AY_MODE_LATCH
        sta AY_CTRL
        lda #AY_MODE_INACTIVE
        sta AY_CTRL
        pla
        sta AY_BUS
        lda #AY_MODE_WRITE
        sta AY_CTRL
        lda #AY_MODE_INACTIVE
        sta AY_CTRL
        rts

; ----------------------------------------------------------
; wait_tick -- block until the VIA Timer 1 tick (CPU_HZ) fires.
; Free-running T1 keeps ticking in the background regardless of how long
; play_frame took, so this self-corrects instead of drifting the way a
; fixed-cycle-count busy-wait would once per-frame work varies.
; ----------------------------------------------------------
wait_tick:
        lda AY_IFR
        and #$40
        beq wait_tick
        lda AY_T1CL              ; reading T1C-L clears the T1 IFR flag
        rts

; ----------------------------------------------------------
; init_music -- initialize player state from YSG header
; ----------------------------------------------------------
init_music:
        lda #0
        sta seq_idx
        sta pat_frames
        sta music_acc
        sta music_acc+1
        sta v_frame
        sta v_frame+1

        lda #<((PLAYER_HZ * 65536) / CPU_HZ)
        sta music_delta
        lda #>((PLAYER_HZ * 65536) / CPU_HZ)
        sta music_delta+1

        lda music_data + TYsgHeader::pat_size
        sta pat_size

        lda music_data + TYsgHeader::seq_len
        sta seq_len

        lda music_data + TYsgHeader::loop_pat
        sta loop_pat

        lda music_data + TYsgHeader::last_pat_frames
        sta last_pat_frames

        lda music_data + TYsgHeader::features
        sta features

        lda #<(music_data + .sizeof(TYsgHeader))
        sta seq_base
        lda #>(music_data + .sizeof(TYsgHeader))
        sta seq_base+1

        clc
        lda seq_base
        adc seq_len
        sta pat_table
        lda seq_base+1
        adc #0
        sta pat_table+1

        lda music_data + TYsgHeader::num_unique
        sta tmp_mask
        lda #0
        sta tmp_mask+1
        asl tmp_mask
        rol tmp_mask+1
        asl tmp_mask
        rol tmp_mask+1
        clc
        lda pat_table
        adc tmp_mask
        sta pat_base
        lda pat_table+1
        adc tmp_mask+1
        sta pat_base+1

        rts

; ----------------------------------------------------------
; play_frame -- advance one music frame, write AY regs
; ----------------------------------------------------------
play_frame:
        lda pat_frames
        bne do_play

        lda seq_idx
        cmp seq_len
        bcc load_pattern

        lda loop_pat
        cmp #$FF
        bne do_loop
        jsr init_music
        rts

do_loop:
        sta seq_idx

load_pattern:
        ldy seq_idx
        lda (seq_base),y
        inc seq_idx

        sta tmp_mask
        lda #0
        sta tmp_mask+1
        asl tmp_mask
        rol tmp_mask+1
        asl tmp_mask
        rol tmp_mask+1
        clc
        lda tmp_mask
        adc pat_table
        sta tmp_mask
        lda tmp_mask+1
        adc pat_table+1
        sta tmp_mask+1

        ldy #0
        lda (tmp_mask),y
        sta music_ptr
        iny
        lda (tmp_mask),y
        sta music_ptr+1

        clc
        lda music_ptr
        adc pat_base
        sta music_ptr
        lda music_ptr+1
        adc pat_base+1
        sta music_ptr+1

        lda last_pat_frames
        beq use_pat_size
        lda seq_idx
        cmp seq_len
        bne use_pat_size
        lda last_pat_frames
        sta pat_frames
        jmp do_play
use_pat_size:
        lda pat_size
        sta pat_frames

do_play:
        dec pat_frames

        ldy #0
        lda (music_ptr),y
        sta tmp_mask
        iny
        lda (music_ptr),y
        sta tmp_mask+1
        clc
        lda music_ptr
        adc #2
        sta music_ptr
        lda music_ptr+1
        adc #0
        sta music_ptr+1

        lda features
        lsr
        bcc rle_done
        bit tmp_mask+1
        bpl rle_done
        ldy #0
        lda (music_ptr),y
        inc music_ptr
        bne rle_ptr_done
        inc music_ptr+1
rle_ptr_done:
        sta tmp_mask
        lda pat_frames
        sec
        sbc tmp_mask
        sta pat_frames
        rts
rle_done:
        ldx #0
reg_low:
        lda bit_table,x
        and tmp_mask
        beq next_low
        ldy #0
        lda (music_ptr),y
        jsr ay_write
        inc music_ptr
        bne next_low
        inc music_ptr+1
next_low:
        inx
        cpx #8
        bne reg_low

reg_high:
        txa
        and #$07
        tay
        lda bit_table,y
        and tmp_mask+1
        beq next_high
        ldy #0
        lda (music_ptr),y
        jsr ay_write
        inc music_ptr
        bne next_high
        inc music_ptr+1
next_high:
        inx
        cpx #NUM_REGS
        bne reg_high

        rts

bit_table:
        .byte $01, $02, $04, $08, $10, $20, $40, $80
