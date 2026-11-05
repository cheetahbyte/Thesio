#!/bin/zsh
# Replace the running hand-test build (dist-preview) with target/release; see
# bundle.sh for the library it runs on.
# Only this checkout's own binaries are stopped:
# pkill matches this checkout's absolute path. Log: /tmp/thesio-preview.log
set -e
cd "$(dirname "$0")/.."
bin="$PWD/dist-preview/Thesio.app/Contents/MacOS/thesio"
pkill -f "^$bin" || true
sleep 1
scripts/bundle.sh
(nohup "$bin" >/tmp/thesio-preview.log 2>&1 &)
sleep 2; pgrep -fl "^$bin"
