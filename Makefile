CARGO ?= cargo
CARGO_TARGET_DIR ?= ../target
CARGO_BUILD_JOBS ?= 2
export CARGO_TARGET_DIR
export CARGO_BUILD_JOBS

.PHONY: build check fmt lint test package clean

check: fmt lint test


fmt:
	$(CARGO) fmt --check

lint:
	$(CARGO) clippy --all-targets -- -D warnings

build:
	$(CARGO) build --release

test:
	$(CARGO) test

package: build
	mkdir -p dist/native
	cp $(CARGO_TARGET_DIR)/release/libirodori_extension_* dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/irodori_extension_*.dll dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/libirodori_extension_*.dylib dist/native/ 2>/dev/null || true

clean:
	$(CARGO) clean
