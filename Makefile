# Developer aliases. The binary installs itself; there is nothing else to do.
.PHONY: install status dev test-dev ci brew-linux

install:
	cargo build --release
ifeq ($(shell uname -s),Darwin)
	cargo build --release --manifest-path app/Cargo.toml
	scripts/package-app.sh app/target/release/rhebok-app "$$(target/release/rhebok --version | awk '{print $$2}')" --dev target/release/rhebok-app-aarch64-apple-darwin.tar.gz
	cd target/release && shasum -a 256 rhebok-app-aarch64-apple-darwin.tar.gz > rhebok-app-aarch64-apple-darwin.sha256
endif
	target/release/rhebok install

status:
	@target/release/rhebok status

dev:
	cargo build --no-default-features

test-dev:
	cargo test --no-default-features

ci:
	scripts/ci-local.sh

brew-linux:
	scripts/homebrew-linux-test.sh
