BIN := gm

.PHONY: all build install test lint fmt check clean

all: build

build:
	cargo build --release
	cp target/release/$(BIN) $(BIN)

install:
	cargo install --locked --path .

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt

check: lint test
	cargo fmt --check

clean:
	cargo clean
	$(RM) $(BIN)
