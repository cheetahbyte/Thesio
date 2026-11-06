#!/bin/sh
# Before every commit: tests and clippy, zero warnings.
set -e
cd "$(dirname "$0")/.."
cargo test --release --workspace --locked
cargo clippy --release --workspace --locked --all-targets -- -D warnings
