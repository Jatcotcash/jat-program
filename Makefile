.PHONY: build test fmt fmt-check lint check idl clean

build:
	anchor build

# host tests: real proof verifies through groth16-solana, on-chain tree root
# reproduces the proof's merkle root. No validator needed.
test:
	cargo test --workspace

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace -- -W warnings

check:
	cargo check --workspace

idl:
	anchor idl build

clean:
	cargo clean
	rm -rf .anchor target
