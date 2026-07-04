CARGO ?= cargo
CARGO_TARGET_DIR ?= ../target
CARGO_BUILD_JOBS ?= 2
EXTENSION_PACKAGE := irodori-extension-iceberg.tar.gz
LIB_NAME := irodori_extension_iceberg
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
	rm -f dist/native/libirodori_extension_*.so dist/native/irodori_extension_*.dll dist/native/libirodori_extension_*.dylib
	cp $(CARGO_TARGET_DIR)/release/lib$(LIB_NAME).so dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/$(LIB_NAME).dll dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/lib$(LIB_NAME).dylib dist/native/ 2>/dev/null || true
	tar -czf dist/$(EXTENSION_PACKAGE) README.md LICENSE-MIT LICENSE-0BSD connector.config.json connector.source.json irodori.extension.json dist/native

clean:
	$(CARGO) clean
