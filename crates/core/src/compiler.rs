//! Compiles Typst documents from a vault: a plain folder whose root is the
//! Typst project root (paths starting with `/` also try the main file's
//! folders; see `project::candidates`), so `#include`, `#image`, and `#bibliography` reach any
//! file in it. Unsaved editor text is held in buffers that shadow the disk.
//! Compiling never reaches the network: a package not on disk is reported in
//! `Compilation::missing`, for the caller to fetch off the compile.

use crate::buffer::{difference, follow, stretch};
use crate::packages::{PackageSpec, Packages};
use crate::paths::visit_vault;
use crate::project::{self, Project};
use anyhow::{Context, Result};
use rustc_hash::{FxHashMap, FxHashSet};
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use typst::diag::{FileError, FileResult, PackageError, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::introspection::{Introspector, PagedPosition};
use typst::layout::{Abs, Frame, FrameItem, Point};
use typst::model::{FigureElem, Refable};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot, is_valid_label_literal_id};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_ide::{IdeWorld, Jump};
use typst_kit::fonts::FontStore;
pub use typst_layout::PagedDocument;

/// Memoized results older than this many compilations are dropped.
const CACHE_AGE: usize = 10;
const COMPILE_STACK: usize = 64 * 1024 * 1024;
/// How far to look for text when the caret is on none, in bytes.
const NEAREST_TEXT: usize = 64;
/// Starts the error a missing package raises, which `diagnostic` turns into a status.
const DOWNLOADING: &str = "Downloading ";

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

/// A label in the compiled document, whichever file wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLabel {
    pub key: String,
    /// A caption, a heading's text, or the key when the element has no text.
    pub title: String,
    /// What the document calls it, such as "Figure"; "Label" for anything a
    /// reference gives no number.
    pub kind: String,
    /// One-based, counting the document's pages rather than its page numbers.
    pub page: Option<usize>,
}

/// The outcome of one compile, whole: it holds the files it read, so it needs
/// no compiler to answer questions and any thread can keep it. A later edit
/// does not change what it says. `ide.rs` asks `typst_ide` through `world`.
pub struct Compilation {
    pub main: String,
    /// The last pass's document; `None` when compilation failed.
    pub document: Option<PagedDocument>,
    pub diagnostics: Vec<Diagnostic>,
    /// The document's words by vault file, in the order the files first
    /// appear. Text with no file, such as a bibliography or page numbers, does
    /// not count.
    pub words: Vec<(String, usize)>,
    /// Labels `@` can reach, in document order. The first of a repeated label.
    pub labels: Vec<DocumentLabel>,
    /// Packages this compile stopped at, to `fetch` and compile again.
    pub missing: Vec<PackageSpec>,
    pub(crate) world: Snapshot,
}

impl Compilation {
    /// Downloads a package from `missing`, on the calling thread. A failure is
    /// not returned: the next compile reports it where the import is.
    pub fn fetch(&self, spec: &PackageSpec) { self.world.resources.packages.fetch(spec); }

    /// Those of `diagnostics` in `path`, moved from the text this compile read
    /// onto `text`. Any that newer text no longer holds are left out.
    pub fn placed(&self, diagnostics: &[Diagnostic], path: &str, text: &str) -> Vec<Diagnostic> {
        let own = diagnostics.iter().filter(|d| d.range.is_some() && d.path.as_deref() == Some(path));
        let Some(compiled) = self.world.text(path) else { return vec![] };
        let (edit, new_len) = difference(&compiled, text);
        own.filter_map(|d| {
            let mut range = d.range.clone()?;
            stretch(&mut range, &edit, new_len).then(|| Diagnostic { range: Some(range), ..d.clone() })
        }).collect()
    }

    /// Byte `offset` of the text this compile read of `path`, in `text`: where
    /// the edit that replaced it starts, if one did.
    pub fn moved(&self, path: &str, offset: usize, text: &str) -> usize {
        let Some(compiled) = self.world.text(path) else { return offset };
        let (edit, new_len) = difference(&compiled, text);
        let mut at = offset..offset;
        if follow(&mut at, &edit, new_len) { at.start } else { edit.start }
    }

    /// Where byte `offset` of `path` appears: its page and height in points.
    pub fn position_of(&self, path: &str, offset: usize) -> Option<(usize, f32)> {
        let source = self.world.source(file_id(path).ok()?).ok()?;
        let document = self.document.as_ref()?;
        // Spaces, empty lines, and a heading's `=` draw nothing: use the nearest text.
        let text = source.text();
        let position = (0..=NEAREST_TEXT).flat_map(|distance| [offset.checked_sub(distance), Some(offset + distance)]).flatten()
            .filter(|at| *at <= text.len() && text.is_char_boundary(*at))
            .find_map(|at| typst_ide::jump_from_cursor(document, &source, at).into_iter().next())?;
        Some((position.page.get() - 1, position.point.y.to_pt() as f32))
    }

    /// What is at a point on a page, in points.
    pub fn target_at(&self, page: usize, x: f32, y: f32) -> Option<PageTarget> {
        let position = PagedPosition {
            page: NonZeroUsize::new(page + 1)?,
            point: Point::new(Abs::pt(x as f64), Abs::pt(y as f64)),
        };
        match typst_ide::jump_from_click(&self.world, self.document.as_ref()?, &position)? {
            Jump::File(id, offset) => vault_path(self.world.resolved(id)).map(|path| PageTarget::Source(path, offset)),
            Jump::Url(url) => self.note_target(url.as_str()).map(PageTarget::Note),
            _ => None,
        }
    }

    /// Where a link's text points. A page only holds the text, which is
    /// relative to the file it was written in, so that file is looked up.
    fn note_target(&self, literal: &str) -> Option<String> {
        let mut sources: Vec<(String, &Source)> = self.world.files.iter().filter_map(|(id, file)| Some((vault_path(*id)?, file.source.as_ref()?))).collect();
        sources.sort_by(|a, b| a.0.cmp(&b.0));
        sources.into_iter().find_map(|(path, source)| {
            crate::references::note_links(source.text(), &path).into_iter().find(|link| link.literal == literal).map(|link| link.target)
        })
    }

    /// For each page, whether it shows content from `path` or a file `path`
    /// includes. Every page counts for the main file, and for a file that puts
    /// nothing on the page, such as one holding only definitions.
    pub fn pages_from(&self, path: &str) -> Vec<bool> {
        let Some(document) = &self.document else { return vec![] };
        let every = vec![true; document.pages().len()];
        if path == self.main { return every; }
        let project = Project::within(&self.main, path, &mut |file| self.world.text(file));
        let files: FxHashSet<FileId> = project.files().filter_map(|(file, _)| file_id(file).ok()).collect();
        let own: Vec<bool> = document.pages().iter().map(|page| shows(&page.frame, &files)).collect();
        if own.contains(&true) { own } else { every }
    }
}

/// One page as premultiplied RGBA.
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What a click on a page lands on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageTarget {
    /// A vault path and byte offset.
    Source(String, usize),
    /// A note link, by the vault path it points at.
    Note(String),
}

/// What every compile shares and none changes.
struct Resources {
    library: LazyHash<Library>,
    fonts: FontStore,
    packages: Arc<Packages>,
}

pub struct Compiler {
    root: PathBuf,
    resources: Arc<Resources>,
    buffers: FxHashMap<FileId, String>,
    /// Files from disk, kept between compiles so unchanged ones are not read
    /// or parsed again.
    files: Mutex<FxHashMap<FileId, FileSlot>>,
}

impl Compiler {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self::with_packages(root, Packages::new().into()) }

    pub(crate) fn with_packages(root: impl Into<PathBuf>, packages: Arc<Packages>) -> Self {
        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        let resources = Arc::new(Resources { library: LazyHash::new(Library::default()), fonts, packages });
        Self { root: root.into(), resources, buffers: FxHashMap::default(), files: Mutex::default() }
    }

    /// Shadows a vault file with unsaved text.
    pub fn set_buffer(&mut self, path: &str, text: impl Into<String>) -> Result<()> {
        self.buffers.insert(file_id(path)?, text.into());
        Ok(())
    }

    /// Compiles `main` on a stack with room for deep documents.
    pub fn compile(&mut self, main: &str) -> Result<Compilation> {
        let id = file_id(main)?;
        let compiler = &*self;
        with_large_stack(move || compiler.run(main, id))
    }

    /// Compiles `main` once, downloading what it imports on this thread, for
    /// work with no session to show a download and compile again.
    pub fn compile_fetching(&mut self, main: &str) -> Result<Compilation> {
        loop {
            let compilation = self.compile(main)?;
            if compilation.missing.is_empty() { return Ok(compilation); }
            for spec in &compilation.missing { compilation.fetch(spec); }
        }
    }

    fn run(&self, main: &str, id: FileId) -> Compilation {
        let world = VaultWorld { compiler: self, main: id, read: Mutex::default(), missing: Mutex::default() };
        let result = typst::compile::<PagedDocument>(&world);
        let (document, errors) = match result.output {
            Ok(document) => (Some(document), Default::default()),
            Err(errors) => (None, errors),
        };
        let diagnostics = errors.iter().chain(&result.warnings).map(|d| world.diagnostic(d)).collect();
        comemo::evict(CACHE_AGE);
        let words = document.as_ref().map_or(vec![], count_words);
        let labels = document.as_ref().map_or(vec![], document_labels);
        let files = world.read.into_inner().unwrap_or_default();
        let missing = world.missing.into_inner().unwrap_or_default();
        Compilation { main: main.to_string(), document, diagnostics, words, labels, missing, world: Snapshot { resources: self.resources.clone(), root: self.root.clone(), main: id, files } }
    }
}

/// Visits everything drawn in `frame`, through its groups.
fn each_item(frame: &Frame, visit: &mut impl FnMut(&FrameItem)) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => each_item(&group.frame, visit),
            item => visit(item),
        }
    }
}

/// Whether anything drawn in `frame` came from `files`.
fn shows(frame: &Frame, files: &FxHashSet<FileId>) -> bool {
    let from = |id: Option<FileId>| id.is_some_and(|id| files.contains(&id));
    let mut found = false;
    each_item(frame, &mut |item| found = found || match item {
        FrameItem::Text(text) => text.glyphs.iter().any(|glyph| from(glyph.span.0.id())),
        FrameItem::Shape(_, span) | FrameItem::Image(_, _, span) => from(span.id()),
        _ => false,
    });
    found
}

fn count_words(document: &PagedDocument) -> Vec<(String, usize)> {
    let mut found: Vec<(FileId, usize)> = vec![];
    for page in document.pages() {
        each_item(&page.frame, &mut |item| {
            let FrameItem::Text(text) = item else { return };
            let Some(id) = text.glyphs.first().and_then(|glyph| glyph.span.0.id()) else { return };
            let words = text.text.split_whitespace().count();
            match found.iter_mut().find(|(file, _)| *file == id) {
                Some((_, count)) => *count += words,
                None => found.push((id, words)),
            }
        });
    }
    found.into_iter().filter_map(|(id, words)| Some((vault_path(id)?, words))).collect()
}

/// `@` cannot spell a label made with `label("a b")`, and leaves a trailing
/// `.` or `:` to the sentence.
fn reachable_by_at(key: &str) -> bool { is_valid_label_literal_id(key) && !key.ends_with(['.', ':']) }

fn document_labels(document: &PagedDocument) -> Vec<DocumentLabel> {
    let introspector = document.introspector();
    let labelled = introspector.query_labelled();
    let mut elements = FxHashMap::default();
    for element in &labelled {
        if let Some(label) = element.label() { elements.entry(label).or_insert(element); }
    }
    let (labels, bibliography_keys) = typst_ide::analyze_labels(document);
    labels.into_iter().take(bibliography_keys).filter_map(|(label, title)| {
        let element = elements.get(&label)?;
        let key = label.resolve().to_string();
        if !reachable_by_at(&key) { return None; }
        let title = title.map_or(String::new(), |title| title.split_whitespace().collect::<Vec<_>>().join(" "));
        let supplement = element.to_packed::<FigureElem>().map(|figure| figure.supplement().plain_text().to_string()).filter(|name| !name.is_empty());
        let kind = supplement.unwrap_or_else(|| if element.can::<dyn Refable>() { element.elem().title() } else { "Label" }.to_string());
        let page = element.location().and_then(|location| introspector.page(location)).map(NonZeroUsize::get);
        Some(DocumentLabel { title: if title.is_empty() { key.clone() } else { title }, key, kind, page })
    }).collect()
}

/// Runs `work` on a thread with room for deep documents and embedded PDFs,
/// whose colour conversion overflows a pool thread's 512 KB in a debug build.
pub(crate) fn with_large_stack<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T> {
    std::thread::scope(|scope| {
        let thread = std::thread::Builder::new().stack_size(COMPILE_STACK).spawn_scoped(scope, work).context("Could not start compiling")?;
        Ok(thread.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
    })
}

/// Rasterizes a page; `scale` is pixels per typographic point.
fn render_page(document: &PagedDocument, page: usize, scale: f32) -> Option<PageImage> {
    let options = typst_render::RenderOptions { pixel_per_pt: (scale as f64).into(), render_bleed: false };
    let pixmap = typst_render::render(document.pages().get(page)?, &options);
    Some(PageImage { width: pixmap.width(), height: pixmap.height(), rgba: pixmap.data().to_vec() })
}

pub fn export_pdf(document: &PagedDocument) -> Result<Vec<u8>> {
    with_large_stack(|| typst_pdf::pdf(document, &typst_pdf::PdfOptions::default()))?.map_err(|errors| {
        anyhow::anyhow!(errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>().join("; "))
    })
}

/// Each page's hash of content and scale, and its image at `scale(page)`
/// unless `known` already has that hash.
pub fn render_changed(document: &PagedDocument, known: &[u128], scale: impl Fn(usize) -> f32 + Sync) -> Result<Vec<(u128, Option<PageImage>)>> {
    with_large_stack(|| (0..document.pages().len()).map(|i| {
        let hash = typst::utils::hash128(&(&document.pages()[i], scale(i).to_bits()));
        (hash, (!known.contains(&hash)).then(|| render_page(document, i, scale(i))).flatten())
    }).collect())
}

fn file_id(path: &str) -> Result<FileId> {
    let vpath = VirtualPath::new(path).map_err(|e| anyhow::anyhow!("{e}")).with_context(|| format!("invalid vault path {path:?}"))?;
    Ok(RootedPath::new(VirtualRoot::Project, vpath).intern())
}

/// The vault path of a file, unless it lies outside the project, as a package does.
fn vault_path(id: FileId) -> Option<String> {
    matches!(id.get().root(), VirtualRoot::Project).then(|| id.get().vpath().get_without_slash().to_string())
}

/// The file `id` means when `main` is compiled; see `project::candidates`.
fn resolved(main: FileId, id: FileId, exists: impl Fn(FileId, &str) -> bool) -> FileId {
    let (Some(main), Some(file)) = (vault_path(main), vault_path(id)) else { return id };
    project::candidates(&main, &file).find_map(|path| file_id(&path).ok().filter(|found| exists(*found, &path))).unwrap_or(id)
}

/// Source text with the same byte offsets as the file: Typst would not take a
/// byte order mark in markup, and dropping it would shift every offset.
fn without_bom(text: &str) -> String {
    match text.strip_prefix('\u{feff}') {
        Some(rest) => format!("   {rest}"),
        None => text.to_string(),
    }
}

fn today(offset: Option<Duration>) -> Option<Datetime> {
    let now = crate::time::now() as i64;
    let (y, m, d, ..) = match offset {
        Some(offset) => crate::time::civil((now - crate::time::local_offset() + offset.seconds() as i64) as u64),
        None => crate::time::civil(now as u64),
    };
    Datetime::from_ymd(y as i32, m as u8, d as u8)
}

#[derive(Default)]
struct FileSlot {
    stamp: Option<(SystemTime, u64)>,
    bytes: Option<Bytes>,
    source: Option<Source>,
}

/// A file as one compile read it.
#[derive(Clone, Default)]
struct ReadFile {
    source: Option<Source>,
    bytes: Option<Bytes>,
}

/// One compile's view of the vault, noting each file it reads.
struct VaultWorld<'a> {
    compiler: &'a Compiler,
    main: FileId,
    read: Mutex<FxHashMap<FileId, ReadFile>>,
    missing: Mutex<Vec<PackageSpec>>,
}

impl VaultWorld<'_> {
    fn resolved(&self, id: FileId) -> FileId {
        resolved(self.main, id, |found, path| self.compiler.buffers.contains_key(&found) || self.compiler.root.join(path).is_file())
    }

    fn path(&self, id: FileId) -> FileResult<PathBuf> {
        let rooted = id.get();
        match rooted.root() {
            VirtualRoot::Project => rooted.vpath().realize(&self.compiler.root).map_err(FileError::Realize),
            VirtualRoot::Package(spec) => match self.compiler.resources.packages.find(spec)? {
                Some(root) => root.resolve(rooted.vpath()),
                None => {
                    let mut missing = self.missing.lock().unwrap();
                    if !missing.contains(spec) { missing.push(spec.clone()); }
                    Err(PackageError::Other(Some(format!("{DOWNLOADING}{spec}").into())).into())
                }
            },
        }
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
        let missing = self.missing.lock().unwrap();
        let waiting = missing.iter().find(|spec| d.message.contains(&format!("{DOWNLOADING}{spec}")));
        Diagnostic {
            error: d.severity == Severity::Error && waiting.is_none(),
            message: waiting.map_or(d.message.to_string(), |spec| format!("{DOWNLOADING}{spec}…")),
            hints: d.hints.iter().map(|h| h.v.to_string()).collect(),
            path: id.and_then(vault_path),
            range,
            line,
        }
    }
}

impl World for VaultWorld<'_> {
    fn library(&self) -> &LazyHash<Library> { &self.compiler.resources.library }
    fn book(&self) -> &LazyHash<FontBook> { self.compiler.resources.fonts.book() }
    fn main(&self) -> FileId { self.main }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let id = self.resolved(id);
        let mut files = self.compiler.files.lock().unwrap();
        let slot = files.entry(id).or_default();
        let text = match self.compiler.buffers.get(&id) {
            Some(text) => without_bom(text),
            None => {
                let bytes = self.disk_bytes(id, slot)?;
                without_bom(std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?)
            }
        };
        // Reusing the Source reparses only the edited part.
        let source = match &mut slot.source {
            Some(source) => {
                if source.text() != text { source.replace(&text); }
                source.clone()
            }
            None => slot.source.insert(Source::new(id, text)).clone(),
        };
        self.read.lock().unwrap().entry(id).or_default().source = Some(source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let id = self.resolved(id);
        let bytes = match self.compiler.buffers.get(&id) {
            Some(text) => Bytes::from_string(text.clone()),
            None => self.disk_bytes(id, self.compiler.files.lock().unwrap().entry(id).or_default())?,
        };
        self.read.lock().unwrap().entry(id).or_default().bytes = Some(bytes.clone());
        Ok(bytes)
    }

    fn font(&self, index: usize) -> Option<Font> { self.compiler.resources.fonts.font(index) }
    fn today(&self, offset: Option<Duration>) -> Option<Datetime> { today(offset) }
}

/// The files one compile read, as a world of their own.
#[derive(Clone)]
pub(crate) struct Snapshot {
    resources: Arc<Resources>,
    root: PathBuf,
    main: FileId,
    files: FxHashMap<FileId, ReadFile>,
}

impl Snapshot {
    fn resolved(&self, id: FileId) -> FileId { resolved(self.main, id, |found, _| self.files.contains_key(&found)) }

    /// This world with `text` as the file at `path`, and that text's source.
    /// Replacing the text keeps the spans of what did not change.
    pub(crate) fn with_text(&self, path: &str, text: &str) -> Option<(Self, Source)> {
        let id = file_id(path).ok()?;
        let text = without_bom(text);
        let mut world = self.clone();
        let file = world.files.entry(id).or_default();
        let source = file.source.get_or_insert_with(|| Source::new(id, text.clone()));
        if source.text() != text { source.replace(&text); }
        let source = source.clone();
        file.bytes = None;
        Some((world, source))
    }

    /// A file's text, exactly at `path`.
    fn text(&self, path: &str) -> Option<String> {
        let source = self.files.get(&file_id(path).ok()?)?.source.as_ref()?;
        Some(source.text().to_string())
    }

    fn missing(id: FileId) -> FileError { FileError::NotFound(id.get().vpath().get_without_slash().into()) }
}

impl IdeWorld for Snapshot {
    fn upcast(&self) -> &dyn World { self }

    /// Every file in the vault, read when a path is being completed.
    fn files(&self) -> Vec<FileId> {
        let mut files = vec![];
        visit_vault(&self.root, &mut |item| if !item.info.is_dir() && let Ok(id) = file_id(&item.id) { files.push(id); });
        files
    }
}

impl World for Snapshot {
    fn library(&self) -> &LazyHash<Library> { &self.resources.library }
    fn book(&self) -> &LazyHash<FontBook> { self.resources.fonts.book() }
    fn main(&self) -> FileId { self.main }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let id = self.resolved(id);
        self.files.get(&id).and_then(|file| file.source.clone()).ok_or_else(|| Self::missing(id))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let id = self.resolved(id);
        let file = self.files.get(&id).ok_or_else(|| Self::missing(id))?;
        file.bytes.clone().or_else(|| Some(Bytes::from_string(file.source.as_ref()?.text().to_string()))).ok_or_else(|| Self::missing(id))
    }

    fn font(&self, index: usize) -> Option<Font> { self.resources.fonts.font(index) }
    fn today(&self, offset: Option<Duration>) -> Option<Datetime> { today(offset) }
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

    fn chapters(name: &str) -> Compilation {
        let page_of_text = "Word ".repeat(600);
        let root = vault(name, &[
            ("main.typ", "#include \"chapters/one.typ\"\n#pagebreak()\n#include \"chapters/two.typ\"\n#pagebreak()\n#include \"chapters/three.typ\"\n"),
            ("chapters/three.typ", "= Third\n"),
            ("chapters/one.typ", "First chapter.\n"),
            ("chapters/two.typ", &format!("Second chapter.\n\n\n{page_of_text}\n")),
        ]);
        let compilation = Compiler::new(root).compile("main.typ").unwrap();
        assert!(errors(&compilation).is_empty());
        compilation
    }

    #[test]
    fn the_preview_and_the_source_point_at_each_other() {
        let compilation = chapters("jump-pages");
        let (page, y) = compilation.position_of("chapters/two.typ", 3).unwrap();
        assert_eq!(page, 1);
        let Some(PageTarget::Source(path, offset)) = compilation.target_at(page, 75., y - 3.) else { panic!("no source there") };
        assert_eq!(path, "chapters/two.typ");
        assert!(offset < "Second chapter.".len(), "{offset}");
    }

    #[test]
    fn blank_lines_take_the_place_of_the_text_before_them() {
        assert_eq!(chapters("jump-text").position_of("chapters/two.typ", 17).map(|(page, _)| page), Some(1));
    }

    #[test]
    fn a_heading_marker_takes_the_place_of_the_heading() {
        assert_eq!(chapters("jump-heading").position_of("chapters/three.typ", 0).map(|(page, _)| page), Some(2));
    }

    #[test]
    fn a_click_on_a_margin_lands_on_nothing() {
        assert_eq!(chapters("jump-margin").target_at(1, 1., 1.), None);
    }

    #[test]
    fn a_click_on_a_note_link_resolves_from_the_file_that_wrote_it() {
        let root = vault("note-link", &[
            ("main.typ", "#include \"ch/one.typ\"\n"),
            ("ch/one.typ", "#link(\"../notes/idea.typ\")[The idea] and more.\n"),
            ("notes/idea.typ", "An idea.\n"),
        ]);
        let compilation = Compiler::new(root).compile("main.typ").unwrap();
        assert!(errors(&compilation).is_empty());
        let (page, y) = compilation.position_of("ch/one.typ", 30).unwrap();
        assert_eq!(compilation.target_at(page, 75., y - 3.), Some(PageTarget::Note("notes/idea.typ".to_string())));
    }

    #[test]
    fn a_note_link_resolves_only_among_the_files_of_its_own_compile() {
        let root = vault("two-mains", &[
            ("a/main.typ", "#link(\"note.typ\")[One link] and more.\n"),
            ("b/main.typ", "#link(\"note.typ\")[One link] and more.\n"),
        ]);
        let mut compiler = Compiler::new(root);
        compiler.compile("a/main.typ").unwrap();
        let second = compiler.compile("b/main.typ").unwrap();
        let (page, y) = second.position_of("b/main.typ", 30).unwrap();
        assert_eq!(second.target_at(page, 75., y - 3.), Some(PageTarget::Note("b/note.typ".to_string())));
    }

    #[test]
    fn a_compilation_keeps_answering_after_the_next_compile() {
        let root = vault("held", &[("one.typ", "First.\n#pagebreak()\nSecond.\n"), ("two.typ", "Other.\n")]);
        let mut compiler = Compiler::new(root);
        let first = compiler.compile("one.typ").unwrap();
        let again = compiler.compile("one.typ").unwrap();
        compiler.set_buffer("one.typ", "Changed.").unwrap();
        compiler.compile("two.typ").unwrap();
        assert_eq!(first.main, "one.typ");
        for compilation in [&first, &again] {
            let (page, y) = compilation.position_of("one.typ", 20).unwrap();
            assert_eq!(page, 1);
            assert!(matches!(compilation.target_at(page, 75., y - 3.), Some(PageTarget::Source(path, _)) if path == "one.typ"));
        }
    }

    #[test]
    fn pages_belong_to_the_file_that_draws_them() {
        let root = vault("own", &[
            ("main.typ", "Front.\n#pagebreak()\n#include \"ch/one.typ\"\n#pagebreak()\n#include \"ch/defs.typ\"\nBack.\n"),
            ("ch/one.typ", "#include \"two.typ\"\n"),
            ("ch/two.typ", "Chapter.\n"),
            ("ch/defs.typ", "#let unused = 1\n"),
        ]);
        let compilation = Compiler::new(root).compile("main.typ").unwrap();
        assert_eq!(compilation.pages_from("ch/one.typ"), [false, true, false]);
        assert_eq!(compilation.pages_from("main.typ"), [true, true, true]);
        assert_eq!(compilation.pages_from("ch/defs.typ"), [true, true, true], "nothing drawn: every page");
    }

    fn range_in(diagnostic: &Diagnostic, text: &str) -> String { text[diagnostic.range.clone().unwrap()].to_string() }

    #[test]
    fn a_diagnostic_moves_onto_newer_text() {
        let text = "é #nope\n";
        let compilation = Compiler::new(vault("placed", &[("main.typ", text)])).compile("main.typ").unwrap();
        let place = |text: &str| compilation.placed(&compilation.diagnostics, "main.typ", text);
        assert_eq!(range_in(&place(text)[0], text), "nope");
        let above = format!("日本語 {text}");
        assert_eq!(range_in(&place(&above)[0], &above), "nope", "text added before it");
        let below = format!("{text}more\n");
        assert_eq!(range_in(&place(&below)[0], &below), "nope", "text added after it");
        assert_eq!(range_in(&place("é #nopes\n")[0], "é #nopes\n"), "nope", "typed right after it");
        assert_eq!(range_in(&place("é #nxpe\n")[0], "é #nxpe\n"), "nxpe", "changed inside it");
        assert!(place("é\n").is_empty(), "deleted");
    }

    #[test]
    fn a_diagnostic_stays_with_the_file_it_points_into() {
        let chapter = "é #nope\n";
        let root = vault("placed-chapter", &[("main.typ", "#include \"ch.typ\"\n"), ("ch.typ", chapter)]);
        let compilation = Compiler::new(root).compile("main.typ").unwrap();
        assert_eq!(compilation.diagnostics.len(), 1);
        assert_eq!(range_in(&compilation.placed(&compilation.diagnostics, "ch.typ", chapter)[0], chapter), "nope");
        assert!(compilation.placed(&compilation.diagnostics, "main.typ", "#include \"ch.typ\"\n").is_empty());
    }

    #[test]
    fn an_offset_follows_newer_text_or_stops_where_it_changed() {
        let compilation = Compiler::new(vault("moved", &[("main.typ", "ab #nope cd\n")])).compile("main.typ").unwrap();
        assert_eq!(compilation.moved("main.typ", 4, "日 ab #nope cd\n"), 8, "after an edit");
        assert_eq!(compilation.moved("main.typ", 4, "ab #nope cd!\n"), 4, "before an edit");
        assert_eq!(compilation.moved("main.typ", 6, "ab #ne cd\n"), 5, "inside an edit");
    }

    #[test]
    fn a_byte_order_mark_leaves_offsets_as_the_file_has_them() {
        let text = "\u{feff}Hello there.\n";
        let root = vault("bom", &[("main.typ", "#include \"ch.typ\"\n"), ("ch.typ", text)]);
        let on_disk = Compiler::new(&root).compile("main.typ").unwrap();
        let mut compiler = Compiler::new(&root);
        compiler.set_buffer("ch.typ", text).unwrap();
        for compilation in [on_disk, compiler.compile("main.typ").unwrap()] {
            let (page, y) = compilation.position_of("ch.typ", 4).unwrap();
            let Some(PageTarget::Source(path, offset)) = compilation.target_at(page, 75., y - 3.) else { panic!("no source there") };
            assert_eq!(path, "ch.typ");
            assert!(offset >= 3 && offset < text.len(), "{offset}");
            assert_eq!(compilation.words, [("ch.typ".to_string(), 2)]);
        }
    }

    /// Layout of nested content recurses far past the stack a pool thread has,
    /// so without `with_large_stack` this aborts the test process.
    #[test]
    fn a_deep_document_compiles_on_a_thread_with_a_small_stack() {
        const DEPTH: usize = 70;
        let nested = format!("{}deep{}", "#box[".repeat(DEPTH), "]".repeat(DEPTH));
        let root = vault("deep", &[("main.typ", &nested)]);
        let compile = move || Compiler::new(root).compile("main.typ").unwrap();
        let compilation = std::thread::Builder::new().stack_size(512 * 1024).spawn(compile).unwrap().join().unwrap();
        assert_eq!(errors(&compilation), Vec::<String>::new());
        assert_eq!(compilation.words, [("main.typ".to_string(), 1)]);
    }

    #[test]
    fn words_count_what_is_drawn_by_file() {
        let root = vault("words", &[
            ("main.typ", "#set page(numbering: \"1\")\n= Main heading\nOne two three. // not this\n#include \"ch.typ\"\n#let hidden = [nor this]\n"),
            ("ch.typ", "#strong[Four] five `six`\n"),
        ]);
        let compilation = Compiler::new(root).compile("main.typ").unwrap();
        assert!(errors(&compilation).is_empty());
        assert_eq!(compilation.words, [("main.typ".to_string(), 5), ("ch.typ".to_string(), 3)]);
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
