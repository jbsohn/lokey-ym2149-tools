# Atari ST Music + SFX Keyboard Demo Makefile
# Assembler: rmac (68000 mode), producing a .tos directly (-p)

BUILD_DIR    := build
LYM          ?= ../../../target/release/lym
RMAC         := rmac
RMAC_FLAGS   := -m68000 -p -I. -I../include

SONG_SRC     := ../../../tests/fixtures/song/enchant1.ym
SFX_Z_SRC    := ../../../tests/fixtures/sfx/pew-x.afb
SFX_X_SRC    := ../../../tests/fixtures/sfx/blip.json

.PHONY: all clean help ensure-lym

all: ensure-lym $(BUILD_DIR)/mix.tos

ensure-lym:
	@if [ ! -f "$(LYM)" ]; then \
		echo "  Building 'lym' release binary with Cargo..."; \
		(cd ../../.. && cargo build --release --bin lym); \
	fi

help:
	@echo "Atari ST Music + SFX Keyboard Demo"
	@echo ""
	@echo "Targets:"
	@echo "  make all      Build build/mix.tos (auto-builds 'lym' via cargo if missing)"
	@echo "  make clean    Remove build artifacts"
	@echo ""
	@echo "Requires rmac (https://github.com/reboot-projects/rmac) on PATH."
	@echo ""
	@echo "Variables:"
	@echo "  LYM=path      Path to lym binary (default: ../../../target/release/lym)"
	@echo ""
	@echo "Controls: Z triggers an SFX on channel B, X triggers an SFX on channel C,"
	@echo "both overlaid on top of the looping background song."

$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

# lym's target clock defaults to the Atari 7800's 1.789773MHz, so it must
# be told explicitly to keep the ST's own 2MHz native pitch instead of
# rescaling for the 7800.
$(BUILD_DIR)/song.ysg: $(SONG_SRC) | $(BUILD_DIR)
	@echo "  Rendering YSG background song..."
	@$(LYM) song render --input $< --output $@ --compression full --target-clock 2000000

$(BUILD_DIR)/sfx_z.yfx: $(SFX_Z_SRC) | $(BUILD_DIR)
	@echo "  Rendering YFX sound effect (Z key)..."
	@$(LYM) sfx render --input $< --output $@ --index 0

$(BUILD_DIR)/sfx_x.yfx: $(SFX_X_SRC) | $(BUILD_DIR)
	@echo "  Rendering YFX sound effect (X key)..."
	@$(LYM) sfx render --input $< --output $@ --index 0

# rmac assembles the whole program (including the incbin'd song/sfx data)
# and links it straight into a runnable .tos in one step.
$(BUILD_DIR)/mix.tos: main.s $(BUILD_DIR)/song.ysg $(BUILD_DIR)/sfx_z.yfx $(BUILD_DIR)/sfx_x.yfx \
		../include/st.inc ../include/ym2149.inc ysg.inc | $(BUILD_DIR)
	@echo "  Assembling and linking Atari ST program..."
	@$(RMAC) $(RMAC_FLAGS) main.s -o $@

clean:
	@rm -rf $(BUILD_DIR)
