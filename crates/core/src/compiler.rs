//! Compiles Typst documents from a vault: a plain folder whose root is the
//! Typst project root, so `#include`, `#image`, and `#bibliography` reach any
//! file in it. Unsaved editor text is held in buffers that shadow the disk.

use anyhow::{Context, Result};
use rustc_hash::FxHashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_kit::fonts::FontStore;
pub use typst_layout::PagedDocument;

/// Memoized results older than this many compilations are dropped.
const CACHE_AGE: usize = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub error: bool,
    pub message: String,
    pub hints: Vec<String>,
    /// Vault-relative path, when the diagnostic points at a vault file.
    pub path: Option<String>,
    /// Byte range in that file.
    pub range: Option<Range<usize>>,
    /// Zero-based line of the range start.
    pub line: Option<usize>,
}

pub struct Compilation {
    /// The last pass's document; `None` when compilation failed.
    pub document: Option<PagedDocument>,
    pub diagnostics: Vec<Diagnostic>,
}

/// One page as premultiplied RGBA.
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct Compiler {
    world: VaultWorld,
}

impl Compiler {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        Self { world: VaultWorld {
            root: root.into(),
            main: None,
            library: LazyHash::new(Library::default()),
            fonts,
            buffers: FxHashMap::default(),
            files: Mutex::default(),
        } }
    }

    /// Shadows a vault file with unsaved text.
    pub fn set_buffer(&mut self, path: &str, text: impl Into<String>) -> Result<()> {
        self.world.buffers.insert(file_id(path)?, text.into());
        Ok(())
    }

    pub fn clear_buffer(&mut self, path: &str) -> Result<()> {
        self.world.buffers.remove(&file_id(path)?);
        Ok(())
    }

    pub fn compile(&mut self, main: &str) -> Result<Compilation> {
        self.world.main = Some(file_id(main)?);
        let result = typst::compile::<PagedDocument>(&self.world);
        let (document, errors) = match result.output {
            Ok(document) => (Some(document), Default::default()),
            Err(errors) => (None, errors),
        };
        let diagnostics = errors.iter().chain(&result.warnings).map(|d| self.world.diagnostic(d)).collect();
        comemo::evict(CACHE_AGE);
        Ok(Compilation { document, diagnostics })
    }
}

/// Rasterizes a page; `scale` is pixels per typographic point.
pub fn render_page(document: &PagedDocument, page: usize, scale: f32) -> Option<PageImage> {
    let options = typst_render::RenderOptions { pixel_per_pt: (scale as f64).into(), render_bleed: false };
    let pixmap = typst_render::render(document.pages().get(page)?, &options);
    Some(PageImage { width: pixmap.width(), height: pixmap.height(), rgba: pixmap.data().to_vec() })
}

pub fn export_pdf(document: &PagedDocument) -> Result<Vec<u8>> {
    typst_pdf::pdf(document, &typst_pdf::PdfOptions::default()).map_err(|errors| {
        anyhow::anyhow!(errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>().join("; "))
    })
}

fn file_id(path: &str) -> Result<FileId> {
    let vpath = VirtualPath::new(path).map_err(|e| anyhow::anyhow!("{e}")).with_context(|| format!("invalid vault path {path:?}"))?;
    Ok(RootedPath::new(VirtualRoot::Project, vpath).intern())
}

#[derive(Default)]
struct FileSlot {
    stamp: Option<(SystemTime, u64)>,
    bytes: Option<Bytes>,
    source: Option<Source>,
}

struct VaultWorld {
    root: PathBuf,
    main: Option<FileId>,
    library: LazyHash<Library>,
    fonts: FontStore,
    buffers: FxHashMap<FileId, String>,
    files: Mutex<FxHashMap<FileId, FileSlot>>,
}

impl VaultWorld {
    fn path(&self, id: FileId) -> FileResult<PathBuf> {
        let rooted = id.get();
        // Packages download from the network; the vault stays offline for now.
        if !matches!(rooted.root(), VirtualRoot::Project) {
            return Err(FileError::Other(Some("Typst packages are not supported yet".into())));
        }
        rooted.vpath().realize(&self.root).map_err(FileError::Realize)
    }

    /// Current bytes of a file on disk, reread only when it changed.
    fn disk_bytes(&self, id: FileId, slot: &mut FileSlot) -> FileResult<Bytes> {
        let path = self.path(id)?;
        let meta = std::fs::metadata(&path).map_err(|e| FileError::from_io(e, &path))?;
        if meta.is_dir() { return Err(FileError::IsDirectory); }
        let stamp = meta.modified().ok().map(|time| (time, meta.len()));
        if let Some(bytes) = &slot.bytes && stamp.is_some() && slot.stamp == stamp { return Ok(bytes.clone()); }
        let bytes = Bytes::new(std::fs::read(&path).map_err(|e| FileError::from_io(e, &path))?);
        *slot = FileSlot { stamp, bytes: Some(bytes.clone()), source: slot.source.take() };
        Ok(bytes)
    }

    fn diagnostic(&self, d: &SourceDiagnostic) -> Diagnostic {
        let id = d.span.id();
        let range = self.range(d.span);
        let line = range.as_ref().zip(id).and_then(|(r, id)| self.source(id).ok()?.lines().byte_to_line(r.start));
        Diagnostic {
            error: d.severity == Severity::Error,
            message: d.message.to_string(),
            hints: d.hints.iter().map(|h| h.v.to_string()).collect(),
            path: id.filter(|id| matches!(id.get().root(), VirtualRoot::Project)).map(|id| id.get().vpath().get_without_slash().to_string()),
            range,
            line,
        }
    }
}

impl World for VaultWorld {
    fn library(&self) -> &LazyHash<Library> { &self.library }
    fn book(&self) -> &LazyHash<FontBook> { self.fonts.book() }
    fn main(&self) -> FileId { self.main.expect("compile sets the main file") }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let mut files = self.files.lock().unwrap();
        let slot = files.entry(id).or_default();
        let text = match self.buffers.get(&id) {
            Some(text) => text.clone(),
            None => {
                let bytes = self.disk_bytes(id, slot)?;
                let text = std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?;
                text.strip_prefix('\u{feff}').unwrap_or(text).to_string()
            }
        };
        // Reusing the Source reparses only the edited part.
        if let Some(source) = &mut slot.source {
            if source.text() != text { source.replace(&text); }
            return Ok(source.clone());
        }
        Ok(slot.source.insert(Source::new(id, text)).clone())
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(text) = self.buffers.get(&id) { return Ok(Bytes::from_string(text.clone())); }
        let mut files = self.files.lock().unwrap();
        self.disk_bytes(id, files.entry(id).or_default())
    }

    fn font(&self, index: usize) -> Option<Font> { self.fonts.font(index) }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let now = crate::time::now() as i64;
        let (y, m, d, ..) = match offset {
            Some(offset) => crate::time::civil((now - crate::time::local_offset() + offset.seconds() as i64) as u64),
            None => crate::time::civil(now as u64),
        };
        Datetime::from_ymd(y as i32, m as u8, d as u8)
    }
}

/// The vault-relative form of `path`, when it lies inside `root`.
pub fn vault_path(root: &Path, path: &Path) -> Option<String> {
    let vpath = VirtualPath::virtualize(root, path).ok()?;
    Some(vpath.get_without_slash().to_string())
}

/// One page's content hash, and its image unless `known` already has that hash.
pub fn render_changed(document: &PagedDocument, scale: f32, known: &[u128]) -> Vec<(u128, Option<PageImage>)> {
    (0..document.pages().len()).map(|i| {
        let hash = typst::utils::hash128(&document.pages()[i]);
        (hash, (!known.contains(&hash)).then(|| render_page(document, i, scale)).flatten())
    }).collect()
}
