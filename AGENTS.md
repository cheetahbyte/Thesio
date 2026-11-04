# Thesio

Read PRODUCT.md for direction and docs/ARCHITECTURE.md before changing the engine. Code is the source of truth for UI details.

## Code

- rustfmt stays off: the compact style is deliberate. Match the surrounding code and its comment density.
- `crates/core` (`thesio_core`) is the document engine and stays free of GPUI: model, format, editing core, library, import, Markdown export. Dependencies point from the root crate to core only.
- Structural document edits go through `editor::Core`, in order: `checkpoint`, Core method, `Editor::changed()`. Add each new structural operation to `step` in `fuzz_tests.rs`.
- Only `BlockKind::nests()` blocks and containers (quote, callout, grid, column) have children. Code blocks hold plain text.
- DocxXML output is stable: `crates/core/tests/fixtures/all-blocks.xml` must round-trip unchanged. Add the canonical form of each new block there.
- Colors use `Theme::*` tokens, then the `theme::craft` scale. Shadows use `theme::shadow_*`, menus use `ui::popover` and `ui::menu_row`.
- Record every `vendor/` change in vendor/PATCHES.md. Patch markers there read `quiet.md:` for provenance.
- Bump `store::FORMAT` when a library layout change would mislead older builds. New Meta fields take `#[serde(default)]`.
- User-facing strings go through `tr(zh, en)`.

## Checks

Run `scripts/check.sh` (tests plus clippy with zero warnings) before committing. After editing the core, also run `THESIO_FUZZ_SEEDS=2000 cargo test --release random_edits`.

Code moves that keep the UI identical must pass `tools/harness/compare.py` with matching screenshots.

## User data

`~/Documents/Thesio` and `~/Documents/Quiet.md` hold real user documents. Tests use a temporary library through `THESIO_HOME`. Stop only processes running this checkout's own binary.

## UI scripts

Follow tools/harness/README.md when driving the app. Use `clip` before `key cmd-v`, and `pick` for file dialogs.
