# Thesio

A local-first workspace for academic research and writing. See [PRODUCT.md](PRODUCT.md) for the pitch.

Thesio edits plain `.typ` files in a folder you own, with a live preview compiled by [Typst](https://typst.app/). Notes and papers share one vault, so `#include` and citations work across everything.

The app is Go with [MyGo](https://mygo.egoist.dev) native UI. Typst, typst-ide and Hayagriva are a Rust static library (`typst/`) behind a C ABI, linked with cgo.

## Build

Requires Go 1.27 or later, a Rust toolchain (`typst/rust-toolchain.toml` pins the version), and on macOS the Xcode command line tools for cgo.

```sh
scripts/build-typst.sh            # the Typst library, once and after changes to typst/
CGO_ENABLED=1 go run .            # the app on ~/Documents/Thesio
CGO_ENABLED=1 go tool mygo dev    # with live reload
CGO_ENABLED=1 go tool mygo build  # Thesio.app and a disk image in build/
```

With [just](https://github.com/casey/just): `just start`, `just bundle`, `just check`.

The default vault is `~/Documents/Thesio`. Set `THESIO_HOME` to use another folder, as in `THESIO_HOME=$(mktemp -d) go run .`; `testdata/sample-vault` is a small vault to try.

## Docs

Conventions are in [AGENTS.md](AGENTS.md). The architecture and vault format are described in [docs/](docs/).

## License

MIT, see [LICENSE](LICENSE). Third-party code and assets are listed in [THIRD_PARTY.md](THIRD_PARTY.md).
