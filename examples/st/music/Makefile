# Atari ST YSG Music Player Sample Makefile
# Assembler: rmac (68000 mode), producing .tos executable files directly (-p)
# Usage: make -k all    (build all songs, skip failures)

BUILD_DIR    := build
YM_DIR       ?= ../../../tests/fixtures/song
LYM          ?= ../../../target/release/lym
RMAC         := rmac
RMAC_FLAGS   := -m68000 -p -I. -I../include

YM_SOURCES  := $(wildcard $(YM_DIR)/*.ym) $(wildcard $(YM_DIR)/*.YM)
YSG_FILES   := $(foreach f,$(YM_SOURCES),$(BUILD_DIR)/$(notdir $(basename $(f))).ysg)
TOS_FILES   := $(YSG_FILES:.ysg=.tos)

.PHONY: all ysg clean help ensure-lym
.DELETE_ON_ERROR:
.PRECIOUS: $(BUILD_DIR)/%.music.s $(BUILD_DIR)/%.ysg $(BUILD_DIR)/%.tos

all: ensure-lym $(TOS_FILES)

ensure-lym:
	@if [ ! -f "$(LYM)" ]; then \
		echo "  Building 'lym' release binary with Cargo..."; \
		(cd ../../.. && cargo build --release --bin lym); \
	fi

ysg: $(YSG_FILES)

help:
	@echo "Atari ST YSG Music Player Sample"
	@echo ""
	@echo "Targets:"
	@echo "  make -k all           Build .tos files for all .ym files, skip failures"
	@echo "  make ysg              Convert .ym files to .ysg only"
	@echo "  make build/NAME.tos   Build a single song .tos"
	@echo "  make clean            Remove all build artifacts"
	@echo ""
	@echo "Variables:"
	@echo "  YM_DIR=path           Source directory for .ym files (default: ../../../tests/fixtures/song)"
	@echo "  LYM=path              Path to lym binary (default: ../../../target/release/lym)"
	@echo "  RMAC=path             Path to rmac binary (default: rmac)"

$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

# --- Convert .ym to .ysg ---
# Target clock is explicit 2,000,000 Hz for Atari ST native YM-2149 pitch
$(BUILD_DIR)/%.ysg: $(YM_DIR)/%.ym | $(BUILD_DIR)
	@echo "  Rendering YSG: $*"
	@$(LYM) song render --input $< --output $@ --compression full --target-clock 2000000 \
		|| { rm -f $@; echo "  SKIP $*: unsupported format"; touch $@; }

$(BUILD_DIR)/%.ysg: $(YM_DIR)/%.YM | $(BUILD_DIR)
	@echo "  Rendering YSG: $*"
	@$(LYM) song render --input $< --output $@ --compression full --target-clock 2000000 \
		|| { rm -f $@; echo "  SKIP $*: unsupported format"; touch $@; }

# --- Generate per-song assembly wrapper ---
# ysg_player.s (containing entry point start:) MUST come first in .text so GEMDOS
# executes code instead of raw YSG header bytes at offset text+0x00.
$(BUILD_DIR)/%.music.s: $(BUILD_DIR)/%.ysg ysg_player.s ysg.inc ../include/st.inc ../include/ym2149.inc | $(BUILD_DIR)
	@printf '.include "ysg_player.s"\n\n.text\n.globl music_data\nmusic_data:\n    .incbin "%s"\n    .even\n' \
		"$(BUILD_DIR)/$*.ysg" > $@

# --- Assemble per-song wrapper into runnable Atari ST .tos executable ---
$(BUILD_DIR)/%.tos: $(BUILD_DIR)/%.music.s
	@if [ -s "$(BUILD_DIR)/$*.ysg" ]; then \
		echo "  Assembling TOS: $*"; \
		$(RMAC) $(RMAC_FLAGS) $< -o $@ \
			|| { rm -f $@; echo "  SKIP $*: assembly failed"; touch $@; }; \
	else \
		touch $@; \
	fi

clean:
	@rm -rf $(BUILD_DIR)
