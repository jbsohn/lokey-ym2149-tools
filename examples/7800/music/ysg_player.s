; =========================================================
; ysg_player.s -- YSG Music Player for Atari 7800
; ca65 / cc65 toolchain
; =========================================================
; Build (example):
;   ca65 ysg_player.s -o player.o
;   ca65 music.s -o music.o          ; see music.s.template
;   ld65 -C 7800.cfg player.o music.o -o game.bin
; =========================================================

.include "maria.inc"
.include "ym2149.inc"
.include "ysg.inc"

.segment "CODE"

PLAYER_ZP_BASE  = $80
NTSC_HZ         = 60

; Music frame rate — game code can override with: ca65 -D PLAYER_HZ=60
.ifndef PLAYER_HZ
PLAYER_HZ       = 50
.endif

; Zero-page variable aliases derived from TPlayerState struct offsets
music_ptr   = PLAYER_ZP_BASE + TPlayerState::music_ptr
pat_frames  = PLAYER_ZP_BASE + TPlayerState::pat_frames
seq_idx     = PLAYER_ZP_BASE + TPlayerState::seq_idx
tmp_mask    = PLAYER_ZP_BASE + TPlayerState::tmp_mask
pat_table   = PLAYER_ZP_BASE + TPlayerState::pat_table
pat_base    = PLAYER_ZP_BASE + TPlayerState::pat_base
seq_base    = PLAYER_ZP_BASE + TPlayerState::seq_base
pat_size    = PLAYER_ZP_BASE + TPlayerState::pat_size
seq_len     = PLAYER_ZP_BASE + TPlayerState::seq_len
loop_pat    = PLAYER_ZP_BASE + TPlayerState::loop_pat
last_pat_frames = PLAYER_ZP_BASE + TPlayerState::last_pat_frames
features    = PLAYER_ZP_BASE + TPlayerState::features
music_acc   = PLAYER_ZP_BASE + TPlayerState::music_acc
music_delta = PLAYER_ZP_BASE + TPlayerState::music_delta
v_frame     = PLAYER_ZP_BASE + TPlayerState::v_frame

.import music_data

; ----------------------------------------------------------

reset:
        sei
        cld
        ldx #$ff
        txs

        ldx #$00
        ldy #$00
p_1:    dex
        bne p_1
        dey
        bne p_1

        jsr init_music

        ldx #NUM_REGS-1
cl_y:   stx AY_ADDR
        lda #0
        sta AY_DATA
        dex
        bpl cl_y

main_loop:
        jsr sync_vbi
        jsr update_visuals

        ; Rate conversion: add delta to accumulator each display frame.
        ; delta == 0 means music rate matches display rate — play unconditionally.
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
        jmp main_loop

; ----------------------------------------------------------
sync_vbi:
vbi1:   bit MSTAT
        bmi vbi1
vbi2:   bit MSTAT
        bpl vbi2
        inc v_frame
        bne vbi_done
        inc v_frame+1
vbi_done:
        rts

; ----------------------------------------------------------
update_visuals:
        lda v_frame
        lsr
        lsr
        lsr
        lsr
        lsr
        and #$07
        sta tmp_mask
        lda v_frame+1
        asl
        asl
        asl
        and #$08
        ora tmp_mask
        and #$0F
        asl
        asl
        asl
        asl
        ora #$08
        sta BKGRND
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

        ; music_delta = (PLAYER_HZ * 65536) / NTSC_HZ, evaluated at assemble time.
        ; When PLAYER_HZ == NTSC_HZ this is 0, the sentinel for "play every frame".
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

        ; seq_base = &music_data[sizeof(TYsgHeader)]
        lda #<(music_data + .sizeof(TYsgHeader))
        sta seq_base
        lda #>(music_data + .sizeof(TYsgHeader))
        sta seq_base+1

        ; pat_table = seq_base + seq_len
        clc
        lda seq_base
        adc seq_len
        sta pat_table
        lda seq_base+1
        adc #0
        sta pat_table+1

        ; pat_base = pat_table + num_unique * 4  (4-byte offset entries)
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

        ; Advance to next pattern in sequence
        lda seq_idx
        cmp seq_len
        bcc load_pattern

        ; Sequence exhausted — loop or restart
        lda loop_pat
        cmp #$FF
        bne do_loop
        jsr init_music          ; no loop point: restart from beginning
        rts

do_loop:
        sta seq_idx             ; jump seq_idx to loop point

load_pattern:
        ; Read pattern index from sequence table
        ldy seq_idx
        lda (seq_base),y
        inc seq_idx

        ; Compute pointer to 4-byte offset entry: tmp_mask = pat_table + idx*4
        ; idx*4 can exceed 8 bits, so use 16-bit arithmetic throughout.
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

        ; Read low 16 bits of the 32-bit offset (high 2 bytes unused on 7800)
        ldy #0
        lda (tmp_mask),y
        sta music_ptr
        iny
        lda (tmp_mask),y
        sta music_ptr+1

        ; Resolve to absolute ROM address: music_ptr += pat_base
        clc
        lda music_ptr
        adc pat_base
        sta music_ptr
        lda music_ptr+1
        adc pat_base+1
        sta music_ptr+1

        ; Use last_pat_frames for the final sequence entry, pat_size otherwise.
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

        ; Read 16-bit delta mask for this frame, advance pointer past it
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

        ; Check for RLE token: features bit 0 enables RLE, mask bit 15 (0x8000) is the flag.
        lda features
        lsr                     ; bit 0 -> carry
        bcc rle_done            ; RLE disabled in this file
        bit tmp_mask+1          ; N flag <- bit 7 of tmp_mask+1 (the RLE flag)
        bpl rle_done            ; bit 7 clear -> normal frame
        ; RLE token: read count byte N, subtract from pat_frames (N additional idle frames)
        ldy #0
        lda (music_ptr),y
        inc music_ptr
        bne rle_ptr_done
        inc music_ptr+1
rle_ptr_done:
        sta tmp_mask            ; borrow tmp_mask lo as scratch for N
        lda pat_frames
        sec
        sbc tmp_mask
        sta pat_frames
        rts
rle_done:
        ; Write changed registers R0-R7 (low byte of mask)
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

        ; Write changed registers R8-R13 (high byte of mask)
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

; ----------------------------------------------------------
; Vectors
; ----------------------------------------------------------
.segment "VECTORS"
        .byte $FF, $83
        .word reset
        .word reset
        .word reset
