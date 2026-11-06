# thesio-typst

Typst, typst-ide and Hayagriva behind a C ABI, for the Go app. The ABI is
declared in `include/thesio_typst.h`; `src/ffi.rs` implements it.

Build the static library with `scripts/build-typst.sh` (or `cargo build --release`
here). It writes `target/release/libthesio_typst.a`, which `internal/typst` links
through cgo, so build it before any Go build that needs Typst (`CGO_ENABLED=1`).
Go rebuilds stay fast because the library only changes when this crate does.

The crate has its own `[workspace]`, `Cargo.lock` and `rust-toolchain.toml`, so
Typst stays at the pinned version. Run its tests with `cargo test` here.
