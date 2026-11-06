# thesio-typst

Typst, typst-ide and Hayagriva behind a C ABI, for the Go app. The ABI is
declared in `include/thesio_typst.h`; `src/ffi.rs` implements it.

Build the static library with `scripts/build-typst.sh`. It runs
`cargo build --release --target <triple>` for this machine's triple (the `host`
of `rustc -vV`) and writes `target/<triple>/release/libthesio_typst.a`, which
`internal/typst` links through cgo, so build it before any Go build that needs
Typst (`CGO_ENABLED=1`). Go rebuilds stay fast because the library only changes
when this crate does.

Name triples to build for other targets, as a universal macOS app needs both:

```sh
scripts/build-typst.sh aarch64-apple-darwin x86_64-apple-darwin
```

The script adds missing targets with `rustup target add`. `internal/typst/cgo.go`
links the directory of each `GOOS`/`GOARCH`: `aarch64-apple-darwin`,
`x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and
`x86_64-pc-windows-gnu` (Windows builds with MinGW gcc). A plain
`cargo build --release` writes `target/release`, which Go does not link.

The crate has its own `[workspace]`, `Cargo.lock` and `rust-toolchain.toml`, so
Typst stays at the pinned version. Run its tests with `cargo test` here.
