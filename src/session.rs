//! One document's compiles, apart from anything that shows them: the compiler,
//! when a compile runs, which result counts, and the handoff from launch. The
//! newest result is published here, so a view or an editor reads it without
//! touching the compiler, and nothing on the main thread waits for one to
//! answer a click.

use gpui::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thesio_core::compiler::{Compilation, Compiler, Diagnostic, render_changed};
use thesio_core::project::{LaunchTarget, launch_target};

const DEBOUNCE: Duration = Duration::from_millis(120);
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(100);
/// Pixels per typographic point: sharp on a 2x screen at full page width.
const RENDER_SCALE: f32 = 2.;
/// Pages of other files only need to read as pages, so they render small.
const BLUR_SCALE: f32 = 0.5;
const BLUR_SIGMA: f32 = 1.5;

/// A page of the document, ready to draw.
#[derive(Clone)]
pub struct Page {
    hash: u128,
    pub image: Arc<RenderImage>,
    /// Size in typographic points.
    pub width: f32,
    pub height: f32,
    /// It shows the open file, or a file that one includes.
    pub own: bool,
}

/// A compile's page, whose image is `None` when the session already has it.
struct RenderedPage {
    hash: u128,
    image: Option<Arc<RenderImage>>,
    width: f32,
    height: f32,
    own: bool,
}

struct Output {
    compilation: Arc<Compilation>,
    pages: Option<Vec<RenderedPage>>,
    took: Duration,
}

type Outcome = anyhow::Result<Output>;

/// Compiles `text` as `path` within `main`. With `known`, it also renders the
/// pages whose hashes are not in it.
fn run(compiler: &mut Compiler, path: &str, text: String, main: &str, known: Option<&[u128]>) -> Outcome {
    let started = Instant::now();
    compiler.set_buffer(path, text)?;
    let compilation = Arc::new(compiler.compile(main)?);
    crate::startup::mark("typst-done");
    let pages = match (&compilation.document, known) {
        (Some(document), Some(known)) => {
            let own = compilation.pages_from(path);
            let changed = render_changed(document, known, |page| if own[page] { RENDER_SCALE } else { BLUR_SCALE })?;
            Some(changed.into_iter().zip(document.pages()).zip(&own).map(|(((hash, image), page), own)| RenderedPage {
                hash,
                image: image.map(|image| {
                    let mut buffer = image::RgbaImage::from_raw(image.width, image.height, image.rgba).expect("page size matches its pixels");
                    if !*own { buffer = image::imageops::fast_blur(&buffer, BLUR_SIGMA); }
                    for pixel in buffer.pixels_mut() { pixel.0.swap(0, 2); }
                    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
                }),
                width: page.frame.width().to_pt() as f32,
                height: page.frame.height().to_pt() as f32,
                own: *own,
            }).collect())
        }
        _ => None,
    };
    Ok(Output { compilation, pages, took: started.elapsed() })
}

/// A launch compile: what it guessed, its compiler, and its result.
type Prepared = (LaunchTarget, Compiler, Outcome);
static PREPARED: Mutex<Option<std::thread::JoinHandle<Option<Prepared>>>> = Mutex::new(None);

/// Starts compiling what launch will open, before the UI exists to ask for it.
pub fn prepare() {
    *PREPARED.lock().unwrap() = Some(std::thread::spawn(|| {
        let root = crate::store::Store::root_dir();
        let target = launch_target(&root)?;
        let mut compiler = Compiler::new(root);
        let result = run(&mut compiler, &target.document, target.text.clone(), &target.main, Some(&[]));
        Some((target, compiler, result))
    }));
}

/// Pairs each page with its image, taking those the last compile left unchanged
/// from `old`, and returns the images no page uses any more. Identical pages
/// hash alike, so one image can serve several pages.
fn reuse_images<I: Clone>(rendered: Vec<(u128, Option<I>)>, old: Vec<(u128, I)>) -> (Vec<(u128, I)>, Vec<I>) {
    let mut unused: HashMap<u128, I> = old.into_iter().collect();
    let pages: Vec<(u128, I)> = rendered.into_iter().filter_map(|(hash, image)| Some((hash, image.or_else(|| unused.get(&hash).cloned())?))).collect();
    for (hash, _) in &pages { unused.remove(hash); }
    (pages, unused.into_values().collect())
}

pub enum SessionEvent {
    /// A compile finished; the published result changed.
    Compiled,
    /// The new result's diagnostics differ from the last one's.
    Diagnostics,
}

pub struct DocumentSession {
    root: PathBuf,
    /// Made on first use, off the main thread, since launch may bring its own.
    compiler: Arc<Mutex<Option<Compiler>>>,
    path: String,
    /// The document to compile: `path`, or the one that includes it.
    main: String,
    /// Rendering pages is for a view; an editor alone only needs the result.
    rendered: bool,
    /// Counts `set_text` calls. A compile that finds it moved on does nothing.
    latest: Arc<AtomicU64>,
    /// The first compile, until a frame takes it or the wait is over.
    first: Option<Task<Option<Outcome>>>,
    /// Waits for the newest compile. Replacing it drops any earlier one.
    delivery: Option<Task<()>>,
    /// The newest result, with or without a document.
    newest: Option<Arc<Compilation>>,
    /// The last result with a document, which the pages and word counts are from.
    document: Option<Arc<Compilation>>,
    pages: Rc<Vec<Page>>,
    diagnostics: Rc<Vec<Diagnostic>>,
    /// How long the newest compile took, rendering included, if it gave a document.
    took: Option<Duration>,
    /// Counts published results.
    published: u64,
}

impl EventEmitter<SessionEvent> for DocumentSession {}

impl DocumentSession {
    /// A session whose results carry pages, for a `Preview`.
    pub fn with_pages(root: PathBuf, path: String, main: String) -> Self {
        Self { rendered: true, ..Self::without_pages(root, path, main) }
    }

    /// A session for an editor with no `Preview`, such as paper notes: the
    /// document compiles and nothing is rendered.
    pub fn without_pages(root: PathBuf, path: String, main: String) -> Self {
        Self { root, compiler: Default::default(), path, main, rendered: false, latest: Default::default(), first: None, delivery: None, newest: None, document: None, pages: Default::default(), diagnostics: Default::default(), took: None, published: 0 }
    }

    pub fn path(&self) -> &str { &self.path }
    pub fn pages(&self) -> Rc<Vec<Page>> { self.pages.clone() }
    pub fn diagnostics(&self) -> Rc<Vec<Diagnostic>> { self.diagnostics.clone() }
    pub fn took(&self) -> Option<Duration> { self.took }
    pub fn published(&self) -> u64 { self.published }
    /// The newest result, which has no document if that compile failed.
    pub fn newest(&self) -> Option<&Arc<Compilation>> { self.newest.as_ref() }
    /// The last result with a document, for moving between pages and source.
    pub fn document(&self) -> Option<&Arc<Compilation>> { self.document.as_ref() }
    /// The document's words by file, as of the last good compile.
    pub fn words(&self) -> &[(String, usize)] { self.document.as_ref().map_or(&[], |document| &document.words) }

    /// The open file moved; it recompiles on the next edit.
    /// The document this file compiles within.
    pub fn main(&self) -> &str { &self.main }

    pub fn set_path(&mut self, path: String, main: String) {
        self.path = path;
        self.main = main;
    }

    /// Compiles `text` as the open file, superseding any pending compile.
    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        let generation = self.latest.fetch_add(1, Ordering::Relaxed) + 1;
        let held = self.rendered && generation == 1;
        let known: Option<Vec<u128>> = self.rendered.then(|| self.pages.iter().map(|page| page.hash).collect());
        let (compiler, latest, root, path, main) = (self.compiler.clone(), self.latest.clone(), self.root.clone(), self.path.clone(), self.main.clone());
        let executor = cx.background_executor().clone();
        let prepared = if held { PREPARED.lock().unwrap().take() } else { None };
        // Off the main thread at once: at launch it would otherwise wait for the first frame.
        let work = cx.background_spawn(async move {
            // The first compile has no typing to wait out.
            if !held { executor.timer(DEBOUNCE).await; }
            let mut slot = compiler.lock().unwrap();
            if latest.load(Ordering::Relaxed) != generation { return None; }
            if let Some(handle) = prepared
                && let Ok(Some((target, ready, result))) = handle.join()
                && (target.document == path && target.main == main && target.text == text) {
                    *slot = Some(ready);
                    return Some(result);
                }
            Some(run(slot.get_or_insert_with(|| Compiler::new(root)), &path, text, &main, known.as_deref()))
        });
        self.first = None;
        if held {
            self.first = Some(work);
            // No frame may come for it, when the document opens behind another view.
            self.delivery = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FIRST_FRAME_WAIT).await;
                let _ = this.update(cx, |this, cx| if let Some(work) = this.first.take() { this.deliver(work, cx); });
            }));
        } else {
            self.deliver(work, cx);
        }
    }

    fn deliver(&mut self, work: impl Future<Output = Option<Outcome>> + 'static, cx: &mut Context<Self>) {
        self.delivery = Some(cx.spawn(async move |this, cx| {
            let Some(outcome) = work.await else { return };
            let _ = this.update(cx, |this, cx| this.publish(outcome, cx));
        }));
    }

    /// The first frame holds a moment for the first compile: a blank preview
    /// that fills in one frame later looks slower than a slightly later frame.
    pub fn hold_first_frame(&mut self, cx: &mut Context<Self>) {
        let Some(work) = self.first.take() else { return };
        match cx.background_executor().block_with_timeout(FIRST_FRAME_WAIT, work) {
            Ok(Some(outcome)) => self.publish(outcome, cx),
            Ok(None) => {}
            Err(rest) => self.deliver(rest, cx),
        }
    }

    fn publish(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        let (diagnostics, output) = match outcome {
            Ok(output) => (output.compilation.diagnostics.clone(), Some(output)),
            Err(error) => (vec![Diagnostic { error: true, message: format!("{error:#}"), hints: vec![], path: None, range: None, line: None }], None),
        };
        let diagnostics_changed = *self.diagnostics != diagnostics;
        self.diagnostics = Rc::new(diagnostics);
        self.took = None;
        self.published += 1;
        if let Some(Output { compilation, pages, took }) = output {
            self.newest = Some(compilation.clone());
            // A failed compile keeps the last good pages on screen.
            if compilation.document.is_some() {
                self.document = Some(compilation);
                self.took = Some(took);
            }
            if let Some(pages) = pages { self.place_pages(pages, cx); }
        }
        cx.emit(SessionEvent::Compiled);
        if diagnostics_changed { cx.emit(SessionEvent::Diagnostics); }
        cx.notify();
    }

    fn place_pages(&mut self, rendered: Vec<RenderedPage>, cx: &mut Context<Self>) {
        let sizes: HashMap<u128, (f32, f32, bool)> = rendered.iter().map(|page| (page.hash, (page.width, page.height, page.own))).collect();
        let old = self.pages.iter().map(|page| (page.hash, page.image.clone())).collect();
        let (pages, unused) = reuse_images(rendered.into_iter().map(|page| (page.hash, page.image)).collect(), old);
        for image in unused { cx.drop_image(image, None); }
        self.pages = Rc::new(pages.into_iter().map(|(hash, image)| {
            let (width, height, own) = sizes[&hash];
            Page { hash, image, width, height, own }
        }).collect());
    }
}

#[cfg(test)]
mod tests {
    use super::reuse_images;

    #[test]
    fn identical_pages_share_an_image_when_their_number_grows() {
        let (pages, unused) = reuse_images(vec![(1, None), (1, None), (2, Some("new"))], vec![(1, "blank")]);
        assert_eq!(pages, [(1, "blank"), (1, "blank"), (2, "new")]);
        assert!(unused.is_empty());
    }

    #[test]
    fn an_image_no_page_uses_is_handed_back() {
        let (pages, unused) = reuse_images(vec![(2, None)], vec![(1, "gone"), (2, "kept")]);
        assert_eq!(pages, [(2, "kept")]);
        assert_eq!(unused, ["gone"]);
    }
}
