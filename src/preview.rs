//! Live preview: compiles on a background thread shortly after typing stops
//! and shows the pages as images, re-rendering only pages that changed.

use crate::theme::Theme;
use gpui::{prelude::FluentBuilder, *};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thesio_core::compiler::{Compiler, Diagnostic, render_changed};

const DEBOUNCE: Duration = Duration::from_millis(120);
/// Pixels per typographic point: sharp on a 2x screen at full page width.
const RENDER_SCALE: f32 = 2.;
const PAGE_GAP: f32 = 16.;
const PAD: f32 = 24.;

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
}

pub struct Preview {
    compiler: Arc<Mutex<Compiler>>,
    pages: Vec<Page>,
    diagnostics: Vec<Diagnostic>,
    generation: u64,
    scroll: ScrollHandle,
    width: Arc<Mutex<f32>>,
    open_path: String,
    /// The document to compile: the open file, or the one that includes it.
    main: String,
}

impl EventEmitter<PreviewEvent> for Preview {}

impl Preview {
    pub fn new(vault: std::path::PathBuf, path: String, main: String) -> Self {
        Self {
            compiler: Arc::new(Mutex::new(Compiler::new(vault))),
            pages: vec![],
            diagnostics: vec![],
            generation: 0,
            scroll: ScrollHandle::new(),
            width: Arc::new(Mutex::new(0.)),
            open_path: path,
            main,
        }
    }

    /// The open file moved; it recompiles on the next edit.
    pub fn set_path(&mut self, path: String, main: String) {
        self.open_path = path;
        self.main = main;
    }

    /// Compiles `text` as the open file, superseding any pending compile.
    pub fn update(&mut self, text: String, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let known: Vec<u128> = self.pages.iter().map(|p| p.hash).collect();
        let (compiler, path, main) = (self.compiler.clone(), self.open_path.clone(), self.main.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let stale = this.read_with(cx, |this, _| this.generation != generation).unwrap_or(true);
            if stale { return; }
            let result = cx.background_spawn(async move {
                let mut compiler = compiler.lock().unwrap();
                compiler.set_buffer(&path, text)?;
                let compilation = compiler.compile(&main)?;
                let pages = compilation.document.as_ref().map(|document| {
                    let sizes: Vec<(f32, f32)> = document.pages().iter().map(|p| (p.frame.width().to_pt() as f32, p.frame.height().to_pt() as f32)).collect();
                    let rendered = render_changed(document, RENDER_SCALE, &known).into_iter().map(|(hash, image)| {
                        (hash, image.map(|image| {
                            let mut buffer = image::RgbaImage::from_raw(image.width, image.height, image.rgba).expect("page size matches its pixels");
                            for pixel in buffer.pixels_mut() { pixel.0.swap(0, 2); }
                            Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
                        }))
                    }).collect::<Vec<_>>();
                    (rendered, sizes)
                });
                anyhow::Ok((pages, compilation.diagnostics))
            }).await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation { this.finish(result, cx); }
            });
        }).detach();
    }

    fn finish(&mut self, result: anyhow::Result<(Option<(Vec<(u128, Option<Arc<RenderImage>>)>, Vec<(f32, f32)>)>, Vec<Diagnostic>)>, cx: &mut Context<Self>) {
        let (pages, diagnostics) = match result {
            Ok(result) => result,
            Err(error) => (None, vec![Diagnostic { error: true, message: format!("{error:#}"), hints: vec![], path: None, range: None, line: None }]),
        };
        self.diagnostics = diagnostics;
        // A failed compile keeps the last good pages on screen.
        if let Some((rendered, sizes)) = pages {
            let mut old = std::mem::take(&mut self.pages);
            for ((hash, image), (width, height)) in rendered.into_iter().zip(sizes) {
                let image = image.or_else(|| old.iter().position(|p| p.hash == hash).map(|i| old.remove(i).image));
                if let Some(image) = image { self.pages.push(Page { hash, image, width, height }); }
            }
            for page in old { cx.drop_image(page.image, None); }
        }
        cx.notify();
    }
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = *self.width.lock().unwrap();
        let pages = self.pages.iter().map(|page| {
            let zoom = if measured > 0. { ((measured - 2. * PAD) / page.width).max(0.2) } else { 1. };
            img(page.image.clone()).flex_none().w(px(page.width * zoom)).h(px(page.height * zoom)).shadow_sm()
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
            .child(div().id("preview-pages").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.scroll)
                .flex().flex_col().items_center().gap(px(PAGE_GAP)).p(px(PAD)).children(pages))
            .when(!problems.is_empty(), |d| d.child(div().id("preview-problems").flex_none().max_h(px(160.)).overflow_y_scroll().py(px(6.)).bg(Theme::bg_body()).children(problems)))
            .child(canvas(move |bounds, _, cx| {
                let mut stored = width.lock().unwrap();
                if *stored != f32::from(bounds.size.width) {
                    *stored = bounds.size.width.into();
                    entity.update(cx, |_, cx| cx.notify());
                }
            }, |_, _, _, _| {}).absolute().size_full())
    }
}
