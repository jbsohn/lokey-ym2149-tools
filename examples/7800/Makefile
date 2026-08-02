# Atari 7800 Examples Top-Level Makefile
# Delegates builds to subdirectories: music and mix

LYM_BIN := ../../target/release/lym

.PHONY: all music mix clean help ensure-lym

all: ensure-lym music mix

ensure-lym:
	@if [ ! -f "$(LYM_BIN)" ]; then \
		echo "=== 'lym' release binary not found. Building with Cargo... ==="; \
		(cd ../.. && cargo build --release --bin lym); \
	fi

music: ensure-lym
	@echo "=== Building Atari 7800 Music Sample ==="
	@$(MAKE) -C music -k all

mix: ensure-lym
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
	@echo "  make all       Build all 7800 samples (auto-builds 'lym' via cargo if missing)"
	@echo "  make music     Build music sample ROMs"
	@echo "  make mix       Build interactive music & SFX mix sample ROM"
	@echo "  make clean     Remove all build artifacts across subdirectories"
