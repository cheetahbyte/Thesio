//! Compiles Typst documents from a vault: a plain folder whose root is the
//! Typst project root (paths starting with `/` also try the main file's
//! folders; see `VaultWorld::resolved`), so `#include`, `#image`, and `#bibliography` reach any
//! file in it. Unsaved editor text is held in buffers that shadow the disk.

use anyhow::{Context, Result};
use rustc_hash::FxHashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::layout::{Frame, FrameItem};
use typst::syntax::{FileId, RootedPath, Source, Span, VirtualPath, VirtualRoot};
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

    /// For each page, whether it shows content from `path` or a file `path`
    /// includes. Every page counts for the main file, and for a file that puts
    /// nothing on the page, such as one holding only definitions.
    pub fn pages_from(&self, document: &PagedDocument, path: &str) -> Vec<bool> {
        let every = vec![true; document.pages().len()];
        if file_id(path).ok() == self.world.main { return every; }
        let mut files = rustc_hash::FxHashSet::default();
        let mut stack = vec![path.to_string()];
        while let Some(file) = stack.pop() {
            let Ok(id) = file_id(&file).map(|id| self.world.resolved(id)) else { continue };
            if !files.insert(id) { continue; }
            if let Ok(source) = self.world.source(id) { stack.extend(crate::references::included_files(source.text(), &file)); }
        }
        let own: Vec<bool> = document.pages().iter().map(|page| shows(&page.frame, &files)).collect();
        if own.contains(&true) { own } else { every }
    }
}

/// Whether anything drawn in `frame` came from `files`.
fn shows(frame: &Frame, files: &rustc_hash::FxHashSet<FileId>) -> bool {
    let from = |span: Span| span.id().is_some_and(|id| files.contains(&id));
    frame.items().any(|(_, item)| match item {
        FrameItem::Group(group) => shows(&group.frame, files),
        FrameItem::Text(text) => text.glyphs.iter().any(|glyph| from(glyph.span.0)),
        FrameItem::Shape(_, span) | FrameItem::Image(_, _, span) => from(*span),
        FrameItem::Link(..) | FrameItem::Tag(_) => false,
    })
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
    /// The file `id` means. Typst runs one project per root, but a vault holds
    /// several: `/template.typ` is sought from the main file's folder upward,
    /// then at the vault root, as `typst compile` run in that project would.
    fn resolved(&self, id: FileId) -> FileId {
        let (Some(main), VirtualRoot::Project) = (self.main, id.get().root()) else { return id };
        let main = main.get().vpath().get_without_slash().to_string();
        let file = id.get().vpath().get_without_slash();
        let mut dir = main.as_str();
        while let Some((parent, _)) = dir.rsplit_once('/') {
            dir = parent;
            let candidate = format!("{dir}/{file}");
            let Ok(found) = file_id(&candidate) else { continue };
            if self.buffers.contains_key(&found) || self.root.join(&candidate).is_file() { return found; }
        }
        id
    }

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
        let id = self.resolved(id);
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
        let id = self.resolved(id);
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

/// Each page's hash of content and scale, and its image at `scales[page]`
/// unless `known` already has that hash.
pub fn render_changed(document: &PagedDocument, scales: &[f32], known: &[u128]) -> Vec<(u128, Option<PageImage>)> {
    (0..document.pages().len()).map(|i| {
        let hash = typst::utils::hash128(&(&document.pages()[i], scales[i].to_bits()));
        (hash, (!known.contains(&hash)).then(|| render_page(document, i, scales[i])).flatten())
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("thesio-compiler-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    fn errors(compilation: &Compilation) -> Vec<String> {
        compilation.diagnostics.iter().filter(|d| d.error).map(|d| format!("{:?}: {}", d.path, d.message)).collect()
    }

    #[test]
    fn root_paths_start_at_the_project_folder() {
        let root = vault("project", &[
            ("template.typ", "#let who = \"wrong folder\""),
            ("Thesis/template.typ", "#let who = read(\"name.txt\")"),
            ("Thesis/name.txt", "right folder"),
            ("Thesis/thesis.typ", "#include \"chapters/one.typ\""),
            ("Thesis/chapters/one.typ", "#import \"/template.typ\": who\n#assert(who == \"right folder\")"),
            ("Thesis/exposes/1.typ", "#import \"/template.typ\": who\n#assert(who == \"right folder\")"),
        ]);
        let mut compiler = Compiler::new(&root);
        for main in ["Thesis/thesis.typ", "Thesis/exposes/1.typ"] {
            let compilation = compiler.compile(main).unwrap();
            assert_eq!(errors(&compilation), Vec::<String>::new(), "{main}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn root_paths_see_unsaved_text_and_report_the_real_file() {
        let root = vault("buffer", &[
            ("Thesis/template.typ", "#let who = 1"),
            ("Thesis/thesis.typ", "#import \"/template.typ\": who\n#who"),
        ]);
        let mut compiler = Compiler::new(&root);
        compiler.set_buffer("Thesis/template.typ", "#let who = 1\n#panic(\"unsaved\")").unwrap();
        let compilation = compiler.compile("Thesis/thesis.typ").unwrap();
        let found = errors(&compilation);
        assert!(found.iter().any(|e| e.contains("Thesis/template.typ") && e.contains("unsaved")), "{found:?}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn root_paths_outside_a_project_still_reach_the_vault() {
        let root = vault("shared", &[
            ("shared.typ", "#let who = 1"),
            ("Thesis/thesis.typ", "#import \"/shared.typ\": who\n#who"),
        ]);
        let compilation = Compiler::new(&root).compile("Thesis/thesis.typ").unwrap();
        assert_eq!(errors(&compilation), Vec::<String>::new());
        std::fs::remove_dir_all(root).unwrap();
    }
}
