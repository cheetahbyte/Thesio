#!/bin/sh
# Regenerate the dependency table in THIRD_PARTY.md from cargo metadata.
# Everything between the table header and "## Fonts" is replaced; the rest stays.
set -e
cd "$(dirname "$0")/.."
cargo metadata --locked --format-version 1 | python3 -c '
import json, sys, re
m = json.load(sys.stdin)
ws = set(m["workspace_members"])
deps = sorted((p for p in m["packages"] if p["id"] not in ws), key=lambda p: p["name"].lower())
lines = []
for p in deps:
    lic = p.get("license") or "—"
    lines.append("| " + p["name"] + " | " + p["version"] + " | " + lic + " |")
src = open("THIRD_PARTY.md").read()
src = re.sub(r"[0-9]+ packages", str(len(deps)) + " packages", src)
head = src[: src.index("| Package | Version | License |")] + "| Package | Version | License |\n|---|---|---|\n"
tail = src[src.index("## Fonts") :]
open("THIRD_PARTY.md", "w").write(head + "\n".join(lines) + "\n\n" + tail)
print("THIRD_PARTY.md:", len(deps), "packages")
'
