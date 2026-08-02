; =========================================================
; main.s -- Atari 7800 Background Music + SFX Fire Demo
; ca65 / cc65 toolchain
; =========================================================

.include "maria.inc"
.include "ym2149.inc"
.include "ysg.inc"

.segment "CODE"

PLAYER_ZP_BASE  = $80
NTSC_HZ         = 60

.ifndef PLAYER_HZ
PLAYER_HZ       = 50
.endif

; Zero-page variable aliases derived from TPlayerState struct offsets
music_ptr       = PLAYER_ZP_BASE + TPlayerState::music_ptr
pat_frames      = PLAYER_ZP_BASE + TPlayerState::pat_frames
seq_idx         = PLAYER_ZP_BASE + TPlayerState::seq_idx
tmp_mask        = PLAYER_ZP_BASE + TPlayerState::tmp_mask
pat_table       = PLAYER_ZP_BASE + TPlayerState::pat_table
pat_base        = PLAYER_ZP_BASE + TPlayerState::pat_base
seq_base        = PLAYER_ZP_BASE + TPlayerState::seq_base
pat_size        = PLAYER_ZP_BASE + TPlayerState::pat_size
seq_len         = PLAYER_ZP_BASE + TPlayerState::seq_len
loop_pat        = PLAYER_ZP_BASE + TPlayerState::loop_pat
last_pat_frames = PLAYER_ZP_BASE + TPlayerState::last_pat_frames
features        = PLAYER_ZP_BASE + TPlayerState::features
music_acc       = PLAYER_ZP_BASE + TPlayerState::music_acc
music_delta     = PLAYER_ZP_BASE + TPlayerState::music_delta
v_frame         = PLAYER_ZP_BASE + TPlayerState::v_frame

; Zero-page for SFX
SFX_PTR_L       = $A0
SFX_PTR_H       = $A1
SFX_ACTIVE      = $A2
SFX_DELAY       = $A3
PREV_FIRE       = $A4

.import music_data

reset:
        sei
        cld
        ldx #$FF
        txs

        ; Lock INPTCTRL to 7800 mode and clear TIA VBLANK dump bit
        lda #$07
        sta PTCTRL
        lda #$00
        sta PTCTRL

        ; Clear ZP & SFX vars
        lda #0
        sta SFX_PTR_L
        sta SFX_PTR_H
        sta SFX_ACTIVE
        sta SFX_DELAY
        sta PREV_FIRE

        ; Silence YM2149 registers
        ldx #NUM_REGS-1
cl_ym:  stx AY_ADDR
        lda #0
        sta AY_DATA
        dex
        bpl cl_ym

        ; Initialize background music player
        jsr init_music

main_loop:
        jsr sync_vbi
        jsr check_fire_button

        ; Rate conversion: add delta to accumulator each display frame.
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
        ; Overlay active SFX over Channel C registers
        jsr update_sfx_overlay

        ; Update background color (flashes bright yellow $3A when SFX is active)
        lda SFX_ACTIVE
        beq idle_color
        lda #$3A
        sta BKGRND
        jmp main_loop

idle_color:
        lda #$06
        sta BKGRND
        jmp main_loop

; ----------------------------------------------------------
; VBI Sync
sync_vbi:
v1:     bit MSTAT
        bmi v1
v2:     bit MSTAT
        bpl v2
        rts

; ----------------------------------------------------------
; check_fire_button -- trigger SFX when fire button pressed
; ----------------------------------------------------------
check_fire_button:
        bit INPT4               ; Bit 7 = 0 when pressed, 1 when released
        bmi fire_not_pressed

        lda PREV_FIRE
        bne fire_done

        lda #1
        sta PREV_FIRE
        jsr trigger_sfx
        rts

fire_not_pressed:
        lda #0
        sta PREV_FIRE
fire_done:
        rts

; ----------------------------------------------------------
; trigger_sfx -- point sfx_ptr to sfx_data
; ----------------------------------------------------------
trigger_sfx:
        lda #<sfx_data
        sta SFX_PTR_L
        lda #>sfx_data
        sta SFX_PTR_H
        lda #1
        sta SFX_ACTIVE
        sta SFX_DELAY
        rts

; ----------------------------------------------------------
; update_sfx_overlay -- overlay SFX on Channel C registers
; ----------------------------------------------------------
update_sfx_overlay:
        lda SFX_ACTIVE
        beq sfx_done

        dec SFX_DELAY
        bne sfx_done

        ldy #4
        lda (SFX_PTR_L),y       ; Duration byte
        beq stop_sfx            ; Duration 0 -> End of SFX

        sta SFX_DELAY

        ; Write Channel C Tone Low (R4)
        lda #4
        sta AY_ADDR
        ldy #0
        lda (SFX_PTR_L),y
        sta AY_DATA

        ; Write Channel C Tone High (R5)
        lda #5
        sta AY_ADDR
        ldy #1
        lda (SFX_PTR_L),y
        sta AY_DATA

        ; Write Channel C Volume (R10)
        lda #10
        sta AY_ADDR
        ldy #2
        lda (SFX_PTR_L),y
        sta AY_DATA

        ; Advance SFX pointer by 5 bytes
        clc
        lda SFX_PTR_L
        adc #5
        sta SFX_PTR_L
        lda SFX_PTR_H
        adc #0
        sta SFX_PTR_H
        rts

stop_sfx:
        lda #0
        sta SFX_ACTIVE
        lda #10
        sta AY_ADDR
        lda #0
        sta AY_DATA             ; Mute Channel C volume
sfx_done:
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

        lda #<((PLAYER_HZ * 65536) / NTSC_HZ)
        sta music_delta
        lda #>((PLAYER_HZ * 65536) / NTSC_HZ)
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
        rol tmp_mask+1          ; tmp_mask (16-bit) = num_unique * 4
        clc
        lda pat_table
        adc tmp_mask
        sta pat_base
        lda pat_table+1
        adc tmp_mask+1
        sta pat_base+1

        rts

; ----------------------------------------------------------
; play_frame -- advance one music frame, write YM2149 regs
; ----------------------------------------------------------
play_frame:
        lda pat_frames
        bne do_play

        lda seq_idx
        cmp seq_len
        bcc load_pattern

        ; Sequence exhausted — loop or restart
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
        rol tmp_mask+1          ; tmp_mask (16-bit) = idx * 4
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
        stx AY_ADDR
        ldy #0
        lda (music_ptr),y
        sta AY_DATA
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
        stx AY_ADDR
        ldy #0
        lda (music_ptr),y
        sta AY_DATA
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

.segment "RODATA"
.export sfx_data
sfx_data:
    .incbin "build/pew-x.yfx"
    .byte 0,0,0,0,0             ; End-of-SFX sentinel frame (0 duration)

.segment "VECTORS"
        .byte $FF, $83          ; Atari 7800 Encryption Header Signature Flag
        .word reset             ; NMI vector
        .word reset             ; RESET vector
        .word reset             ; IRQ vector
