#!/bin/sh
# Before every commit: the Rust library's tests, then vet and tests of the Go module.
set -e
cd "$(dirname "$0")/.."
(cd typst && cargo test --release)
scripts/build-typst.sh
export CGO_ENABLED=1
go vet ./...
go test ./...
