//! Thesio's engine, free of any UI: the vault on disk, the Typst compiler,
//! the source buffer, and syntax highlighting.

pub mod biblatex;
pub mod bibliography;
pub mod buffer;
pub mod compiler;
pub mod diff;
pub mod history;
pub mod ide;
pub mod identifier;
pub mod images;
pub mod library;
pub mod lookup;
pub mod packages;
pub mod paths;
pub mod project;
pub mod quote;
pub mod references;
pub mod source_type;
pub mod store;
pub mod syntax;
pub mod time;
mod typst_source;
