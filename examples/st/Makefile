# Atari ST Examples Top-Level Makefile
# Delegates builds to subdirectories: music, mix, and rust

LYM_BIN := ../target/release/lym

.PHONY: all music mix rust clean help ensure-lym

all: ensure-lym music mix rust

ensure-lym:
	@if [ ! -f "$(LYM_BIN)" ]; then \
		echo "=== 'lym' release binary not found. Building with Cargo... ==="; \
		(cd .. && cargo build --release --bin lym); \
	fi

music: ensure-lym
	@echo "=== Building Atari ST Music Sample ==="
	@$(MAKE) -C music -k all

mix: ensure-lym
	@echo "=== Building Atari ST Music & SFX Mix Sample ==="
	@$(MAKE) -C mix all

rust:
	@echo "=== Building Atari ST Native 68000 Rust Sample ==="
	@$(MAKE) -C rust all

clean:
	@echo "=== Cleaning Atari ST Examples ==="
	@$(MAKE) -C music clean
	@$(MAKE) -C mix clean
	@$(MAKE) -C rust clean

help:
	@echo "Atari ST Sample Builds"
	@echo ""
	@echo "Targets:"
	@echo "  make all       Build all ST samples (music, mix, rust)"
	@echo "  make music     Build music sample .tos executables"
	@echo "  make mix       Build interactive music & SFX mix sample .tos"
	@echo "  make rust      Build native 68000 Rust sample .tos executable"
	@echo "  make clean     Remove all build artifacts across subdirectories"
