# Thesio

A local-first workspace for academic research and writing. See [PRODUCT.md](PRODUCT.md) for the pitch.

Thesio starts from [quiet.md](https://github.com/s1dashu/quiet-md) commit `937b254` (MIT), a native Rust and GPUI notes app. The app still behaves like that notes app: documents are stored as DocxXML, and there is no citation or Typst support yet.

## Build

Requires macOS 13 or later and Xcode with the Metal Toolchain. `rust-toolchain.toml` pins the Rust version.

```sh
xcodebuild -downloadComponent MetalToolchain  # once
cargo build --release
scripts/preview.sh                            # bundle and launch dist-preview/Thesio.app
```

To build without the Metal Toolchain, compile shaders at startup instead: `cargo build --release --features gpui/runtime_shaders`.

The default library is `~/Documents/Thesio`. Set `THESIO_HOME` to use another folder.

## Docs

Conventions are in [AGENTS.md](AGENTS.md). The docs under [docs/](docs/) describe the architecture and storage format inherited from quiet.md.

## License

MIT, see [LICENSE](LICENSE). Third-party code and assets are listed in [THIRD_PARTY.md](THIRD_PARTY.md).
