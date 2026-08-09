; ============================================================
; mockingboard.inc -- Apple II Mockingboard AY-3-8910 Defines
; ca65 / cc65 toolchain
; ============================================================
; Mockingboard maps one SY6522 VIA per 256-byte slot ROM page ($Cs00-
; $CsFF; NOT the 16-byte $C0sX device-select space -- that range is
; unused by real Mockingboard/Mockingboard-C cards). The AY-3-8910
; sits behind the VIA's parallel ports, NOT as two directly addressable
; ports. Register access requires driving the AY's BC1/BDIR bus-control
; lines (via VIA Port B) around each register/data write:
;   AY_BUS  = $Cs01   (VIA ORA -- 8-bit data bus: register # or value)
;   AY_CTRL = $Cs00   (VIA ORB -- bit0=BC1, bit1=BDIR; BC2 tied high)
; Default: slot 4 ($C400/$C401)
; ============================================================

.ifndef MOCK_SLOT
MOCK_SLOT = 4
.endif

MOCK_SLOT_IO = $C000 + (MOCK_SLOT * $100)

AY_CTRL     = MOCK_SLOT_IO + 0     ; VIA ORB
AY_BUS      = MOCK_SLOT_IO + 1     ; VIA ORA
AY_DDRB     = MOCK_SLOT_IO + 2
AY_DDRA     = MOCK_SLOT_IO + 3
AY_T1CL     = MOCK_SLOT_IO + 4     ; Timer 1 counter low (read clears IFR bit6)
AY_T1CH     = MOCK_SLOT_IO + 5     ; Timer 1 counter high (write loads + starts)
AY_T1LL     = MOCK_SLOT_IO + 6     ; Timer 1 latch low
AY_T1LH     = MOCK_SLOT_IO + 7     ; Timer 1 latch high
AY_ACR      = MOCK_SLOT_IO + 11    ; Auxiliary Control Register
AY_IFR      = MOCK_SLOT_IO + 13    ; Interrupt Flag Register

; AY-3-8910 bus modes (BC2 hardwired high; nAYFunc = BDIR<<2 | BC2<<1 | BC1)
; Bit 2 of ORB is also the AY's active-low /RESET line -- it must stay 1
; during normal operation or the chip resets instead of latching/writing.
AY_RESET_HOLD    = %00000000       ; /RESET asserted (chip held in reset)
AY_MODE_INACTIVE = %00000100       ; /RESET=1, BDIR=0, BC1=0
AY_MODE_WRITE    = %00000110       ; /RESET=1, BDIR=1, BC1=0
AY_MODE_LATCH    = %00000111       ; /RESET=1, BDIR=1, BC1=1

; ACR bit6=1 puts Timer 1 in free-running mode: it auto-reloads from the
; T1 latch on every underflow and keeps setting the IFR T1 flag, with no
; CPU intervention needed between ticks. Bit7=0 so PB7 isn't driven as a
; square-wave output (PB7 is part of AY_CTRL above).
AY_ACR_T1_FREERUN = %01000000

NUM_REGS    = 14
AY_MIXER    = 7
