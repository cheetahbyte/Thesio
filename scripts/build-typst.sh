#!/bin/sh
# Builds typst/target/<triple>/release/libthesio_typst.a, which internal/typst
# links with cgo: for this machine's triple, or for each triple given, as in
#   scripts/build-typst.sh aarch64-apple-darwin x86_64-apple-darwin
set -eu
cd "$(dirname "$0")/../typst"
[ $# -gt 0 ] || set -- "$(rustc -vV | sed -n 's/^host: //p')"
for target in "$@"; do
  # CI restores finished libraries from its cache and skips those.
  if [ "${THESIO_SKIP_BUILT:-}" = 1 ] && [ -f "target/$target/release/libthesio_typst.a" ]; then
    echo "libthesio_typst.a for $target is built already"
    continue
  fi
  # rustup runs in typst/, so it adds the target to the pinned toolchain.
  rustup target list --installed | grep -qx "$target" || rustup target add "$target"
  cargo build --release --target "$target"
done
