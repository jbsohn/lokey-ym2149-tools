; =========================================================
; ysg_player.s -- YSG Music Player for Atari 7800
; ca65 / cc65 toolchain
; Target Zero Page: 14 bytes strictly enforced ($80-$8D)
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

; Zero Page variables (14 bytes total)
ptr_a       = PLAYER_ZP_BASE + TYsgPlayerState::ptr_a
ptr_b       = PLAYER_ZP_BASE + TYsgPlayerState::ptr_b
ptr_c       = PLAYER_ZP_BASE + TYsgPlayerState::ptr_c
ptr_glob    = PLAYER_ZP_BASE + TYsgPlayerState::ptr_glob
wait_a      = PLAYER_ZP_BASE + TYsgPlayerState::wait_a
wait_b      = PLAYER_ZP_BASE + TYsgPlayerState::wait_b
wait_c      = PLAYER_ZP_BASE + TYsgPlayerState::wait_c
wait_glob   = PLAYER_ZP_BASE + TYsgPlayerState::wait_glob
pat_frame   = PLAYER_ZP_BASE + TYsgPlayerState::pat_frame
seq_step    = PLAYER_ZP_BASE + TYsgPlayerState::seq_step

; Zero Page temporary pointer scratch for indirect indexed (ptr),y (4 bytes: $8E-$91)
tmp_ptr         = PLAYER_ZP_BASE + .sizeof(TYsgPlayerState)
track_desc_ptr  = tmp_ptr + 2

; Zero Page scratch and playback parameters ($92-$9D)
tmp_op_a        = track_desc_ptr + 2
tmp_op_b        = tmp_op_a + 1
tmp_op_c        = tmp_op_b + 1
tmp_op_glob     = tmp_op_c + 1
pat_frames_def  = tmp_op_glob + 1
seq_len_def     = pat_frames_def + 1
loop_step_def   = seq_len_def + 1
music_acc       = loop_step_def + 1
music_delta     = music_acc + 2
v_frame         = music_delta + 2


.segment "CODE"

.import music_data

; ----------------------------------------------------------
reset:
        sei
        cld
        ldx #$FF
        txs

        ldx #$00
        ldy #$00
p_1:    dex
        bne p_1
        dey
        bne p_1

        jsr init_music

        ; Silence all PSG registers initially
        ldx #NUM_REGS-1
cl_y:   stx AY_ADDR
        lda #0
        sta AY_DATA
        dex
        bpl cl_y

main_loop:
        jsr sync_vbi
        jsr update_visuals

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
        sta tmp_ptr
        lda v_frame+1
        asl
        asl
        asl
        and #$08
        ora tmp_ptr
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
.export init_music
init_music:
        lda #0
        sta seq_step
        sta pat_frame
        sta wait_a
        sta wait_b
        sta wait_c
        sta wait_glob
        sta music_acc
        sta music_acc+1
        sta v_frame
        sta v_frame+1

        ; Set music_delta dynamically based on header frame_rate_hz
        lda music_data + TYsgHeader::frame_rate_hz
        cmp #25
        bne @not_25
        lda #<((25 * 65536) / NTSC_HZ)
        ldx #>((25 * 65536) / NTSC_HZ)
        jmp @set_delta
@not_25:
        cmp #30
        bne @not_30
        lda #<((30 * 65536) / NTSC_HZ)
        ldx #>((30 * 65536) / NTSC_HZ)
        jmp @set_delta
@not_30:
        cmp #60
        bne @default_hz
        lda #0
        ldx #0
        jmp @set_delta
@default_hz:
        lda #<((PLAYER_HZ * 65536) / NTSC_HZ)
        ldx #>((PLAYER_HZ * 65536) / NTSC_HZ)
@set_delta:
        sta music_delta
        stx music_delta+1

        lda music_data + TYsgHeader::pattern_frames
        sta pat_frames_def

        lda music_data + TYsgHeader::seq_len
        sta seq_len_def

        lda music_data + TYsgHeader::loop_step
        sta loop_step_def

        rts

; ----------------------------------------------------------
; load_step_patterns -- resolves track pointers for current seq_step
; ----------------------------------------------------------
load_step_patterns:
        ; Track A
        lda music_data + TYsgHeader::offset_track_a
        ldx music_data + TYsgHeader::offset_track_a + 1
        jsr setup_track_a

        ; Track B
        lda music_data + TYsgHeader::offset_track_b
        ldx music_data + TYsgHeader::offset_track_b + 1
        jsr setup_track_b

        ; Track C
        lda music_data + TYsgHeader::offset_track_c
        ldx music_data + TYsgHeader::offset_track_c + 1
        jsr setup_track_c

        ; Track Global
        lda music_data + TYsgHeader::offset_track_glob
        ldx music_data + TYsgHeader::offset_track_glob + 1
        jsr setup_track_glob

        rts

; Helper: sets up track descriptor base address in track_desc_ptr
; In: A = offset LO, X = offset HI
setup_track_desc:
        clc
        adc #<music_data
        sta track_desc_ptr
        txa
        adc #>music_data
        sta track_desc_ptr+1
        rts

setup_track_a:
        jsr setup_track_desc
        ; Read pattern index for this step from sequence_table (offset 1 + seq_step)
        ldy seq_step
        iny
        lda (track_desc_ptr),y
        cmp #$FF
        bne :+
        ; Sentinel empty pattern: silence voice A and wait full pattern
        ldx #8
        stx AY_ADDR
        lda #0
        sta AY_DATA
        lda pat_frames_def
        sta wait_a
        rts
:
        ; Offset table starts at track_desc_ptr + 1 + seq_len
        jsr resolve_pattern_offset
        sta ptr_a
        stx ptr_a+1
        lda #0
        sta wait_a
        rts

setup_track_b:
        jsr setup_track_desc
        ldy seq_step
        iny
        lda (track_desc_ptr),y
        cmp #$FF
        bne :+
        ldx #9
        stx AY_ADDR
        lda #0
        sta AY_DATA
        lda pat_frames_def
        sta wait_b
        rts
:
        jsr resolve_pattern_offset
        sta ptr_b
        stx ptr_b+1
        lda #0
        sta wait_b
        rts

setup_track_c:
        jsr setup_track_desc
        ldy seq_step
        iny
        lda (track_desc_ptr),y
        cmp #$FF
        bne :+
        ldx #10
        stx AY_ADDR
        lda #0
        sta AY_DATA
        lda pat_frames_def
        sta wait_c
        rts
:
        jsr resolve_pattern_offset
        sta ptr_c
        stx ptr_c+1
        lda #0
        sta wait_c
        rts

setup_track_glob:
        jsr setup_track_desc
        ldy seq_step
        iny
        lda (track_desc_ptr),y
        jsr resolve_pattern_offset
        sta ptr_glob
        stx ptr_glob+1
        lda #0
        sta wait_glob
        rts

; In: A = pattern_idx (0..P-1)
; Out: A = payload ptr LO, X = payload ptr HI
resolve_pattern_offset:
        ; Table offset = 1 + seq_len + pat_idx * 2
        asl
        sta tmp_ptr
        lda #0
        rol
        sta tmp_ptr+1

        ; SEC sets carry (+1) to account for 1-byte unique_patterns count at track start
        sec
        lda tmp_ptr
        adc seq_len_def
        sta tmp_ptr
        lda tmp_ptr+1
        adc #0
        sta tmp_ptr+1

        clc
        lda tmp_ptr
        adc track_desc_ptr
        sta tmp_ptr
        lda tmp_ptr+1
        adc track_desc_ptr+1
        sta tmp_ptr+1

        ldy #0
        lda (tmp_ptr),y
        tax
        iny
        lda (tmp_ptr),y
        tay ; Y = offset HI, X = offset LO

        ; Address = track_desc_ptr + offset
        clc
        txa
        adc track_desc_ptr
        pha
        tya
        adc track_desc_ptr+1
        tax
        pla
        rts

; ----------------------------------------------------------
; play_frame -- advance one frame and dispatch opcodes
; ----------------------------------------------------------
.export play_frame
play_frame:
        lda pat_frame
        bne @no_advance

        ; Advance to next pattern in sequence
        lda seq_step
        cmp seq_len_def
        bcc :+
        lda loop_step_def
        cmp #$FF
        bne @loop
        lda #0
@loop:  sta seq_step
:
        jsr load_step_patterns
        inc seq_step
        lda pat_frames_def
        sta pat_frame

@no_advance:
        dec pat_frame

        ; === Track Global (dispatched FIRST to establish R7 Mixer & R6 Noise) ===
@tick_glob:
        lda wait_glob
        beq @read_glob
        dec wait_glob
        jmp @tick_a
@read_glob:
        ldy #0
        lda (ptr_glob),y
        inc ptr_glob
        bne :+
        inc ptr_glob+1
:
        ; INC clobbers N/Z. Refresh them from the opcode: bit 7 clear = wait.
        ora #0
        bpl @wait_token_glob
        sta tmp_op_glob

        ; Bit 0: R6 (Noise Period)
        lsr
        bcc @chk_r7
        lda (ptr_glob),y
        inc ptr_glob
        bne :+
        inc ptr_glob+1
:
        ldx #6
        stx AY_ADDR
        sta AY_DATA

@chk_r7:
        ; Bit 1: R7 (Mixer)
        lda tmp_op_glob
        and #$02
        beq @chk_r11
        lda (ptr_glob),y
        inc ptr_glob
        bne :+
        inc ptr_glob+1
:
        ldx #7
        stx AY_ADDR
        sta AY_DATA

@chk_r11:
        ; Bit 2: R11 (Envelope Period LSB)
        lda tmp_op_glob
        and #$04
        beq @chk_r12
        lda (ptr_glob),y
        inc ptr_glob
        bne :+
        inc ptr_glob+1
:
        ldx #11
        stx AY_ADDR
        sta AY_DATA

@chk_r12:
        ; Bit 3: R12 (Envelope Period MSB)
        lda tmp_op_glob
        and #$08
        beq @chk_r13
        lda (ptr_glob),y
        inc ptr_glob
        bne :+
        inc ptr_glob+1
:
        ldx #12
        stx AY_ADDR
        sta AY_DATA

@chk_r13:
        ; Bit 4: R13 (Envelope Shape Retrigger)
        lda tmp_op_glob
        and #$10
        beq @glob_done
        lda (ptr_glob),y
        inc ptr_glob
        bne :+
        inc ptr_glob+1
:
        ldx #13
        stx AY_ADDR
        sta AY_DATA

@glob_done:
        jmp @tick_a

@wait_token_glob:
        beq :+
        sec
        sbc #1
:       sta wait_glob


@tick_a:
        ; === Track A ===
        lda wait_a
        beq @read_a
        dec wait_a
        jmp @tick_b
@read_a:
        ldy #0
        lda (ptr_a),y
        inc ptr_a
        bne :+
        inc ptr_a+1
:
        ; INC clobbers N/Z. Refresh them from the opcode: bit 7 clear = wait.
        ora #0
        bpl @wait_token_a
        sta tmp_op_a

        ; Bit 6: Period LSB
        and #$40
        beq @chk_msb_a
        lda (ptr_a),y
        inc ptr_a
        bne :+
        inc ptr_a+1
:
        ldx #0
        stx AY_ADDR
        sta AY_DATA

@chk_msb_a:
        ; Bit 5: Period MSB
        lda tmp_op_a
        and #$20
        beq @set_vol_a
        lda (ptr_a),y
        inc ptr_a
        bne :+
        inc ptr_a+1
:
        ldx #1
        stx AY_ADDR
        sta AY_DATA

@set_vol_a:
        ; Bit 4: Hardware Envelope Mode
        lda tmp_op_a
        and #$10
        bne @env_a
        lda tmp_op_a
        and #$0F
        jmp @write_vol_a
@env_a:
        lda #$10
@write_vol_a:
        ldx #8
        stx AY_ADDR
        sta AY_DATA
        jmp @tick_b

@wait_token_a:
        beq :+
        sec
        sbc #1
:       sta wait_a


@tick_b:
        ; === Track B ===
        lda wait_b
        beq @read_b
        dec wait_b
        jmp @tick_c
@read_b:
        ldy #0
        lda (ptr_b),y
        inc ptr_b
        bne :+
        inc ptr_b+1
:
        ; INC clobbers N/Z. Refresh them from the opcode: bit 7 clear = wait.
        ora #0
        bpl @wait_token_b
        sta tmp_op_b

        ; Bit 6: Period LSB
        and #$40
        beq @chk_msb_b
        lda (ptr_b),y
        inc ptr_b
        bne :+
        inc ptr_b+1
:
        ldx #2
        stx AY_ADDR
        sta AY_DATA

@chk_msb_b:
        ; Bit 5: Period MSB
        lda tmp_op_b
        and #$20
        beq @set_vol_b
        lda (ptr_b),y
        inc ptr_b
        bne :+
        inc ptr_b+1
:
        ldx #3
        stx AY_ADDR
        sta AY_DATA

@set_vol_b:
        ; Bit 4: Hardware Envelope Mode
        lda tmp_op_b
        and #$10
        bne @env_b
        lda tmp_op_b
        and #$0F
        jmp @write_vol_b
@env_b:
        lda #$10
@write_vol_b:
        ldx #9
        stx AY_ADDR
        sta AY_DATA
        jmp @tick_c

@wait_token_b:
        beq :+
        sec
        sbc #1
:       sta wait_b


@tick_c:
        ; === Track C ===
        lda wait_c
        beq @read_c
        dec wait_c
        rts
@read_c:
        ldy #0
        lda (ptr_c),y
        inc ptr_c
        bne :+
        inc ptr_c+1
:
        ; INC clobbers N/Z. Refresh them from the opcode: bit 7 clear = wait.
        ora #0
        bpl @wait_token_c
        sta tmp_op_c

        ; Bit 6: Period LSB
        and #$40
        beq @chk_msb_c
        lda (ptr_c),y
        inc ptr_c
        bne :+
        inc ptr_c+1
:
        ldx #4
        stx AY_ADDR
        sta AY_DATA

@chk_msb_c:
        ; Bit 5: Period MSB
        lda tmp_op_c
        and #$20
        beq @set_vol_c
        lda (ptr_c),y
        inc ptr_c
        bne :+
        inc ptr_c+1
:
        ldx #5
        stx AY_ADDR
        sta AY_DATA

@set_vol_c:
        ; Bit 4: Hardware Envelope Mode
        lda tmp_op_c
        and #$10
        bne @env_c
        lda tmp_op_c
        and #$0F
        jmp @write_vol_c
@env_c:
        lda #$10
@write_vol_c:
        ldx #10
        stx AY_ADDR
        sta AY_DATA
        rts

@wait_token_c:
        beq :+
        sec
        sbc #1
:       sta wait_c
        rts


; ----------------------------------------------------------
; Vectors
; ----------------------------------------------------------
.segment "VECTORS"
        .byte $FF, $83
        .word reset
        .word reset
        .word reset

