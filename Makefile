CARGO ?= cargo
CARGO_TARGET_DIR ?= $(abspath ../target)
CARGO_BUILD_JOBS ?= 2
EXTENSION_PACKAGE := irodori-extension-iceberg.tar.gz
LIB_NAME := irodori_extension_iceberg
DUCKDB_ENV := DUCKDB_DOWNLOAD_LIB=1
DUCKDB_RUSTFLAGS ?= -C link-arg=-Wl,-rpath,$$ORIGIN
export CARGO_TARGET_DIR
export CARGO_BUILD_JOBS

.PHONY: build check fmt lint test package clean

check: fmt lint test


fmt:
	$(CARGO) fmt --check

lint:
	$(DUCKDB_ENV) RUSTFLAGS='$(DUCKDB_RUSTFLAGS)' $(CARGO) clippy --all-targets -- -D warnings

build:
	$(DUCKDB_ENV) RUSTFLAGS='$(DUCKDB_RUSTFLAGS)' $(CARGO) build --release

test:
	$(DUCKDB_ENV) RUSTFLAGS='$(DUCKDB_RUSTFLAGS)' $(CARGO) test

package: build
	mkdir -p dist/native
	rm -f dist/native/libirodori_extension_*.so dist/native/irodori_extension_*.dll dist/native/libirodori_extension_*.dylib dist/native/libduckdb.so dist/native/duckdb.dll dist/native/libduckdb.dylib
	cp $(CARGO_TARGET_DIR)/release/lib$(LIB_NAME).so dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/$(LIB_NAME).dll dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/lib$(LIB_NAME).dylib dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/deps/libduckdb.so dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/deps/duckdb.dll dist/native/ 2>/dev/null || true
	cp $(CARGO_TARGET_DIR)/release/deps/libduckdb.dylib dist/native/ 2>/dev/null || true
	tar -czf dist/$(EXTENSION_PACKAGE) README.md LICENSE-MIT LICENSE-0BSD connector.config.json connector.source.json irodori.extension.json dist/native

clean:
	$(CARGO) clean
