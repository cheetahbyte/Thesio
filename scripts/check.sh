#!/bin/sh
# Before every commit: tests (including a short fuzz run) and clippy, zero warnings.
# Longer fuzzing: THESIO_FUZZ_SEEDS=2000 scripts/check.sh
set -e
cd "$(dirname "$0")/.."
cargo test --release --workspace --locked
cargo clippy --release --workspace --locked --all-targets -- -D warnings
