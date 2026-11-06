#!/bin/sh
# Before every commit and in CI: the Rust library's tests and build, then the
# Go module held to the strict bar: formatting, vet, lint, tests with the race
# detector and coverage, known vulnerabilities, and unsafe pointer checks.
# --rust-only and --go-only run one half, as CI does when the library is cached.
set -eu
cd "$(dirname "$0")/.."
mode=${1:-all}
if [ "$mode" != --go-only ]; then
  # The same --target as build-typst.sh, so the tests and the library share one build.
  host=$(rustc -vV | sed -n 's/^host: //p')
  (cd typst && cargo test --release --target "$host" && cargo clippy --release --target "$host" --all-targets -- -D warnings)
  scripts/build-typst.sh
fi
[ "$mode" != --rust-only ] || exit 0
export CGO_ENABLED=1

unformatted=$(gofmt -l . | grep -v '^typst/' || true)
if [ -n "$unformatted" ]; then
  echo "gofmt: $unformatted"
  exit 1
fi
go vet ./...
golangci-lint run ./...
go test -count=1 -race -cover ./...
govulncheck ./...
# checkptr finds unsafe.Pointer misuse at run time; the cgo and purego packages are where it matters.
go test -count=1 -gcflags=all=-d=checkptr=2 ./internal/typst/ ./internal/pdfkit/
