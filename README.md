# Thesio

A local-first workspace for academic research and writing. See [PRODUCT.md](PRODUCT.md) for the pitch.

Thesio edits plain `.typ` files in a folder you own, with a live preview compiled by [Typst](https://typst.app/). Notes and papers share one vault, so `#include` and citations work across everything. Its window and sidebar started from [quiet.md](https://github.com/s1dashu/quiet-md) commit `937b254` (MIT).

## Build

Requires macOS 13 or later and Xcode with the Metal Toolchain. `rust-toolchain.toml` pins the Rust version.

```sh
xcodebuild -downloadComponent MetalToolchain  # once
cargo build --release
scripts/preview.sh                            # bundle and launch dist-preview/Thesio.app
```

To build and run a debug version, install `just` and run `just start`.

To build without the Metal Toolchain, compile shaders at startup instead: `cargo build --release --features gpui/runtime_shaders`.

The default vault is `~/Documents/Thesio`. Set `THESIO_HOME` to use another folder.

## Docs

Conventions are in [AGENTS.md](AGENTS.md). The architecture and vault format are described in [docs/](docs/).

## License

MIT, see [LICENSE](LICENSE). Third-party code and assets are listed in [THIRD_PARTY.md](THIRD_PARTY.md).
