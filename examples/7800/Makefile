# Atari 7800 Examples Top-Level Makefile
# Delegates builds to subdirectories: music and mix

.PHONY: all music mix clean help

all: music mix

music:
	@echo "=== Building Atari 7800 Music Sample ==="
	@$(MAKE) -C music -k all

mix:
	@echo "=== Building Atari 7800 Music & SFX Mix Sample ==="
	@$(MAKE) -C mix all

clean:
	@echo "=== Cleaning Atari 7800 Examples ==="
	@$(MAKE) -C music clean
	@$(MAKE) -C mix clean

help:
	@echo "Atari 7800 Sample Builds"
	@echo ""
	@echo "Targets:"
	@echo "  make all       Build all 7800 samples (music and mix)"
	@echo "  make music     Build music sample ROMs"
	@echo "  make mix       Build interactive music & SFX mix sample ROM"
	@echo "  make clean     Remove all build artifacts across subdirectories"
