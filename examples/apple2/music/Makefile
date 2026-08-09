# Apple II Mockingboard YSG Player Sample Makefile
# Assembler: ca65 / ld65 (cc65 toolchain)
# Usage: make -k all    (build all songs, skip failures)

BUILD_DIR    := build
YM_DIR       ?= ../../../tests/fixtures/song
LYM          ?= ../../../target/release/lym
CA65         := ca65
LD65         := ld65
CA65_FLAGS   := -I. -I../include
LD65_FLAGS   := -C apple2.cfg
# apple2.cfg reserves $0803-$9600 (the standard safe ceiling below DOS 3.3's
# resident code/buffers) for player code + song data combined. Player code
# is ~500 bytes, so budget the rest to the song with a safety margin below
# ld65's hard segment-overflow check.
YSG_MAX_BYTES ?= 35000
TARGET_CLOCK  ?= 1020484

# --- Disk image (DOS 3.3, auto-boots DISK_SONG via a generated HELLO) ---
AC                ?= applecommander-ac
DISK_IMAGE        ?= $(BUILD_DIR)/music.dsk
DISK_SONG         ?= ND-Loader
APPLEWIN_TEMPLATE ?= DOS 3.3 System Master - 680-0210-A.dsk

YM_SOURCES  := $(wildcard $(YM_DIR)/*.ym) $(wildcard $(YM_DIR)/*.YM)
YSG_FILES   := $(foreach f,$(YM_SOURCES),$(BUILD_DIR)/$(notdir $(basename $(f))).ysg)
BIN_FILES   := $(YSG_FILES:.ysg=.bin)

.PHONY: all ysg clean help ensure-lym disk
.DELETE_ON_ERROR:
.PRECIOUS: $(BUILD_DIR)/player.o $(BUILD_DIR)/%.music.o $(BUILD_DIR)/%.bin

all: ensure-lym $(BIN_FILES)

ensure-lym:
	@if [ ! -f "$(LYM)" ]; then \
		echo "  Building 'lym' release binary with Cargo..."; \
		(cd ../../.. && cargo build --release --bin lym); \
	fi

ysg: $(YSG_FILES)

# --- Build a bootable DOS 3.3 disk image that auto-runs DISK_SONG ---

disk: ensure-lym $(BUILD_DIR)/$(DISK_SONG).bin | $(BUILD_DIR)
	@echo "  Building disk image: $(DISK_IMAGE) (auto-boots $(DISK_SONG))"
	@cp "$(APPLEWIN_TEMPLATE)" $(DISK_IMAGE)
	@$(AC) -d $(DISK_IMAGE) HELLO 2>/dev/null || true
	@$(AC) -p $(DISK_IMAGE) $(DISK_SONG) BIN 0x0803 < $(BUILD_DIR)/$(DISK_SONG).bin
	@printf '10 PRINT CHR$$(4);"BRUN $(DISK_SONG)"\n' | $(AC) -bas $(DISK_IMAGE) HELLO
	@echo "  Done. Boot $(DISK_IMAGE) in AppleWin (slot 4 = Mockingboard) -- it auto-runs $(DISK_SONG)."

help:
	@echo "Apple II Mockingboard YSG Player Sample"
	@echo ""
	@echo "Targets:"
	@echo "  make -k all           Build .bin files for all .ym files, skip failures"
	@echo "  make ysg              Convert .ym files to .ysg only"
	@echo "  make build/NAME.bin   Build a single song .bin"
	@echo "  make disk             Build a bootable disk image that auto-runs DISK_SONG"
	@echo "  make clean            Remove all build artifacts"
	@echo ""
	@echo "BLOAD build/NAME.bin on the Apple II, then CALL 2051 to start."
	@echo ""
	@echo "Variables:"
	@echo "  YM_DIR=path            Source directory for .ym files (default: ../../../tests/fixtures/song)"
	@echo "  LYM=path               Path to lym binary (default: ../../../target/release/lym)"
	@echo "  YSG_MAX_BYTES=N        Truncate songs longer than N bytes (default: 35000)"
	@echo "  DISK_SONG=NAME         Song to auto-boot on the disk image (default: ND-Loader)"
	@echo "  DISK_IMAGE=path        Output disk image path (default: music.dsk)"
	@echo "  APPLEWIN_TEMPLATE=path DOS 3.3 System Master template to base the disk on"

$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

# --- Convert .ym to .ysg (also produces .ysi ca65 include) ---

$(BUILD_DIR)/%.ysg: $(YM_DIR)/%.ym | $(BUILD_DIR)
	@echo "  Rendering YSG: $*"
	@$(LYM) song render --input $< --output $@ --compression full --max-bytes $(YSG_MAX_BYTES) --target-clock $(TARGET_CLOCK) \
		|| { rm -f $@ $(BUILD_DIR)/$*.ysi; echo "  SKIP $*: unsupported format"; touch $@; }

$(BUILD_DIR)/%.ysg: $(YM_DIR)/%.YM | $(BUILD_DIR)
	@echo "  Rendering YSG: $*"
	@$(LYM) song render --input $< --output $@ --compression full --max-bytes $(YSG_MAX_BYTES) --target-clock $(TARGET_CLOCK) \
		|| { rm -f $@ $(BUILD_DIR)/$*.ysi; echo "  SKIP $*: unsupported format"; touch $@; }

# --- Assemble player once ---

$(BUILD_DIR)/player.o: ysg_player.s ysg.inc | $(BUILD_DIR)
	@echo "  Assembling player"
	@$(CA65) $(CA65_FLAGS) $< -o $@

# --- Generate and assemble per-song music module ---

$(BUILD_DIR)/%.music.s: $(BUILD_DIR)/%.ysg
	@printf '.segment "RODATA"\n.export music_data\nmusic_data:\n    .incbin "%s"\n' \
		"$(BUILD_DIR)/$*.ysg" > $@

$(BUILD_DIR)/%.music.o: $(BUILD_DIR)/%.music.s
	@$(CA65) $(CA65_FLAGS) $< -o $@

# --- Link player + music into raw binary ---

$(BUILD_DIR)/%.bin: $(BUILD_DIR)/player.o $(BUILD_DIR)/%.music.o
	@if [ -s "$(BUILD_DIR)/$*.ysg" ]; then \
		echo "  Linking: $*"; \
		$(LD65) $(LD65_FLAGS) $^ -o $@ \
			|| { rm -f $@; echo "  SKIP $*: song too large"; touch $@; }; \
	else \
		touch $@; \
	fi

# --- Utilities ---

clean:
	@rm -rf $(BUILD_DIR)
