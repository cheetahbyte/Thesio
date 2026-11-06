#!/bin/sh
# Builds typst/target/release/libthesio_typst.a, which internal/typst links with cgo.
set -e
cd "$(dirname "$0")/../typst"
cargo build --release
