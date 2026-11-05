//! Compiles Typst documents from a vault: a plain folder whose root is the
//! Typst project root (paths starting with `/` also try the main file's
//! folders; see `project::candidates`), so `#include`, `#image`, and `#bibliography` reach any
//! file in it. Unsaved editor text is held in buffers that shadow the disk.

use crate::project::{self, Project};
use anyhow::{Context, Result};
use rustc_hash::{FxHashMap, FxHashSet};
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::introspection::PagedPosition;
use typst::layout::{Abs, Frame, FrameItem, Point};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
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

/// The outcome of one compile, whole: it holds the files it read, so it needs
/// no compiler to answer questions and any thread can keep it. A later edit
/// does not change what it says. Autocomplete and hover can run against
/// `world`, which is what `typst_ide` asks for.
pub struct Compilation {
    pub main: String,
    /// The last pass's document; `None` when compilation failed.
    pub document: Option<PagedDocument>,
    pub diagnostics: Vec<Diagnostic>,
    /// The document's words by vault file, in the order the files first
    /// appear. Text with no file, such as a bibliography or page numbers, does
    /// not count.
    pub words: Vec<(String, usize)>,
    world: Snapshot,
}

impl Compilation {
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
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        let resources = Arc::new(Resources { library: LazyHash::new(Library::default()), fonts });
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

    fn run(&self, main: &str, id: FileId) -> Compilation {
        let world = VaultWorld { compiler: self, main: id, read: Mutex::default() };
        let result = typst::compile::<PagedDocument>(&world);
        let (document, errors) = match result.output {
            Ok(document) => (Some(document), Default::default()),
            Err(errors) => (None, errors),
        };
        let diagnostics = errors.iter().chain(&result.warnings).map(|d| world.diagnostic(d)).collect();
        comemo::evict(CACHE_AGE);
        let words = document.as_ref().map_or(vec![], count_words);
        let files = world.read.into_inner().unwrap_or_default();
        Compilation { main: main.to_string(), document, diagnostics, words, world: Snapshot { resources: self.resources.clone(), main: id, files } }
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

/// Runs `work` on a thread with room for deep documents and embedded PDFs,
/// whose colour conversion overflows a pool thread's 512 KB in a debug build.
fn with_large_stack<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T> {
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
}

impl VaultWorld<'_> {
    fn resolved(&self, id: FileId) -> FileId {
        resolved(self.main, id, |found, path| self.compiler.buffers.contains_key(&found) || self.compiler.root.join(path).is_file())
    }

    fn path(&self, id: FileId) -> FileResult<PathBuf> {
        let rooted = id.get();
        // Packages download from the network; the vault stays offline for now.
        if !matches!(rooted.root(), VirtualRoot::Project) {
            return Err(FileError::Other(Some("Typst packages are not supported yet".into())));
        }
        rooted.vpath().realize(&self.compiler.root).map_err(FileError::Realize)
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
struct Snapshot {
    resources: Arc<Resources>,
    main: FileId,
    files: FxHashMap<FileId, ReadFile>,
}

impl Snapshot {
    fn resolved(&self, id: FileId) -> FileId { resolved(self.main, id, |found, _| self.files.contains_key(&found)) }

    /// A file's text, exactly at `path`.
    fn text(&self, path: &str) -> Option<String> {
        let source = self.files.get(&file_id(path).ok()?)?.source.as_ref()?;
        Some(source.text().to_string())
    }

    fn missing(id: FileId) -> FileError { FileError::NotFound(id.get().vpath().get_without_slash().into()) }
}

impl IdeWorld for Snapshot {
    fn upcast(&self) -> &dyn World { self }
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

    fn chapters() -> Compilation {
        let page_of_text = "Word ".repeat(600);
        let root = vault("jump", &[
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
        let compilation = chapters();
        let (page, y) = compilation.position_of("chapters/two.typ", 3).unwrap();
        assert_eq!(page, 1);
        let Some(PageTarget::Source(path, offset)) = compilation.target_at(page, 75., y - 3.) else { panic!("no source there") };
        assert_eq!(path, "chapters/two.typ");
        assert!(offset < "Second chapter.".len(), "{offset}");
    }

    #[test]
    fn blank_lines_take_the_place_of_the_text_before_them() {
        assert_eq!(chapters().position_of("chapters/two.typ", 17).map(|(page, _)| page), Some(1));
    }

    #[test]
    fn a_heading_marker_takes_the_place_of_the_heading() {
        assert_eq!(chapters().position_of("chapters/three.typ", 0).map(|(page, _)| page), Some(2));
    }

    #[test]
    fn a_click_on_a_margin_lands_on_nothing() {
        assert_eq!(chapters().target_at(1, 1., 1.), None);
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

    #[test]
    fn a_package_import_is_an_error_not_a_panic() {
        let root = vault("package", &[("main.typ", "#import \"@preview/x:1.0.0\": y\n")]);
        assert!(!errors(&Compiler::new(root).compile("main.typ").unwrap()).is_empty());
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
