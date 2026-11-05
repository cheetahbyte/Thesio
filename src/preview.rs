//! Live preview: draws the pages of a `DocumentSession` and its diagnostics,
//! and keeps the editor's caret in view.

use crate::session::{DocumentSession, Page, SessionEvent};
use crate::theme::Theme;
use crate::ui::format_count;
use gpui::{prelude::FluentBuilder, *};
use std::{cell::Cell, rc::Rc};
use std::time::Duration;
use thesio_core::compiler::PageTarget;

const FOLLOW_DELAY: Duration = Duration::from_millis(120);
/// A followed spot lands this far down the view, with text above it to read.
const FOLLOW_AT: f32 = 1. / 3.;
/// Closer than this to an edge counts as out of view.
const FOLLOW_MARGIN: f32 = 48.;
const PAGE_GAP: f32 = 16.;
const PAD: f32 = 24.;

/// Where a page sits in the scrolling column, and at what zoom.
struct Placed {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    zoom: f32,
}

/// Lays `pages` out in a pane `pane` wide, or at full size before it is measured.
fn layout(pages: &[Page], pane: f32) -> Vec<Placed> {
    let mut top = PAD;
    pages.iter().map(|page| {
        let zoom = if pane > 0. { ((pane - 2. * PAD) / page.width).max(0.2) } else { 1. };
        let (width, height) = (page.width * zoom, page.height * zoom);
        let placed = Placed { left: (pane - width) / 2., top, width, height, zoom };
        top += height + PAGE_GAP;
        placed
    }).collect()
}

pub enum PreviewEvent {
    /// A diagnostic was clicked: jump to this byte offset of the open file.
    Jump(usize),
    /// A page was clicked: go to this file and byte offset.
    Source(String, usize),
    /// A note link was clicked: open this file.
    Note(String),
}

pub struct Preview {
    session: Entity<DocumentSession>,
    /// The last result of `session` taken in.
    seen: u64,
    scroll: ScrollHandle,
    width: Rc<Cell<f32>>,
    /// Waiting for the first compile to reach the screen.
    unsettled: bool,
    /// The first pages have arrived.
    opened: bool,
    /// The page to scroll to once the pane has a width.
    reveal: Option<usize>,
    /// The editor's caret, which the preview stays near.
    follow: Option<usize>,
    /// Finds the caret's place. A later move replaces it.
    locating: Option<Task<()>>,
    /// Where to scroll once the pane is laid out: a page and a height in points.
    follow_target: Option<(usize, f32)>,
    _subscription: Subscription,
}

impl EventEmitter<PreviewEvent> for Preview {}

impl Preview {
    pub fn new(session: Entity<DocumentSession>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&session, |this, _, event: &SessionEvent, cx| match event {
            SessionEvent::Compiled => this.take_result(cx),
            SessionEvent::Diagnostics => cx.notify(),
        });
        Self { session, seen: 0, scroll: ScrollHandle::new(), width: Default::default(), unsettled: false, opened: false, reveal: None, follow: None, locating: None, follow_target: None, _subscription: subscription }
    }

    /// Keeps the caret's place in view, shortly after it stops moving.
    pub fn follow(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.follow = Some(offset);
        self.locate(FOLLOW_DELAY, cx);
    }

    fn locate(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(offset) = self.follow else { return };
        let session = self.session.read(cx);
        let (compilation, path) = (session.document().cloned(), session.path().to_string());
        let executor = cx.background_executor().clone();
        let work = cx.background_spawn(async move {
            executor.timer(delay).await;
            compilation?.position_of(&path, offset)
        });
        self.locating = Some(cx.spawn(async move |this, cx| {
            let Some(target) = work.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.follow_target = Some(target);
                cx.notify();
            });
        }));
    }

    /// Scrolls to a place in the document unless it is already in view.
    fn scroll_to_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let measured = self.width.get();
        let view = f32::from(self.scroll.bounds().size.height);
        let Some((page, y)) = self.follow_target else { return };
        let pages = self.session.read(cx).pages();
        if pages.is_empty() { return; }
        if measured <= 0. || view <= 0. {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            return;
        }
        self.follow_target = None;
        let Some(placed) = layout(&pages, measured).into_iter().nth(page) else { return };
        let target = placed.top + y * placed.zoom;
        let top = -f32::from(self.scroll.offset().y);
        if target < top + FOLLOW_MARGIN || target > top + view - FOLLOW_MARGIN {
            self.scroll.set_offset(point(px(0.), px(-(target - view * FOLLOW_AT).max(0.))));
        }
    }

    /// Goes to the source of what was clicked on `page`.
    fn page_clicked(&mut self, page: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let Some(placed) = layout(&session.pages(), self.width.get()).into_iter().nth(page) else { return };
        let view = self.scroll.bounds().origin;
        let (left, top) = (view.x + px(placed.left), view.y + self.scroll.offset().y + px(placed.top));
        let (x, y) = (f32::from(position.x - left) / placed.zoom, f32::from(position.y - top) / placed.zoom);
        match session.document().and_then(|compilation| compilation.target_at(page, x, y)) {
            Some(PageTarget::Source(path, offset)) => cx.emit(PreviewEvent::Source(path, offset)),
            Some(PageTarget::Note(path)) => cx.emit(PreviewEvent::Note(path)),
            None => {}
        }
    }

    /// Follows a newly published result. Runs from the session's event, and
    /// from the first frame, which can hold the result before the event arrives.
    fn take_result(&mut self, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        if session.published() == self.seen { return; }
        self.seen = session.published();
        self.unsettled = true;
        let compiled = session.took().is_some();
        let pages = session.pages();
        if !self.opened && !pages.is_empty() {
            self.opened = true;
            if self.follow.is_none() { self.reveal = Some(file_start(&pages)); }
        }
        // Pages moved, and the caret's place may be on a page that just arrived.
        if compiled { self.locate(Duration::ZERO, cx); }
        cx.notify();
    }
}

/// The first page of the longest run of the open file's pages. A contents
/// page lists every chapter, so the first match alone would often land there.
fn file_start(pages: &[Page]) -> usize {
    let (mut best, mut best_length, mut start) = (0, 0, 0);
    for (i, page) in pages.iter().enumerate() {
        if !page.own { start = i + 1; }
        else if i + 1 - start > best_length { (best, best_length) = (start, i + 1 - start); }
    }
    best
}

fn format_took(took: Duration) -> String {
    let ms = took.as_millis();
    if ms < 1000 { format!("{ms} ms") } else { format!("{:.1} s", took.as_secs_f32()) }
}

impl Preview {
    /// The corner's text: the open file's words of the whole, and the compile time.
    fn status(&self, took: Duration, cx: &App) -> String {
        let session = self.session.read(cx);
        let total: usize = session.words().iter().map(|(_, words)| words).sum();
        let own = session.words().iter().find(|(path, _)| path == session.path()).map(|(_, words)| *words);
        let words = match own {
            Some(own) if own != total => format!("{} of {} words", format_count(own), format_count(total)),
            _ if total == 1 => "1 word".to_string(),
            _ => format!("{} words", format_count(total)),
        };
        format!("{words} · {}", format_took(took))
    }
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.session.update(cx, |session, cx| session.hold_first_frame(cx));
        self.take_result(cx);
        if std::mem::take(&mut self.unsettled) { window.on_next_frame(|_, _| crate::startup::settled()); }
        let measured = self.width.get();
        self.scroll_to_target(window, cx);
        if measured > 0. && let Some(page) = self.reveal.take() && page > 0 { self.scroll.scroll_to_top_of_item(page); }
        let session = self.session.read(cx);
        let (pages, diagnostics, open_path, took) = (session.pages(), session.diagnostics(), session.path().to_string(), session.took());
        let status = took.map(|took| self.status(took, cx));
        let placed = layout(&pages, measured);
        let pages = pages.iter().zip(&placed).enumerate().map(|(i, (page, placed))| {
            div().id(("preview-page", i)).flex_none().w(px(placed.width)).h(px(placed.height)).shadow_sm()
                .child(img(page.image.clone()).size_full())
                .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| this.page_clicked(i, ev.position(), cx)))
        }).collect::<Vec<_>>();
        let width = self.width.clone();
        let entity = cx.entity();
        let problems = diagnostics.iter().enumerate().map(|(i, d)| {
            let target = d.range.as_ref().filter(|_| d.path.as_deref() == Some(open_path.as_str())).map(|r| r.start);
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
                .children(status.map(|status| div().absolute().right(px(12.)).bottom(px(8.)).text_size(px(11.)).line_height(px(14.)).text_color(Theme::text_caption()).child(status))))
            .when(!problems.is_empty(), |d| d.child(div().id("preview-problems").flex_none().max_h(px(160.)).overflow_y_scroll().py(px(6.)).bg(Theme::bg_body()).children(problems)))
            .child(canvas(move |bounds, _, cx| {
                if width.get() != f32::from(bounds.size.width) {
                    width.set(bounds.size.width.into());
                    // A notify during drawing is lost, and the pages would wait for the next event.
                    let entity = entity.clone();
                    cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
                }
            }, |_, _, _, _| {}).absolute().size_full())
    }
}
