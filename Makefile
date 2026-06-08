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
	@[ "$$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "release must run on main"; exit 1; }
	git pull origin main
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	@if [ -z "$(SKIP_TESTS)" ]; then cargo test --all-features; else echo "SKIP_TESTS set, skipping test gate"; fi
	$(MAKE) generate-docs
	git add docs
	-git commit -m "Update API documentation for version $(VERSION)"
	-git push origin main
	cargo publish --dry-run
	@# Tag format: v$(VERSION). The release.yml GitHub Action triggers on
	@# `tags: v*` — a bare `$(VERSION)` tag (no v prefix) is silently ignored
	@# by the workflow, so cargo publish never runs. Matches historical tags
	@# v0.1.1 / v0.2.1 that worked.
	git tag -a v$(VERSION) -m "Version $(VERSION)"
	@# Push ONLY the new tag, not --tags (avoids pushing stale local tags).
	@# cargo publish is handled by the release.yml workflow that fires on the
	@# pushed v-tag; local cargo publish is removed because it needs
	@# CARGO_REGISTRY_TOKEN which lives in CI, not on dev machines.
	git push origin v$(VERSION)
	$(MAKE) bump VERSION=$(NEXT_VERSION)

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-features
