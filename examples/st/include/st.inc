; ============================================================
; st.inc -- Atari ST System & Keyboard Defines
; rmac (68000 mode)
; ============================================================

        .if !(^^defined ST_INC)
ST_INC  equ 1

; --- TOS system variables (supervisor-only low memory) ---
VBLCLOCK      equ $462      ; longword, incremented once per vertical blank
CONTERM       equ $484      ; byte, console attribute bits

; --- Shifter palette registers ---
BORDER_COL    equ $ffff8240
PALETTE_BASE  equ $ffff8240

; --- MFP 68901 interrupt controller ---
MFP_IERB      equ $fffffa09
MFP_IMRB      equ $fffffa15

; --- Keyboard ACIA (6850), IKBD raw scancode stream ---
KBD_ACIA_CTRL equ $fffffc00 ; bit0 = RDRF, bit1 = TDRE
KBD_ACIA_DATA equ $fffffc02 ; read/write scancode byte / command

; Standard IKBD scancodes
SC_Z          equ $2c
SC_X          equ $2d
SC_Q          equ $10

        .endif
