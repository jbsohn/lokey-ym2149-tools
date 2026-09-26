# Atari 7800 Dual-Chip (YM-2149 + TIA) Hybrid Audio Sample Makefile
# Assembler: ca65 / ld65 (cc65 toolchain)

BUILD_DIR    := build
LYM          ?= ../../../target/release/lym
A78TOOL      := a78tool
CA65         := ca65
LD65         := ld65
SIGN         := 7800sign

SONG_SRC     := ../../../tests/fixtures/song/ND-Loader.ym
LASER_SRC    := ../../../tests/fixtures/sfx/star_raiders_laser.json
BOOM_SRC     := ../../../tests/fixtures/sfx/tia_explosion.json
JUMP_SRC     := ../../../tests/fixtures/sfx/tia_jump.json
COIN_SRC     := ../../../tests/fixtures/sfx/tia_coin.json

.PHONY: all clean help ensure-lym

all: ensure-lym $(BUILD_DIR)/dual_chip_mix.bin $(BUILD_DIR)/dual_chip_mix.a78

ensure-lym:
	@if [ ! -f "$(LYM)" ]; then \
		echo "  Building 'lym' release binary with Cargo..."; \
		(cd ../../.. && cargo build --release --bin lym); \
	fi

$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

# --- Render YM-2149 Background Music (.ysg) ---
$(BUILD_DIR)/song.ysg: $(SONG_SRC) | $(BUILD_DIR)
	@echo "  Rendering YM2149 background music (.ysg)..."
	@$(LYM) song render --input $< --output $@ --compression full --max-bytes 28672

# --- Render TIA Sound Effects (.tfx & .tfi headers) ---
$(BUILD_DIR)/star_raiders_laser.tfx $(BUILD_DIR)/star_raiders_laser.tfi: $(LASER_SRC) | $(BUILD_DIR)
	@echo "  Rendering TIA Star Raiders Laser (.tfx)..."
	@$(LYM) tia sfx render --input $< --output $(BUILD_DIR)/star_raiders_laser.tfx

$(BUILD_DIR)/tia_explosion.tfx $(BUILD_DIR)/tia_explosion.tfi: $(BOOM_SRC) | $(BUILD_DIR)
	@echo "  Rendering TIA Explosion (.tfx)..."
	@$(LYM) tia sfx render --input $< --output $(BUILD_DIR)/tia_explosion.tfx

$(BUILD_DIR)/tia_jump.tfx $(BUILD_DIR)/tia_jump.tfi: $(JUMP_SRC) | $(BUILD_DIR)
	@echo "  Rendering TIA Jump (.tfx)..."
	@$(LYM) tia sfx render --input $< --output $(BUILD_DIR)/tia_jump.tfx

$(BUILD_DIR)/tia_coin.tfx $(BUILD_DIR)/tia_coin.tfi: $(COIN_SRC) | $(BUILD_DIR)
	@echo "  Rendering TIA Coin (.tfx)..."
	@$(LYM) tia sfx render --input $< --output $(BUILD_DIR)/tia_coin.tfx

# --- Generate and assemble combined audio binary modules ---
$(BUILD_DIR)/audio_data.s: $(BUILD_DIR)/song.ysg $(BUILD_DIR)/star_raiders_laser.tfx $(BUILD_DIR)/tia_explosion.tfx $(BUILD_DIR)/tia_jump.tfx $(BUILD_DIR)/tia_coin.tfx
	@printf '.segment "RODATA"\n' > $@
	@printf '.export music_data, laser_data, explosion_data, jump_data, coin_data\n' >> $@
	@printf 'music_data:\n    .incbin "%s"\n' "$(BUILD_DIR)/song.ysg" >> $@
	@printf 'laser_data:\n    .incbin "%s"\n' "$(BUILD_DIR)/star_raiders_laser.tfx" >> $@
	@printf 'explosion_data:\n    .incbin "%s"\n' "$(BUILD_DIR)/tia_explosion.tfx" >> $@
	@printf 'jump_data:\n    .incbin "%s"\n' "$(BUILD_DIR)/tia_jump.tfx" >> $@
	@printf 'coin_data:\n    .incbin "%s"\n' "$(BUILD_DIR)/tia_coin.tfx" >> $@

$(BUILD_DIR)/audio_data.o: $(BUILD_DIR)/audio_data.s
	@$(CA65) $< -o $@

# Assemble 6502 main driver
$(BUILD_DIR)/main.o: main.s $(BUILD_DIR)/star_raiders_laser.tfi $(BUILD_DIR)/tia_explosion.tfi $(BUILD_DIR)/tia_jump.tfi $(BUILD_DIR)/tia_coin.tfi ../include/maria.inc ../include/ym2149.inc ysg.inc tfx.inc | $(BUILD_DIR)
	@echo "  Assembling 6502 dual-chip main.s..."
	@$(CA65) -I . -I ../include -I $(BUILD_DIR) main.s -o $@

# Link into raw 32KB Atari 7800 ROM
$(BUILD_DIR)/dual_chip_mix.bin: $(BUILD_DIR)/main.o $(BUILD_DIR)/audio_data.o | $(BUILD_DIR)
	@echo "  Linking 7800 Dual-Chip ROM..."
	@$(LD65) -C 7800.cfg $^ -o $@

# Sign ROM
$(BUILD_DIR)/dual_chip_mix.rom: $(BUILD_DIR)/dual_chip_mix.bin
	@cp "$<" "$@"
	@$(SIGN) -w "$@" >/dev/null 2>&1 && $(SIGN) -t "$@" 2>/dev/null || true

# Package .a78 emulator ROM header
$(BUILD_DIR)/dual_chip_mix.a78: $(BUILD_DIR)/dual_chip_mix.rom
	@if command -v "$(A78TOOL)" >/dev/null 2>&1; then \
		echo "  Packaging A78 header via a78tool..."; \
		$(A78TOOL) --input "$<" --output "$@" --title "7800 Dual-Chip Audio Demo" --ym2149 2>/dev/null || true; \
	fi

clean:
	@rm -rf $(BUILD_DIR)

help:
	@echo "Atari 7800 Dual-Chip (YM-2149 + TIA) Audio Demo"
	@echo ""
	@echo "Targets:"
	@echo "  make all       Build dual_chip_mix.bin, .rom, and .a78"
	@echo "  make clean     Remove all build artifacts"
