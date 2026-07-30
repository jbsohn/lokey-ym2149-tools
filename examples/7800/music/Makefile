# Atari 7800 YSG Player Sample Makefile
# Assembler: ca65 / ld65 (cc65 toolchain)
# Usage: make -k all    (build all songs, skip failures)

BUILD_DIR    := build
YM_DIR       ?= ../../../tests/fixtures/song
LYM          ?= ../../../target/release/lym
A78TOOL      ?= a78tool
CA65         := ca65
LD65         := ld65
CA65_FLAGS   := -I.
LD65_FLAGS   := -C 7800.cfg
SIGN         := 7800sign
YSG_MAX_BYTES ?= 28672

YM_SOURCES  := $(wildcard $(YM_DIR)/*.ym) $(wildcard $(YM_DIR)/*.YM)
YSG_FILES   := $(foreach f,$(YM_SOURCES),$(BUILD_DIR)/$(notdir $(basename $(f))).ysg)
ROM_FILES   := $(YSG_FILES:.ysg=.rom)
A78_FILES   := $(YSG_FILES:.ysg=.a78)

.PHONY: all ysg clean help
.DELETE_ON_ERROR:
.PRECIOUS: $(BUILD_DIR)/player.o $(BUILD_DIR)/%.music.o $(BUILD_DIR)/%.bin $(BUILD_DIR)/%.rom

all: $(A78_FILES)

ysg: $(YSG_FILES)

help:
	@echo "Atari 7800 YSG Player Sample"
	@echo ""
	@echo "Targets:"
	@echo "  make -k all           Build signed .a78 files for all .ym files, skip failures"
	@echo "  make ysg              Convert .ym files to .ysg only"
	@echo "  make build/NAME.a78   Build a single song .a78"
	@echo "  make build/NAME.rom   Build a single song signed ROM"
	@echo "  make clean            Remove all build artifacts"
	@echo ""
	@echo "Variables:"
	@echo "  YM_DIR=path           Source directory for .ym files (default: ../../tests/fixtures/song)"
	@echo "  LYM=path              Path to lym binary (default: ../../target/release/lym)"
	@echo "  A78TOOL=path          Path to a78tool binary (default: a78tool)"
	@echo "  YSG_MAX_BYTES=N       Truncate songs longer than N bytes (default: 28672)"

$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

# --- Convert .ym to .ysg (also produces .ysi ca65 include) ---

$(BUILD_DIR)/%.ysg: $(YM_DIR)/%.ym | $(BUILD_DIR)
	@echo "  Rendering YSG: $*"
	@$(LYM) song render --input $< --output $@ --compression full --max-bytes $(YSG_MAX_BYTES) \
		|| { rm -f $@ $(BUILD_DIR)/$*.ysi; echo "  SKIP $*: unsupported format"; touch $@; }

$(BUILD_DIR)/%.ysg: $(YM_DIR)/%.YM | $(BUILD_DIR)
	@echo "  Rendering YSG: $*"
	@$(LYM) song render --input $< --output $@ --compression full --max-bytes $(YSG_MAX_BYTES) \
		|| { rm -f $@ $(BUILD_DIR)/$*.ysi; echo "  SKIP $*: unsupported format"; touch $@; }

# --- Assemble player once — music_data is imported from the per-song music module ---

$(BUILD_DIR)/player.o: ysg_player.asm ysg.inc | $(BUILD_DIR)
	@echo "  Assembling player"
	@$(CA65) $(CA65_FLAGS) $< -o $@

# --- Generate and assemble per-song music module ---

$(BUILD_DIR)/%.music.asm: $(BUILD_DIR)/%.ysg
	@printf '.segment "RODATA"\n.export music_data\nmusic_data:\n    .incbin "%s"\n' \
		"$(BUILD_DIR)/$*.ysg" > $@

$(BUILD_DIR)/%.music.o: $(BUILD_DIR)/%.music.asm
	@$(CA65) $(CA65_FLAGS) $< -o $@

# --- Link player + music into ROM ---

$(BUILD_DIR)/%.bin: $(BUILD_DIR)/player.o $(BUILD_DIR)/%.music.o
	@if [ -s "$(BUILD_DIR)/$*.ysg" ]; then \
		echo "  Linking: $*"; \
		$(LD65) $(LD65_FLAGS) $^ -o $@ \
			|| { rm -f $@; echo "  SKIP $*: song too large for 32KB ROM"; touch $@; }; \
	else \
		touch $@; \
	fi

$(BUILD_DIR)/%.rom: $(BUILD_DIR)/%.bin
	@if [ -s "$<" ]; then \
		cp "$<" "$@"; \
		$(SIGN) -w "$@" >/dev/null 2>&1 && $(SIGN) -t "$@" 2>/dev/null || true; \
	else \
		touch $@; \
	fi

$(BUILD_DIR)/%.a78: $(BUILD_DIR)/%.rom
	@if [ -s "$<" ]; then \
		echo "  Packaging A78: $*"; \
		$(A78TOOL) --input "$<" --output "$@" --title "$*"; \
	else \
		touch $@; \
	fi

# --- Utilities ---

clean:
	@rm -rf $(BUILD_DIR)
