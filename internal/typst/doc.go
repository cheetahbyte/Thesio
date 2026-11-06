// Package typst binds the Rust crate in typst/ (libthesio_typst.a) through cgo:
// compiling a vault with Typst, rendering pages, exporting PDF, typst-ide
// completion and hover, source rewrites that need Typst's parser, and Hayagriva.
//
// Building needs CGO_ENABLED=1 and the static library built first with
// scripts/build-typst.sh, which writes typst/target/<triple>/release for this
// machine's Rust target triple, or for each triple it is given; cgo.go picks the
// directory of GOOS and GOARCH. The ABI is declared in typst/include/thesio_typst.h.
//
// A Compiler is safe to share; its calls are serialized. A Compilation is
// immutable and safe to use from any goroutine. Compile, Render, PDF, Complete
// and Hover can take long and belong off the UI thread.
package typst
