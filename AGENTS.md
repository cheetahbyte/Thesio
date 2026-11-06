# Thesio

Thesio is Obsidian for Typst: a plain-file vault of `.typ` notes and papers. Read PRODUCT.md for direction and docs/ARCHITECTURE.md before changing the engine.

## Code

- rustfmt stays off: the compact style is deliberate. Match the surrounding code and its comment density.
- `crates/core` (`thesio_core`) stays free of GPUI: vault, compiler, buffer, syntax. Dependencies point from the root crate to core only.
- A document's id is its vault-relative path. Anything that renames or moves a file goes through `Workspace::path_changed`.
- Plain files come first. Keep app state in `.thesio/` only for what a file cannot hold, and keep the vault readable without the app.
- `library.yaml` is the source of truth for citations. Never overwrite a library file that failed to parse.
- All vault paths pass through `Store::path`, which rejects anything leaving the vault.
- Icons are `Icon` variants in `src/icon.rs`: an SF Symbol for macOS and a file in `assets/icons/` for other platforms. Never ship SF Symbol outlines as files.
- Colors use `Theme::*` tokens, then the `theme::craft` scale. Shadows use `theme::shadow_*`, menus use `ui::popover` and `ui::menu_row`.
- Record every `vendor/` change in vendor/PATCHES.md. Patch markers there read `quiet.md:` for provenance.
- Bump `store::FORMAT` when a change to `.thesio/` would mislead older builds. New Meta fields take `#[serde(default)]`.

## Checks

Run `scripts/check.sh` (tests plus clippy with zero warnings) before committing. Without the Metal Toolchain, add `--features gpui/runtime_shaders` to the cargo commands.

## User data

`~/Documents/Thesio` holds real user documents. Tests and scripted runs use a temporary vault through `THESIO_HOME`. Stop only processes running this checkout's own binary.

## UI scripts

Follow tools/harness/README.md when driving the app. Use `clip` before `key cmd-v`, and `pick` for file dialogs.
