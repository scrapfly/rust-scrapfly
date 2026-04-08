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
	git branch | grep \* | cut -d ' ' -f2 | grep main || exit 1
	git pull origin main
	$(MAKE) generate-docs
	git add docs
	-git commit -m "Update API documentation for version $(VERSION)"
	-git push origin main
	cargo publish --dry-run
	git tag -a $(VERSION) -m "Version $(VERSION)"
	git push --tags
	cargo publish
	$(MAKE) bump VERSION=$(NEXT_VERSION)

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-features
