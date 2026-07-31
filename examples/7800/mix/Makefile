# Atari 7800 Music + SFX Proof-of-Concept Sample Makefile
# Assembler: ca65 / ld65 (cc65 toolchain)

export PATH := /Users/john/7800AsmDevKit:/Users/john/cc65/bin:$(PATH)

BUILD_DIR    := build
LYM          ?= ../../../target/release/lym
A78TOOL      ?= a78tool
CA65         := ca65
LD65         := ld65
SIGN         := 7800sign

SONG_SRC     := ../../../tests/fixtures/song/ND-Loader.ym
SFX_SRC      := ../../../tests/fixtures/sfx/pew-x.yfx

.PHONY: all clean help

all: $(BUILD_DIR)/sample.a78

$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

# Render background song into .ysg payload
$(BUILD_DIR)/song.ysg: $(SONG_SRC) | $(BUILD_DIR)
	@echo "  Rendering YSG background song..."
	@$(LYM) song render --input $< --output $@ --compression full --max-bytes 28672

# Copy/render sound effect payload
$(BUILD_DIR)/pew-x.yfx: $(SFX_SRC) | $(BUILD_DIR)
	@cp $(SFX_SRC) $(BUILD_DIR)/pew-x.yfx

# Generate music data module assembly
$(BUILD_DIR)/music.asm: $(BUILD_DIR)/song.ysg
	@printf '.segment "RODATA"\n.export music_data\nmusic_data:\n    .incbin "%s"\n' "$<" > $@

# Assemble music data module
$(BUILD_DIR)/music.o: $(BUILD_DIR)/music.asm
	@$(CA65) $< -o $@

# Assemble 6502 main.asm
$(BUILD_DIR)/main.o: main.asm $(BUILD_DIR)/pew-x.yfx maria.inc ym2149.inc ysg.inc | $(BUILD_DIR)
	@echo "  Assembling 6502 main.asm..."
	@$(CA65) -I . -I $(BUILD_DIR) main.asm -o $@

# Link into raw 32KB ROM
$(BUILD_DIR)/sample.bin: $(BUILD_DIR)/main.o $(BUILD_DIR)/music.o | $(BUILD_DIR)
	@echo "  Linking 7800 ROM..."
	@$(LD65) -C 7800.cfg $^ -o $@

# Sign ROM
$(BUILD_DIR)/sample.rom: $(BUILD_DIR)/sample.bin
	@cp "$<" "$@"
	@$(SIGN) -w "$@" >/dev/null 2>&1 && $(SIGN) -t "$@" 2>/dev/null || true

# Package .a78 emulator ROM header
$(BUILD_DIR)/sample.a78: $(BUILD_DIR)/sample.rom
	@echo "  Packaging A78 header..."
	@$(A78TOOL) --input "$<" --output "$@" --title "7800 Music & SFX Fire Demo"

clean:
	@rm -rf $(BUILD_DIR)
