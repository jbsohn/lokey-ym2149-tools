; ============================================================
; ym2149.inc -- YM2149 PSG Defines for Atari ST
; rmac (68000 mode)
; ============================================================

        .if !(^^defined YM2149_INC)
YM2149_INC equ 1

; --- Hardware Registers ---
PSG_SELECT  equ $ffff8800   ; write: select register 0-15; read: data from selected reg
PSG_DATA    equ $ffff8802   ; write: data into the selected register

; --- Constants ---
NUM_REGS    equ 14
AY_MIXER    equ 7

        .endif
