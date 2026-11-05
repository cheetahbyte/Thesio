//! Live preview: compiles on a background thread shortly after typing stops
//! and shows the pages as images, re-rendering only pages that changed.

use crate::theme::Theme;
use gpui::{prelude::FluentBuilder, *};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thesio_core::compiler::{Compiler, Diagnostic, render_changed};

const DEBOUNCE: Duration = Duration::from_millis(120);
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(100);
const FOLLOW_DELAY: Duration = Duration::from_millis(120);
/// A followed spot lands this far down the view, with text above it to read.
const FOLLOW_AT: f32 = 1. / 3.;
/// Closer than this to an edge counts as out of view.
const FOLLOW_MARGIN: f32 = 48.;
/// Pixels per typographic point: sharp on a 2x screen at full page width.
const RENDER_SCALE: f32 = 2.;
/// Pages of other files only need to read as pages, so they render small.
const BLUR_SCALE: f32 = 0.5;
const BLUR_SIGMA: f32 = 1.5;
const PAGE_GAP: f32 = 16.;
const PAD: f32 = 24.;

type CompileResult = anyhow::Result<(Option<(Vec<(u128, Option<Arc<RenderImage>>)>, Vec<(f32, f32, bool)>)>, Vec<Diagnostic>, Duration)>;

/// Compiles `text` as `path` within `main`, rendering pages not in `known`.
fn run(compiler: &mut Compiler, path: &str, text: String, main: &str, known: &[u128]) -> CompileResult {
    let started = Instant::now();
    compiler.set_buffer(path, text)?;
    let compilation = compiler.compile(main)?;
    crate::startup::mark("typst-done");
    let pages = compilation.document.as_ref().map(|document| {
        let own = compiler.pages_from(document, path);
        let sizes: Vec<(f32, f32, bool)> = document.pages().iter().zip(&own).map(|(p, own)| (p.frame.width().to_pt() as f32, p.frame.height().to_pt() as f32, *own)).collect();
        let scales: Vec<f32> = own.iter().map(|own| if *own { RENDER_SCALE } else { BLUR_SCALE }).collect();
        let rendered = render_changed(document, &scales, known).into_iter().zip(&own).map(|((hash, image), own)| {
            (hash, image.map(|image| {
                let mut buffer = image::RgbaImage::from_raw(image.width, image.height, image.rgba).expect("page size matches its pixels");
                if !*own { buffer = image::imageops::fast_blur(&buffer, BLUR_SIGMA); }
                for pixel in buffer.pixels_mut() { pixel.0.swap(0, 2); }
                Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
            }))
        }).collect::<Vec<_>>();
        (rendered, sizes)
    });
    anyhow::Ok((pages, compilation.diagnostics, started.elapsed()))
}

/// A launch compile: (path, main, text), its compiler, and its result.
type Prepared = ((String, String, String), Compiler, CompileResult);
static PREPARED: Mutex<Option<std::thread::JoinHandle<Option<Prepared>>>> = Mutex::new(None);

/// Starts compiling what launch will open, before the UI exists to ask for it.
pub fn prepare() {
    *PREPARED.lock().unwrap() = Some(std::thread::spawn(|| {
        let root = crate::store::Store::root_dir();
        let (path, main, text) = thesio_core::store::launch_target(&root)?;
        let mut compiler = Compiler::new(root);
        let result = run(&mut compiler, &path, text.clone(), &main, &[]);
        Some(((path, main, text), compiler, result))
    }));
}

struct Page {
    hash: u128,
    image: Arc<RenderImage>,
    /// Size in typographic points.
    width: f32,
    height: f32,
}

pub enum PreviewEvent {
    /// A diagnostic was clicked: jump to this byte offset of the open file.
    Jump(usize),
    /// A page was clicked: go to this file and byte offset.
    Source(String, usize),
}

pub struct Preview {
    compiler: Arc<Mutex<Compiler>>,
    pages: Vec<Page>,
    diagnostics: Vec<Diagnostic>,
    generation: u64,
    /// `generation` for the compile thread, which skips superseded work.
    latest: Arc<AtomicU64>,
    scroll: ScrollHandle,
    width: Arc<Mutex<f32>>,
    open_path: String,
    /// The document to compile: the open file, or the one that includes it.
    main: String,
    /// How long the last good compile took, rendering included.
    took: Option<Duration>,
    /// Waiting for the first compile to reach the screen.
    unsettled: bool,
    /// The page to scroll to once the pane has a width.
    reveal: Option<usize>,
    /// The editor's caret, which the preview stays near.
    follow: Option<usize>,
    /// Bumped by each caret move, so that only the last one scrolls.
    follow_generation: u64,
    /// Where to scroll once the pane is laid out: a page and a height in points.
    follow_target: Option<(usize, f32)>,
    first_compile: Option<Task<Option<CompileResult>>>,
}

impl EventEmitter<PreviewEvent> for Preview {}

impl Preview {
    pub fn new(vault: std::path::PathBuf, path: String, main: String) -> Self {
        Self {
            compiler: Arc::new(Mutex::new(Compiler::new(vault))),
            pages: vec![],
            diagnostics: vec![],
            generation: 0,
            latest: Default::default(),
            scroll: ScrollHandle::new(),
            width: Arc::new(Mutex::new(0.)),
            open_path: path,
            main,
            took: None,
            unsettled: false,
            reveal: None,
            follow: None,
            follow_generation: 0,
            follow_target: None,
            first_compile: None,
        }
    }

    /// The open file moved; it recompiles on the next edit.
    pub fn set_path(&mut self, path: String, main: String) {
        self.open_path = path;
        self.main = main;
    }

    /// Keeps the caret's place in view, shortly after it stops moving.
    pub fn follow(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.follow = Some(offset);
        self.locate(FOLLOW_DELAY, cx);
    }

    fn locate(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(offset) = self.follow else { return };
        self.follow_generation += 1;
        let generation = self.follow_generation;
        let (compiler, path) = (self.compiler.clone(), self.open_path.clone());
        let executor = cx.background_executor().clone();
        let work = cx.background_spawn(async move {
            executor.timer(delay).await;
            compiler.lock().unwrap().position_of(&path, offset)
        });
        cx.spawn(async move |this, cx| {
            let target = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.follow_generation != generation || target.is_none() { return; }
                this.follow_target = target;
                cx.notify();
            });
        }).detach();
    }

    /// Scrolls to a place in the document unless it is already in view.
    fn scroll_to_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let measured = *self.width.lock().unwrap();
        let view = f32::from(self.scroll.bounds().size.height);
        let Some((page, y)) = self.follow_target else { return };
        if self.pages.is_empty() { return; }
        if measured <= 0. || view <= 0. {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            return;
        }
        self.follow_target = None;
        let zoom = |page: &Page| ((measured - 2. * PAD) / page.width).max(0.2);
        let above: f32 = self.pages.iter().take(page).map(|page| page.height * zoom(page) + PAGE_GAP).sum();
        let target = PAD + above + y * self.pages.get(page).map_or(1., zoom);
        let top = -f32::from(self.scroll.offset().y);
        if target < top + FOLLOW_MARGIN || target > top + view - FOLLOW_MARGIN {
            self.scroll.set_offset(point(px(0.), px(-(target - view * FOLLOW_AT).max(0.))));
        }
    }

    /// Goes to the source of what was clicked on `page`.
    fn page_clicked(&mut self, page: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(bounds), Some(width)) = (self.scroll.bounds_for_item(page), self.pages.get(page).map(|page| page.width)) else { return };
        let origin = bounds.origin + self.scroll.offset();
        let zoom = f32::from(bounds.size.width) / width;
        let (x, y) = (f32::from(position.x - origin.x) / zoom, f32::from(position.y - origin.y) / zoom);
        let Some((path, offset)) = self.compiler.lock().unwrap().source_at(page, x, y) else { return };
        cx.emit(PreviewEvent::Source(path, offset));
    }

    /// Compiles `text` as the open file, superseding any pending compile.
    pub fn update(&mut self, text: String, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let first = generation == 1;
        let known: Vec<u128> = self.pages.iter().map(|p| p.hash).collect();
        let (compiler, path, main) = (self.compiler.clone(), self.open_path.clone(), self.main.clone());
        let latest = self.latest.clone();
        latest.store(generation, Ordering::Relaxed);
        let executor = cx.background_executor().clone();
        let prepared = if first { PREPARED.lock().unwrap().take() } else { None };
        // Off the main thread at once: at launch it would otherwise wait for the first frame.
        let work = cx.background_spawn(async move {
            // The first compile has no typing to wait out.
            if !first { executor.timer(DEBOUNCE).await; }
            if latest.load(Ordering::Relaxed) != generation { return None; }
            if let Some(handle) = prepared
                && let Ok(Some((target, ready, result))) = handle.join()
                && target == (path.clone(), main.clone(), text.clone()) {
                    *compiler.lock().unwrap() = ready;
                    return Some(result);
                }
            Some(run(&mut compiler.lock().unwrap(), &path, text, &main, &known))
        });
        if first {
            self.first_compile = Some(work);
        } else {
            self.first_compile = None;
            self.deliver(work, generation, cx);
        }
    }

    fn deliver(&mut self, work: impl Future<Output = Option<CompileResult>> + 'static, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let Some(result) = work.await else { return };
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation { this.finish(result, cx); }
            });
        }).detach();
    }

    /// The first frame holds a moment for the first compile: a blank preview
    /// that fills in one frame later looks slower than a slightly later frame.
    fn await_first_compile(&mut self, cx: &mut Context<Self>) {
        let Some(work) = self.first_compile.take() else { return };
        match cx.background_executor().block_with_timeout(FIRST_FRAME_WAIT, work) {
            Ok(Some(result)) => self.finish(result, cx),
            Ok(None) => {}
            Err(rest) => self.deliver(rest, self.generation, cx),
        }
    }

    fn finish(&mut self, result: CompileResult, cx: &mut Context<Self>) {
        let (pages, diagnostics, took) = match result {
            Ok(result) => result,
            Err(error) => (None, vec![Diagnostic { error: true, message: format!("{error:#}"), hints: vec![], path: None, range: None, line: None }], Duration::ZERO),
        };
        self.unsettled = true;
        self.diagnostics = diagnostics;
        self.took = pages.is_some().then_some(took);
        // A failed compile keeps the last good pages on screen.
        if let Some((rendered, sizes)) = pages {
            let mut old = std::mem::take(&mut self.pages);
            if old.is_empty() && self.follow.is_none() { self.reveal = Some(file_start(&sizes)); }
            for ((hash, image), (width, height, _)) in rendered.into_iter().zip(sizes) {
                let image = image.or_else(|| old.iter().position(|p| p.hash == hash).map(|i| old.remove(i).image));
                if let Some(image) = image { self.pages.push(Page { hash, image, width, height }); }
            }
            for page in old { cx.drop_image(page.image, None); }
            // Pages moved, and the caret's place may be on a page that just arrived.
            self.locate(Duration::ZERO, cx);
        }
        cx.notify();
    }
}

/// The first page of the longest run of the open file's pages. A contents
/// page lists every chapter, so the first match alone would often land there.
fn file_start(pages: &[(f32, f32, bool)]) -> usize {
    let (mut best, mut best_length, mut start) = (0, 0, 0);
    for (i, (.., own)) in pages.iter().enumerate() {
        if !*own { start = i + 1; }
        else if i + 1 - start > best_length { (best, best_length) = (start, i + 1 - start); }
    }
    best
}

fn format_took(took: Duration) -> String {
    let ms = took.as_millis();
    if ms < 1000 { format!("{ms} ms") } else { format!("{:.1} s", took.as_secs_f32()) }
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.await_first_compile(cx);
        if std::mem::take(&mut self.unsettled) { window.on_next_frame(|_, _| crate::startup::settled()); }
        let measured = *self.width.lock().unwrap();
        self.scroll_to_target(window, cx);
        if measured > 0. && let Some(page) = self.reveal.take() && page > 0 { self.scroll.scroll_to_top_of_item(page); }
        let pages = self.pages.iter().enumerate().map(|(i, page)| {
            let zoom = if measured > 0. { ((measured - 2. * PAD) / page.width).max(0.2) } else { 1. };
            div().id(("preview-page", i)).flex_none().w(px(page.width * zoom)).h(px(page.height * zoom)).shadow_sm()
                .child(img(page.image.clone()).size_full())
                .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| this.page_clicked(i, ev.position(), cx)))
        }).collect::<Vec<_>>();
        let width = self.width.clone();
        let entity = cx.entity();
        let problems = self.diagnostics.iter().enumerate().map(|(i, d)| {
            let target = d.range.as_ref().filter(|_| d.path.as_deref() == Some(self.open_path.as_str())).map(|r| r.start);
            let location = d.path.as_ref().map(|p| match d.line { Some(line) => format!("{p}:{}  ", line + 1), None => format!("{p}  ") }).unwrap_or_default();
            div().id(("diagnostic", i)).px(px(12.)).py(px(4.)).text_size(px(12.))
                .text_color(if d.error { Theme::danger() } else { Theme::text_caption() })
                .when(target.is_some(), |row| row.cursor_pointer())
                .child(format!("{location}{}", d.message))
                .on_click(cx.listener(move |_, _, _, cx| if let Some(offset) = target { cx.emit(PreviewEvent::Jump(offset)); }))
        }).collect::<Vec<_>>();
        div().relative().size_full().flex().flex_col().bg(Theme::sidebar_hover())
            .child(div().relative().flex().flex_col().flex_1().min_h_0()
                .child(div().id("preview-pages").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.scroll)
                    .flex().flex_col().items_center().gap(px(PAGE_GAP)).p(px(PAD)).children(pages))
                .children(self.took.map(|took| div().absolute().right(px(12.)).bottom(px(8.)).text_size(px(11.)).line_height(px(14.)).text_color(Theme::text_caption()).child(format_took(took)))))
            .when(!problems.is_empty(), |d| d.child(div().id("preview-problems").flex_none().max_h(px(160.)).overflow_y_scroll().py(px(6.)).bg(Theme::bg_body()).children(problems)))
            .child(canvas(move |bounds, _, cx| {
                let mut stored = width.lock().unwrap();
                if *stored != f32::from(bounds.size.width) {
                    *stored = bounds.size.width.into();
                    // A notify during drawing is lost, and the pages would wait for the next event.
                    let entity = entity.clone();
                    cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
                }
            }, |_, _, _, _| {}).absolute().size_full())
    }
}
