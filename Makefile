# Scrapfly Rust SDK — release/dev Makefile.
# Target names mirror sdk/python/Makefile for muscle-memory parity.

VERSION ?=
NEXT_VERSION ?=

.PHONY: init install dev bump generate-docs release fmt lint test

init:
	rustup toolchain install stable
	rustup component add clippy rustfmt

install:
	cargo fetch
	cargo build --all-targets

dev:
	cargo build
	cargo test --no-run

bump:
	@if [ -z "$(VERSION)" ]; then echo "Usage: make bump VERSION=x.y.z"; exit 2; fi
	sed -i "0,/^version = .*/s//version = \"$(VERSION)\"/" Cargo.toml
	git add Cargo.toml
	git commit -m "bump version to $(VERSION)"
	git push

generate-docs:
	cargo doc --no-deps --all-features
	rm -rf docs && cp -r target/doc docs

release:
	@if [ -z "$(VERSION)" ]; then echo "Usage: make release VERSION=x.y.z [NEXT_VERSION=x.y.(z+1)]"; exit 2; fi
	@[ "$$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "release must run on main"; exit 1; }
	git pull origin main
	sed -i "0,/^version = .*/s//version = \"$(VERSION)\"/" Cargo.toml
	cargo fmt --all
	-cargo clippy --fix --allow-dirty --allow-staged --all-targets -- -D warnings
	cargo fmt --all
	cargo clippy --all-targets -- -D warnings
	@if [ -z "$(SKIP_TESTS)" ]; then cargo test --lib --release; else echo "SKIP_TESTS set, skipping test gate"; fi
	$(MAKE) generate-docs
	git add -A
	-git commit -m "Release $(VERSION)"
	-git push origin main
	@TAG_V="$(VERSION)"; CARGO_V=$$(grep -m1 '^version' Cargo.toml | sed -E 's/version *= *"([^"]+)".*/\1/'); \
		[ "$$TAG_V" = "$$CARGO_V" ] || { echo "ABORT: tag v$$TAG_V != Cargo.toml $$CARGO_V"; exit 1; }
	cargo publish --dry-run
	git tag -a v$(VERSION) -m "Version $(VERSION)"
	git push origin v$(VERSION)
	@if [ -n "$(NEXT_VERSION)" ]; then $(MAKE) bump VERSION=$(NEXT_VERSION); fi

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-features
