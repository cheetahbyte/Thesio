#!/bin/sh
# Regenerate the dependency tables in THIRD_PARTY.md: the Rust crates of typst/
# from cargo metadata, and the Go modules from go list. Everything between the
# first table header and "## Fonts" is replaced; the rest stays.
set -e
cd "$(dirname "$0")/.."
{
  echo "## Rust dependencies"
  echo
  echo "The following table is generated from \`cargo metadata\` in \`typst/\`. It records declared licenses, including MPL-2.0, and does not replace each component's license text."
  echo
  echo "| Package | Version | License |"
  echo "|---|---|---|"
  (cd typst && cargo metadata --locked --format-version 1) | python3 -c '
import json, sys
m = json.load(sys.stdin)
ws = set(m["workspace_members"])
for p in sorted((p for p in m["packages"] if p["id"] not in ws), key=lambda p: p["name"].lower()):
    print("| " + p["name"] + " | " + p["version"] + " | " + (p.get("license") or "—") + " |")
'
  echo
  echo "## Go dependencies"
  echo
  echo "The following table is generated from \`go list -m all\`. Licenses are in each module's source."
  echo
  echo "| Module | Version |"
  echo "|---|---|"
  go list -m all | tail -n +2 | awk '{ print "| " $1 " | " $2 " |" }'
  echo
} > /tmp/thesio-deps.md
python3 - <<'EOF'
src = open("THIRD_PARTY.md").read()
head = src[: src.index("## Rust dependencies")]
tail = src[src.index("## Fonts") :]
open("THIRD_PARTY.md", "w").write(head + open("/tmp/thesio-deps.md").read() + tail)
EOF
echo "THIRD_PARTY.md updated"
