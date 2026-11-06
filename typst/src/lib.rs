//! Typst for Thesio behind a C ABI (`ffi.rs`, declared in
//! `include/thesio_typst.h`): compiling a vault, rendering, PDF, `typst_ide`
//! questions, source rewrites through Typst's parser, and Hayagriva.

pub mod bibliography;
pub mod buffer;
pub mod compiler;
pub mod ffi;
pub mod ide;
pub mod images;
pub mod library;
pub mod packages;
pub mod paths;
pub mod project;
pub mod references;
pub mod syntax;
pub mod time;
pub mod typst_source;
