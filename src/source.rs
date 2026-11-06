//! The Typst source editor: one virtualized list row per logical line, soft
//! wrapped, with the selection and caret painted by each row.

use crate::completion::{Completion, CompletionKind, Found, Item, Provider, Query, Reach};
use crate::session::{DocumentSession, SessionEvent};
use crate::text::{CaretMark, LineMarks, PaintedLine, Shaped, from_utf16, paint_line, plain_newlines, range_from_utf16, range_to_utf16, request_line, to_utf16};
use crate::theme::{self, MONO_FONT, Theme, c, craft};
use crate::ui::{MENU_PAD_Y, menu_row, popover, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc, time::Duration};
use thesio_core::buffer::{Buffer, Change, EditKind, difference, follow, stretch};
use thesio_core::bibliography;
use thesio_core::ide::{Hover, word_around};
use thesio_core::images::{Incoming, figure_place, figures};
use thesio_core::quote::block_glue;
use thesio_core::syntax::{SyntaxTag, highlights};

const FONT_SIZE: f32 = 14.;
const LINE_HEIGHT: f32 = 22.;
const TYPING_MARGIN: f32 = LINE_HEIGHT * 2.;
/// Room after the last line, so the document's end can have that margin too.
const END_PADDING: f32 = LINE_HEIGHT * 4.;
const PAD_X: f32 = 24.;

actions!(
    source_editor,
    [Backspace, Delete, DeleteWordLeft, DeleteWordRight, Left, Right, Up, Down, WordLeft, WordRight, LineStart, LineEnd,
     DocStart, DocEnd, SelectLeft, SelectRight, SelectUp, SelectDown, SelectWordLeft, SelectWordRight, SelectLineStart,
     SelectLineEnd, SelectAll, Copy, Cut, Paste, Undo, Redo, Enter, Tab, CompletionUp, CompletionDown, CompletionAccept,
     CompletionDismiss, CompletionTrigger, HoverDismiss]
);

pub fn bind_keys(cx: &mut App) {
    let c = Some("SourceEditor");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("alt-backspace", DeleteWordLeft, c),
        KeyBinding::new("alt-delete", DeleteWordRight, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("cmd-left", LineStart, c),
        KeyBinding::new("cmd-right", LineEnd, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("cmd-up", DocStart, c),
        KeyBinding::new("cmd-down", DocEnd, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("cmd-shift-left", SelectLineStart, c),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("cmd-shift-z", Redo, c),
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("tab", Tab, c),
        KeyBinding::new("ctrl-space", CompletionTrigger, c),
        KeyBinding::new("up", CompletionUp, COMPLETING),
        KeyBinding::new("down", CompletionDown, COMPLETING),
        KeyBinding::new("enter", CompletionAccept, COMPLETING),
        KeyBinding::new("tab", CompletionAccept, COMPLETING),
        KeyBinding::new("escape", CompletionDismiss, COMPLETING),
        KeyBinding::new("escape", HoverDismiss, Some("SourceEditor && hovering")),
    ]);
}

const COMPLETING: Option<&str> = Some("SourceEditor && completing");
/// Rows the popup shows at once; the rest scroll.
const COMPLETION_ROWS: usize = 6;
const COMPLETION_ROW_H: f32 = 42.;
const HOVER_DELAY: Duration = Duration::from_millis(350);
const HOVER_MAX_H: f32 = 220.;

pub enum SourceEvent {
    Changed,
    /// The caret moved, by typing or otherwise.
    Caret,
    /// ⌘-click at this byte offset.
    Follow(usize),
    /// A completion was accepted.
    Completed(CompletionKind),
    /// Images were dropped or pasted. The host copies them and answers with `insert_figures`.
    Images(Vec<Incoming>),
}

/// What asked the popup to follow the caret. Only typing or asking opens it,
/// so moving through existing citations stays quiet.
#[derive(Clone, Copy, PartialEq)]
enum Trigger {
    Moved,
    Typed,
    Asked,
}

/// An underline over bytes of the text, and what it means. It follows edits
/// and resizes with one inside it, so typing in an error keeps it until the
/// next compile; an edit across its edge ends it.
#[derive(Clone, PartialEq)]
pub struct Decoration {
    pub range: Range<usize>,
    pub error: bool,
    pub message: String,
    pub hints: Vec<String>,
}

impl Decoration {
    fn color(&self) -> Hsla { if self.error { Theme::danger() } else { Theme::caution() } }
    fn meets(&self, range: &Range<usize>) -> bool { self.range.start < range.end && self.range.end > range.start }
}

/// The lines of the last frame, kept whole until the next one paints, so
/// anything may read them whatever the order within a frame.
#[derive(Default)]
struct Rows {
    lines: HashMap<usize, PaintedLine>,
    /// Frames rendered, to tell whether `lines` has seen a request yet.
    frame: u64,
    outdated: bool,
}

impl Rows {
    fn begin_frame(&mut self) {
        self.frame += 1;
        self.outdated = true;
    }

    fn insert(&mut self, line: usize, painted: PaintedLine) {
        if std::mem::take(&mut self.outdated) { self.lines.clear(); }
        self.lines.insert(line, painted);
    }
}

type Painted = Rc<RefCell<Rows>>;

/// The generated bibliography, folded into one row.
struct Fold {
    bytes: Range<usize>,
    lines: Range<usize>,
    /// It ends the document, so nothing can be typed below it.
    at_end: bool,
}

impl Fold {
    fn holds(&self, offset: usize) -> bool {
        offset >= self.bytes.start && (offset < self.bytes.end || self.at_end)
    }

    /// `offset` moved out of the block, the way a caret at `from` was heading.
    fn outside(&self, offset: usize, from: usize) -> usize {
        if !self.holds(offset) { return offset; }
        if self.at_end || offset < from { self.bytes.start.saturating_sub(1) } else { self.bytes.end }
    }

    /// The line a move by one line reaches, given it would reach `line`.
    fn past(&self, line: usize, down: bool) -> usize {
        if !self.lines.contains(&line) { return line; }
        if down { self.lines.end } else { self.lines.start.wrapping_sub(1) }
    }
}

/// Where a caret move leaves the selection.
enum Place {
    At(usize),
    /// The caret goes here and the anchor stays.
    Extend(usize),
    Select(Range<usize>),
}

/// How far the list has been moved towards a row that was not painted.
enum Jump {
    Unmoved,
    /// To the nearest edge, by the heights the list has measured.
    Near,
    /// To the row itself: rows on the way may be unmeasured.
    Row,
}

/// A request to bring the caret into view, resolved once its row is painted.
struct Reveal {
    /// Room to keep below the caret, so typing never runs along the edge.
    margin: f32,
    jump: Jump,
    /// The frame the request was made in. Rows of that frame or earlier do
    /// not show the text it was made for.
    frame: u64,
}

/// What the popover says about what is under the pointer.
enum Said {
    Docs(Hover),
    /// Most serious first.
    Problems(Vec<Decoration>),
}

/// The word or underline under the pointer, and what is known about it once
/// that arrives.
struct Pointed {
    word: Range<usize>,
    /// The start of the part the pointer's line shows, for the popover.
    anchor: usize,
    /// It is an underline, which is said before anything about the word.
    problem: bool,
    shown: Option<Said>,
    /// Waits out the delay, then asks. Replacing it drops the question.
    _pending: Option<Task<()>>,
}

pub struct SourceEditor {
    buffer: Buffer,
    /// Highlight as Typst; other text files stay plain.
    typst: bool,
    focus: FocusHandle,
    list: ListState,
    reveal: Option<Reveal>,
    painted: Painted,
    spans: Vec<(Range<usize>, SyntaxTag)>,
    marked: Option<Range<usize>>,
    selecting: bool,
    /// Column kept while moving vertically.
    goal_x: Option<Pixels>,
    folded: Option<Fold>,
    /// The user asked to see the generated block.
    pub block_shown: bool,
    /// What can complete text here, first match first.
    providers: Vec<Rc<dyn Provider>>,
    /// Width of the popup.
    pub completion_w: f32,
    completion: Option<Completion>,
    /// The `@` whose popup was accepted or dismissed: it stays closed there.
    settled: Option<usize>,
    /// Placeholders of an accepted snippet still ahead of Tab. Any caret move
    /// other than Tab ends them.
    stops: Vec<Range<usize>>,
    pointed: Option<Pointed>,
    /// Errors last, to paint over a warning in the same place.
    decorations: Vec<Decoration>,
    /// This text's compiles, which answer hover and underline it. Only Typst has them.
    session: Option<Entity<DocumentSession>>,
    _session_events: Option<Subscription>,
}

impl EventEmitter<SourceEvent> for SourceEditor {}

impl Focusable for SourceEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle { self.focus.clone() }
}

impl SourceEditor {
    pub fn new(text: String, typst: bool, providers: Vec<Rc<dyn Provider>>, cx: &mut Context<Self>) -> Self {
        let buffer = Buffer::new(text);
        let list = ListState::new(buffer.line_count(), ListAlignment::Top, px(400.));
        let spans = if typst { highlights(buffer.text()) } else { vec![] };
        let mut editor = Self { reveal: None, buffer, typst, focus: cx.focus_handle(), list, painted: Default::default(), spans, marked: None, selecting: false, goal_x: None, folded: None, block_shown: false, providers, completion_w: 380., completion: None, settled: None, stops: vec![], pointed: None, decorations: vec![], session: None, _session_events: None };
        editor.refold();
        editor
    }

    fn refold(&mut self) {
        let text = self.buffer.text();
        self.folded = bibliography::block_range(text).filter(|_| self.typst && !self.block_shown).map(|bytes| {
            let first = self.buffer.line_of(bytes.start);
            // At the end of the file the empty last line goes too, so nothing
            // can be typed below the bibliography.
            let at_end = bytes.end == text.len();
            let last = if at_end { self.buffer.line_count() } else { self.buffer.line_of(bytes.end) };
            Fold { bytes, lines: first..last, at_end }
        });
        // An undo, or the block arriving, can leave the caret where it cannot be seen.
        let caret = self.buffer.caret();
        if let Some(fold) = &self.folded && self.buffer.sel.is_empty() && fold.holds(caret) {
            let outside = fold.outside(caret, caret);
            self.buffer.move_to(outside);
        }
    }

    pub fn toggle_block(&mut self, cx: &mut Context<Self>) {
        self.block_shown = !self.block_shown;
        let folded = self.folded.as_ref().map(|fold| fold.lines.clone());
        self.refold();
        if let Some(lines) = folded.or(self.folded.as_ref().map(|fold| fold.lines.clone())) { self.splice_rows(lines.clone(), lines.len()); }
        self.completion = None;
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window) { window.focus(&self.focus); }

    pub fn text(&self) -> &str { self.buffer.text() }
    pub fn caret(&self) -> usize { self.buffer.caret() }

    pub fn set_session(&mut self, session: Entity<DocumentSession>, cx: &mut Context<Self>) {
        self._session_events = Some(cx.subscribe(&session, |editor, _, event: &SessionEvent, cx| {
            if matches!(event, SessionEvent::Diagnostics) { editor.take_diagnostics(cx); }
        }));
        self.session = Some(session);
    }

    /// Underlines the newest result, which replaces the last one only now, so
    /// nothing disappears while a compile runs.
    fn take_diagnostics(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let problems = session.read(cx).problems(self.buffer.text());
        let mut decorations: Vec<Decoration> = problems.into_iter().filter_map(|problem| {
            Some(Decoration { range: self.buffer.visible(problem.range?), error: problem.error, message: problem.message, hints: problem.hints })
        }).collect();
        decorations.sort_by_key(|decoration| (decoration.error, decoration.range.start));
        self.set_decorations(decorations, cx);
    }

    /// Replaces the underlines. What the popover says may be gone with them.
    pub fn set_decorations(&mut self, decorations: Vec<Decoration>, cx: &mut Context<Self>) {
        if self.decorations == decorations { return; }
        self.decorations = decorations;
        self.pointed = None;
        cx.notify();
    }

    /// The byte under `position` if the pointer is on text itself, for hover.
    /// The fold's row is painted when an underline in it needs pointing at.
    fn text_at(&self, position: Point<Pixels>) -> Option<usize> {
        let painted = self.painted.borrow();
        let (line, row) = painted.lines.iter().find(|(_, row)| row.bounds.contains(&position))?;
        if let Some(fold) = &self.folded && fold.lines.contains(line) { return Some(fold.bytes.start); }
        Some(self.buffer.line_range(*line).start + row.text_at(position)?)
    }

    /// Moves the caret, as when a diagnostic is clicked.
    pub fn go_to(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.focus(window);
        self.go(self.buffer.snapped(offset), false, cx);
    }

    /// The differing span between the buffer and `text`, and where the caret
    /// lands: `(old range, new end, caret)`.
    fn diff(&self, text: &str) -> (std::ops::Range<usize>, usize, usize) {
        let (range, new_len) = difference(self.buffer.text(), text);
        let (start, old_end, new_end) = (range.start, range.end, range.start + new_len);
        let caret = self.buffer.caret();
        let caret = if caret <= start { caret } else if caret >= old_end { caret + new_end - old_end } else { new_end };
        (range, new_end, caret)
    }

    /// Takes in text changed on disk as one undoable edit, keeping the caret
    /// on the same text where it can.
    pub fn reload(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.buffer.text() == text { return; }
        let (range, new_end, caret) = self.diff(text);
        let start = range.start;
        let change = self.buffer.replace_selecting(range, &text[start..new_end], caret..caret, EditKind::Other);
        self.changed(change, Trigger::Moved, cx);
    }

    /// Takes in text Thesio generated, outside the undo history and without
    /// announcing a change: it is already saved.
    pub fn sync_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.buffer.text() == text { return; }
        let (range, new_end, caret) = self.diff(text);
        let start = range.start;
        let change = self.buffer.replace_silently(range, &text[start..new_end], caret..caret);
        self.text_changed(change);
        cx.notify();
    }

    /// Inserts a citation at the caret, spaced off the word before it.
    pub fn insert_citation(&mut self, key: &str, cx: &mut Context<Self>) {
        let before = self.buffer.text()[..self.buffer.sel.start].chars().next_back();
        let glue = if before.is_some_and(|c| !c.is_whitespace() && !"([{".contains(c)) { " " } else { "" };
        self.insert(&format!("{glue}@{key}"), EditKind::Other, cx);
    }

    /// Inserts a quotation at the caret: inline like a citation, or a block in
    /// a paragraph of its own.
    pub fn insert_quote(&mut self, markup: &str, block: bool, cx: &mut Context<Self>) {
        let before = &self.buffer.text()[..self.buffer.sel.start];
        let text = if !block {
            let glue = if before.chars().next_back().is_some_and(|c| !c.is_whitespace() && !"([{".contains(c)) { " " } else { "" };
            let after = self.buffer.text()[self.buffer.sel.end..].chars().next();
            let tail = if after.is_some_and(char::is_alphanumeric) { " " } else { "" };
            format!("{glue}{markup}{tail}")
        } else {
            format!("{}{markup}\n\n", block_glue(before))
        };
        self.insert(&text, EditKind::Other, cx);
    }

    /// Replaces the selection with a figure for each path, as one step to undo,
    /// with the caret in the first caption and Tab reaching the others.
    pub fn insert_figures(&mut self, paths: &[String], cx: &mut Context<Self>) {
        let sel = self.buffer.sel.clone();
        let text = self.buffer.text();
        let sel = if sel.is_empty() { let place = figure_place(text, sel.start); place..place } else { sel };
        let (inserted, captions) = figures(paths, &text[..sel.start], &text[sel.end..]);
        let Some(first) = captions.first().map(|caption| sel.start + caption) else { return };
        let change = self.buffer.replace_selecting(sel.clone(), &inserted, first..first, EditKind::Other);
        self.changed(change, Trigger::Moved, cx);
        self.stops = captions[1..].iter().map(|caption| sel.start + caption..sel.start + caption).collect();
    }

    /// Replaces rows, keeping the view: `splice` sends a list whose top row is
    /// replaced to that row's start, and paragraphs are often taller than the view.
    fn splice_rows(&mut self, old: Range<usize>, new: usize) {
        let top = self.list.logical_scroll_top();
        self.list.splice(old.clone(), new);
        if old.contains(&top.item_ix) {
            let item_ix = old.start + (top.item_ix - old.start).min(new.saturating_sub(1));
            self.list.scroll_to(ListOffset { item_ix, offset_in_item: top.offset_in_item });
        }
    }

    /// Brings what derives from the text up to date. Rows follow only the edit:
    /// a reset would lose the scroll position and every measured height, and
    /// with them the way back to the caret.
    fn text_changed(&mut self, change: Change) {
        if self.typst { self.spans = highlights(self.buffer.text()); }
        // An undo or a reload before the popup moves the text its offsets and items point at.
        if self.completion.as_ref().is_some_and(|open| change.range.start < open.range.start) { self.completion = None; }
        self.refold();
        self.decorations.retain_mut(|decoration| stretch(&mut decoration.range, &change.range, change.new_len));
        self.stops.retain_mut(|stop| follow(stop, &change.range, change.new_len));
        self.pointed = None;
        self.splice_rows(change.lines, change.new_lines);
    }

    fn changed(&mut self, change: Change, trigger: Trigger, cx: &mut Context<Self>) {
        self.text_changed(change);
        self.goal_x = None;
        self.request_reveal(TYPING_MARGIN);
        cx.notify();
        self.refresh_completion(trigger, cx);
        cx.emit(SourceEvent::Changed);
        cx.emit(SourceEvent::Caret);
    }

    /// Follows the caret: the popup moves with it, closes where no provider
    /// has anything to complete, and opens when `trigger` says so.
    fn refresh_completion(&mut self, trigger: Trigger, cx: &mut Context<Self>) {
        let (text, caret) = (self.buffer.text(), self.buffer.caret());
        let plain = self.typst && self.buffer.sel.is_empty() && self.marked.is_none();
        let open = self.completion.as_ref().map(|open| (open.kind, open.range.start));
        let reach = |provider: &Rc<dyn Provider>| match open {
            _ if trigger == Trigger::Asked => Reach::Asked,
            Some((kind, start)) if kind == provider.kind() => Reach::Open(start),
            _ => Reach::Typed,
        };
        let found: Option<(Rc<dyn Provider>, Found)> = plain.then(|| self.providers.iter().find_map(|provider| Some((provider.clone(), provider.find(text, caret, reach(provider))?)))).flatten();
        let Some((provider, found)) = found else {
            self.settled = None;
            self.completion = None;
            return;
        };
        if trigger == Trigger::Asked { self.settled = None; }
        if self.settled == Some(found.range.start) { return; }
        match &mut self.completion {
            Some(open) if open.kind == provider.kind() && open.range.start == found.range.start => {
                open.range = found.range;
                if open.query != found.query {
                    open.query = found.query;
                    open.select(0);
                    self.query_completion(false, cx);
                }
            }
            _ if trigger == Trigger::Moved => self.completion = None,
            _ => {
                self.completion = Some(Completion::new(provider, found, trigger == Trigger::Asked));
                self.query_completion(true, cx);
            }
        }
    }

    fn query_completion(&mut self, opening: bool, cx: &mut Context<Self>) {
        let Some(open) = &mut self.completion else { return };
        let query = Query { text: open.query.clone(), opening, document: self.buffer.text().to_string(), caret: self.buffer.caret(), asked: open.asked };
        let answer = open.provider.items(query, cx);
        open.pending = Some(cx.spawn(async move |this, cx| {
            let items = answer.await;
            let _ = this.update(cx, |this, cx| this.completion_answered(items, cx));
        }));
    }

    fn completion_answered(&mut self, items: Vec<Item>, cx: &mut Context<Self>) {
        let Some(open) = &mut self.completion else { return };
        open.items = items;
        open.select(open.selected.min(open.items.len().saturating_sub(1)));
        cx.notify();
    }

    fn complete(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.completion.as_ref().is_none_or(|open| index >= open.items.len()) { return; }
        let Some(mut open) = self.completion.take() else { return };
        let item = open.items.swap_remove(index);
        let kind = item.kind;
        self.settled = Some(open.range.start);
        let at = item.from.unwrap_or(open.range.start);
        let sel = item.selection.map_or(at + item.insert.len()..at + item.insert.len(), |sel| at + sel.start..at + sel.end);
        let end = self.buffer.snapped(open.range.end + item.tail);
        let change = self.buffer.replace_selecting(at..end, &item.insert, sel, EditKind::Other);
        self.changed(change, Trigger::Moved, cx);
        self.stops = item.stops.iter().map(|stop| at + stop.start..at + stop.end).collect();
        cx.emit(SourceEvent::Completed(kind));
    }

    fn completion_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(open) = &mut self.completion else { return };
        let shown = open.items.len();
        if shown == 0 { return; }
        open.select((open.selected + if down { 1 } else { shown - 1 }) % shown);
        cx.notify();
    }

    fn completion_accept(&mut self, _: &CompletionAccept, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.completion.as_ref().map_or(0, |open| open.selected);
        self.complete(selected, cx);
    }

    fn completion_dismiss(&mut self, _: &CompletionDismiss, _: &mut Window, cx: &mut Context<Self>) {
        self.settled = self.completion.take().map(|open| open.range.start);
        cx.notify();
    }

    /// A popup with no items draws nothing and stays open for typing to bring them back.
    fn completion_shown(&self) -> bool { self.completion.as_ref().is_some_and(|open| !open.items.is_empty()) }

    /// Where the row holding `offset` was last painted, in window coordinates.
    fn painted_at(&self, offset: usize) -> Option<Point<Pixels>> {
        let line = self.buffer.line_of(offset);
        let line = self.folded.as_ref().filter(|fold| fold.lines.contains(&line)).map_or(line, |fold| fold.lines.start);
        let painted = self.painted.borrow();
        let row = painted.lines.get(&line)?;
        Some(row.bounds.origin + row.position(offset - self.buffer.line_range(line).start)?)
    }

    fn render_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.focus.is_focused(window) { return None; }
        let open = self.completion.as_ref()?;
        if open.items.is_empty() { return None; }
        // A row not painted yet: the reveal that follows the edit brings the popup back.
        let at = self.painted_at(open.range.start)?;
        let (count, scroll) = (open.items.len(), open.scroll.clone());
        let info = open.items.get(open.selected).and_then(|item| item.info.clone());
        let rows = uniform_list("completion-rows", count, cx.processor(|editor: &mut Self, range: Range<usize>, _, cx| {
            let Some(open) = &editor.completion else { return vec![] };
            range.filter_map(|i| {
                let item = open.items.get(i)?;
                // The list places its items' outer edges, so the row's margin moves to a wrapper.
                Some(div().px(px(4.)).child(menu_row(("completion", i)).mx(px(0.)).h(px(COMPLETION_ROW_H)).flex_col().items_start().justify_center().gap(px(0.)).overflow_hidden()
                    .when(i == open.selected, |d| d.bg(Theme::menu_highlight()))
                    .when(i != open.selected, |d| d.hover(|s| s.bg(Theme::fill_hover())))
                    .child(div().w_full().text_size(px(13.)).line_height(px(18.)).truncate().child(ui_text(item.title.clone())))
                    .child(div().w_full().text_size(px(12.)).line_height(px(16.)).truncate().text_color(Theme::text_caption()).child(ui_text(item.detail.clone())))
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(move |e, _, _, cx| e.complete(i, cx)))))
            }).collect()
        })).track_scroll(scroll).flex_none().h(px(COMPLETION_ROW_H * count.min(COMPLETION_ROWS) as f32));
        let menu = popover().id("completion").occlude().w(px(self.completion_w)).py(px(MENU_PAD_Y)).flex().flex_col().shadow(theme::shadow_panel()).child(rows)
            .children(info.map(|info| div().mt(px(4.)).px(px(12.)).py(px(4.)).border_t_1().border_color(Theme::line_border()).text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption()).child(ui_text(info))));
        Some(deferred(anchored().position_mode(AnchoredPositionMode::Window).position(at).offset(point(px(0.), px(LINE_HEIGHT + 4.))).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(crate::ui::layer::COMPLETION).into_any_element())
    }

    /// Follows the pointer: on a new word or underline it waits, then says what
    /// the underline means or asks what the word is.
    fn point_at(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let Some(at) = at.filter(|_| self.session.is_some() && !self.completion_shown()) else { return self.stop_pointing(cx) };
        let place = self.folded.as_ref().filter(|fold| fold.holds(at)).map_or(at..at + 1, |fold| fold.bytes.clone());
        let problems: Vec<Decoration> = self.decorations.iter().rev().filter(|decoration| decoration.meets(&place)).cloned().collect();
        let problem = !problems.is_empty();
        if self.pointed.as_ref().is_some_and(|pointed| pointed.word.contains(&at) && pointed.problem == problem) { return; }
        let line = self.buffer.line_range(self.buffer.line_of(at)).start;
        let (word, anchor, pending) = if problem {
            let word = problems.iter().fold(place, |word, decoration| word.start.min(decoration.range.start)..word.end.max(decoration.range.end));
            let start = word.start.max(line);
            // An underline over wrapped rows: the popover goes under the row pointed at.
            let anchor = if self.painted_at(start).map(|p| p.y) == self.painted_at(at).map(|p| p.y) { start } else { at };
            (word, anchor, self.say_problems(problems, cx))
        } else {
            let word = word_around(self.buffer.text(), at);
            let anchor = word.start;
            (word, anchor, self.ask_about(at, cx))
        };
        self.stop_pointing(cx);
        self.pointed = Some(Pointed { word, anchor, problem, shown: None, _pending: Some(pending) });
    }

    fn say_problems(&self, problems: Vec<Decoration>, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_DELAY).await;
            let _ = this.update(cx, |this, cx| this.answered(Some(Said::Problems(problems)), cx));
        })
    }

    fn ask_about(&self, at: usize, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_DELAY).await;
            let Ok(Some(answer)) = this.update(cx, |this, cx| {
                let session = this.session.as_ref()?.read(cx);
                Some(session.hover(this.buffer.text().to_string(), at, cx))
            }) else { return };
            let shown = answer.await;
            let _ = this.update(cx, |this, cx| this.answered(shown.map(Said::Docs), cx));
        })
    }

    fn answered(&mut self, said: Option<Said>, cx: &mut Context<Self>) {
        if let Some(pointed) = &mut self.pointed { pointed.shown = said; }
        cx.notify();
    }

    fn stop_pointing(&mut self, cx: &mut Context<Self>) {
        if self.pointed.take().is_some_and(|pointed| pointed.shown.is_some()) { cx.notify(); }
    }

    /// Esc: this word stays quiet until the pointer moves to another.
    fn hover_dismiss(&mut self, _: &HoverDismiss, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(pointed) = &mut self.pointed { (pointed.shown, pointed._pending) = (None, None); }
        cx.notify();
    }

    fn render_hover(&self) -> Option<AnyElement> {
        let pointed = self.pointed.as_ref().filter(|_| !self.completion_shown())?;
        let shown = pointed.shown.as_ref()?;
        let at = self.painted_at(pointed.anchor)?;
        let body = div().text_size(px(12.)).line_height(px(17.));
        let body = match shown {
            Said::Docs(hover) if hover.code => body.font_family(MONO_FONT).child(hover.text.clone()),
            Said::Docs(hover) => body.child(ui_text(hover.text.clone())),
            Said::Problems(problems) => body.flex().flex_col().gap(px(6.)).children(problems.iter().map(|problem| {
                // Plain strings wrap, and a message can name a whole path or label.
                div().child(problem.message.clone())
                    .children(problem.hints.iter().map(|hint| div().text_color(Theme::text_caption()).child(format!("Hint: {hint}"))))
            })),
        };
        let card = popover().max_w(px(self.completion_w)).max_h(px(HOVER_MAX_H)).overflow_hidden().px(px(10.)).py(px(6.)).shadow(theme::shadow_panel()).child(body);
        Some(deferred(anchored().position_mode(AnchoredPositionMode::Window).position(at).offset(point(px(0.), px(LINE_HEIGHT + 4.))).snap_to_window_with_margin(px(8.)).child(card)).with_priority(crate::ui::layer::COMPLETION).into_any_element())
    }

    fn request_reveal(&mut self, margin: f32) {
        self.reveal = Some(Reveal { margin, jump: Jump::Unmoved, frame: self.painted.borrow().frame });
    }

    /// After a frame: scrolls by what the caret itself needs. A row's height
    /// is unknown right after an edit, and a row can be taller than the view.
    fn settle_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(reveal) = &mut self.reveal else { return };
        if reveal.frame >= self.painted.borrow().frame { return cx.notify(); }
        let Some(at) = self.painted_at(self.buffer.caret()) else {
            let Some(reveal) = &mut self.reveal else { return };
            reveal.frame = self.painted.borrow().frame;
            let line = self.buffer.line_of(self.buffer.caret());
            match reveal.jump {
                Jump::Unmoved => {
                    reveal.jump = Jump::Near;
                    self.list.scroll_to_reveal_item(line);
                }
                Jump::Near => {
                    reveal.jump = Jump::Row;
                    self.list.scroll_to(ListOffset { item_ix: line, offset_in_item: px(0.) });
                }
                Jump::Row => self.reveal = None,
            }
            return cx.notify();
        };
        let margin = self.reveal.take().map_or(0., |reveal| reveal.margin);
        let view = self.list.viewport_bounds();
        let (top, bottom) = (view.top(), view.bottom() - px(LINE_HEIGHT + margin));
        let distance = if at.y > bottom { at.y - bottom } else if at.y < top { at.y - top } else { px(0.) };
        self.list.scroll_by(distance);
        if distance != px(0.) || self.completion.is_some() { cx.notify(); }
    }

    /// Every caret move that is not an edit ends here. It stays out of the
    /// fold, and with `reveal` it comes into view.
    fn move_caret(&mut self, place: Place, reveal: bool, cx: &mut Context<Self>) {
        self.stops.clear();
        let (sel, caret) = (self.buffer.sel.clone(), self.buffer.caret());
        let outside = |editor: &Self, offset| editor.folded.as_ref().map_or(offset, |fold| fold.outside(offset, caret));
        match place {
            Place::At(offset) => self.buffer.move_to(outside(self, offset)),
            Place::Extend(offset) => self.buffer.select_to(outside(self, offset)),
            Place::Select(range) => {
                self.buffer.move_to(range.start);
                self.buffer.select_to(range.end);
            }
        }
        self.goal_x = None;
        if reveal {
            self.request_reveal(0.);
            let line = self.buffer.line_of(self.buffer.caret());
            if !self.painted.borrow().lines.contains_key(&line) {
                self.list.scroll_to_reveal_item(line);
                if let Some(reveal) = &mut self.reveal { reveal.jump = Jump::Near; }
            }
        }
        self.refresh_completion(Trigger::Moved, cx);
        cx.notify();
        if reveal || self.buffer.sel != sel { cx.emit(SourceEvent::Caret); }
    }

    fn go(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        self.move_caret(if extend { Place::Extend(offset) } else { Place::At(offset) }, true, cx);
    }

    fn delete_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let range = if self.buffer.sel.is_empty() {
            let caret = self.buffer.caret();
            caret.min(offset)..caret.max(offset)
        } else { self.buffer.sel.clone() };
        if !range.is_empty() {
            let change = self.buffer.replace(range, "", EditKind::Deleting);
            self.changed(change, Trigger::Moved, cx);
        }
    }

    fn insert(&mut self, text: &str, kind: EditKind, cx: &mut Context<Self>) {
        let change = self.buffer.replace(self.buffer.sel.clone(), text, kind);
        self.changed(change, Trigger::Moved, cx);
    }

    /// The offset one visual row away, keeping the column.
    fn vertical(&mut self, down: bool) -> usize {
        let caret = self.buffer.caret();
        let line = self.buffer.line_of(caret);
        let range = self.buffer.line_range(line);
        let height = px(LINE_HEIGHT);
        let painted = self.painted.borrow();
        let current = painted.lines.get(&line);
        let here = current.and_then(|row| row.position(caret - range.start));
        let x = self.goal_x.or(here.map(|p| p.x)).unwrap_or(px(0.));
        // A move inside a wrapped line stays on it.
        if let (Some(here), Some(row)) = (here, current) {
            let y = if down { here.y + height } else { here.y - height };
            if y >= px(0.) && y < row.height() {
                let offset = range.start + row.index_at(point(x, y));
                drop(painted);
                self.goal_x = Some(x);
                return offset;
            }
        }
        let next = if down { line + 1 } else { line.wrapping_sub(1) };
        let target = self.folded.as_ref().map_or(next, |fold| fold.past(next, down));
        if target >= self.buffer.line_count() {
            return if down { self.buffer.text().len() } else { 0 };
        }
        let target_range = self.buffer.line_range(target);
        let offset = match painted.lines.get(&target) {
            Some(row) => self.place_in_text(target, row.index_at(point(x, if down { px(0.) } else { row.height() - height }))),
            None => target_range.end.min(target_range.start + (caret - range.start)),
        };
        drop(painted);
        self.goal_x = Some(x);
        offset
    }

    fn line_start(&self) -> usize { self.buffer.line_range(self.buffer.line_of(self.buffer.caret())).start }
    fn line_end(&self) -> usize { self.buffer.line_range(self.buffer.line_of(self.buffer.caret())).end }

    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let painted = self.painted.borrow();
        let row = painted.lines.iter().min_by_key(|(_, p)| {
            let (top, bottom) = (p.bounds.top(), p.bounds.bottom());
            if position.y < top { top - position.y } else if position.y >= bottom { position.y - bottom + px(1.) } else { px(0.) }
        });
        let Some((line, row)) = row else { return self.buffer.text().len() };
        self.place_in_text(*line, row.index_for(position))
    }

    /// Painted rows can be a frame behind an edit, so what they say may no longer exist.
    fn place_in_text(&self, line: usize, index: usize) -> usize {
        let range = self.buffer.line_range(line.min(self.buffer.line_count() - 1));
        self.buffer.snapped((range.start + index).min(range.end))
    }

    fn on_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.completion = None;
        self.stop_pointing(cx);
        let caret = self.buffer.caret();
        let at = self.offset_at(event.position);
        let offset = self.folded.as_ref().map_or(at, |fold| fold.outside(at, caret));
        let place = if event.click_count == 2 {
            Place::Select(self.buffer.word_at(offset))
        } else if event.click_count >= 3 {
            Place::Select(self.buffer.line_range(self.buffer.line_of(offset)))
        } else if event.modifiers.shift {
            Place::Extend(offset)
        } else {
            Place::At(offset)
        };
        self.selecting = true;
        self.move_caret(place, false, cx);
        if event.click_count <= 1 && !event.modifiers.shift && event.modifiers.platform { cx.emit(SourceEvent::Follow(offset)); }
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting { self.move_caret(Place::Extend(self.offset_at(event.position)), false, cx); }
        else if cx.has_active_drag() { self.stop_pointing(cx); }
        else { self.point_at(self.text_at(event.position), cx); }
    }

    fn on_file_drop(&mut self, files: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        self.focus(window);
        cx.emit(SourceEvent::Images(files.paths().iter().cloned().map(Incoming::File).collect()));
    }

    fn on_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) { self.selecting = false; }

    fn runs(&self, range: &Range<usize>) -> Vec<TextRun> {
        let base = font(MONO_FONT);
        let run = |len: usize, tag: Option<SyntaxTag>| TextRun {
            len,
            font: match tag {
                Some(SyntaxTag::Strong | SyntaxTag::Heading) => base.clone().bold(),
                Some(SyntaxTag::Emph) => base.clone().italic(),
                _ => base.clone(),
            },
            color: tag.map_or(Theme::text(), tag_color),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let mut runs = vec![];
        let mut at = range.start;
        let first = self.spans.partition_point(|(r, _)| r.end <= range.start);
        for (span, tag) in &self.spans[first..] {
            if span.start >= range.end { break; }
            let start = span.start.max(range.start);
            if start > at { runs.push(run(start - at, None)); }
            let end = span.end.min(range.end);
            runs.push(run(end - start, Some(*tag)));
            at = end;
        }
        if at < range.end { runs.push(run(range.end - at, None)); }
        if let Some(marked) = &self.marked && marked.start < range.end && marked.end > range.start {
            for r in &mut runs { r.underline = Some(UnderlineStyle { thickness: px(1.), color: None, wavy: false }); }
        }
        runs
    }

    fn render_line(&mut self, line: usize, window: &mut Window, _: &mut Context<Self>) -> AnyElement {
        if let Some(fold) = &self.folded && fold.lines.contains(&line) {
            if line != fold.lines.start { return div().h(px(0.)).into_any_element(); }
            // Its text is hidden, so the row stands for an underline inside it.
            let problem = self.decorations.iter().rfind(|decoration| decoration.meets(&fold.bytes)).map(Decoration::color);
            let painted = self.painted.clone();
            return div().h(px(LINE_HEIGHT + 8.)).pt(px(4.)).pb(px(4.)).child(
                div().relative().size_full().px(px(8.)).flex().items_center().rounded(px(6.)).bg(Theme::fill_hover())
                    .text_size(px(12.)).text_color(Theme::text_caption()).child("Bibliography, generated by Thesio")
                    .when_some(problem, |row, color| row.text_decoration_1().text_decoration_wavy().text_decoration_color(color).child(canvas(|_, _, _| (), move |bounds, _, _, _| {
                        painted.borrow_mut().insert(line, PaintedLine { bounds, layout: None, line_height: px(LINE_HEIGHT) });
                    }).absolute().size_full()))
            ).into_any_element();
        }
        let range = self.buffer.line_range(line);
        let sel = &self.buffer.sel;
        let selection = (!sel.is_empty() && sel.start <= range.end && sel.end > range.start)
            .then(|| sel.start.max(range.start) - range.start..sel.end.min(range.end) - range.start);
        let caret = self.buffer.caret();
        let caret = (self.focus.is_focused(window) && sel.is_empty() && caret >= range.start && caret <= range.end).then(|| caret - range.start);
        let decorations = self.decorations.iter().filter(|d| d.meets(&range))
            .map(|d| (d.range.start.max(range.start) - range.start..d.range.end.min(range.end) - range.start, d.color())).collect();
        LineElement {
            line,
            text: SharedString::from(self.buffer.text()[range.clone()].to_string()),
            runs: self.runs(&range),
            selection,
            through_newline: sel.end > range.end,
            caret,
            decorations,
            painted: self.painted.clone(),
        }.into_any_element()
    }

    /// An edit outside the input method can leave the composition past the text.
    fn marked_range(&self) -> Option<Range<usize>> { self.marked.clone().filter(|marked| marked.end <= self.buffer.text().len()) }

    fn range8(&self, range: &Range<usize>) -> Range<usize> { range_from_utf16(self.buffer.text(), range) }
    fn range16(&self, range: &Range<usize>) -> Range<usize> { range_to_utf16(self.buffer.text(), range) }
}

fn tag_color(tag: SyntaxTag) -> Hsla {
    match tag {
        SyntaxTag::Comment => c(craft::N500),
        SyntaxTag::Punctuation | SyntaxTag::ListMarker | SyntaxTag::Escape => c(craft::N600),
        SyntaxTag::Heading | SyntaxTag::Keyword | SyntaxTag::Label | SyntaxTag::Ref | SyntaxTag::Link => c(craft::B500),
        SyntaxTag::Function | SyntaxTag::MathOperator | SyntaxTag::MathDelimiter | SyntaxTag::Operator => c(craft::B700),
        SyntaxTag::Raw | SyntaxTag::String | SyntaxTag::Number => c(craft::N700),
        SyntaxTag::Error => Theme::danger(),
        _ => Theme::text(),
    }
}

macro_rules! motions {
    ($($name:ident: $action:ident, $extend:expr, |$this:ident| $to:expr;)*) => {
        impl SourceEditor {
            $(fn $name(&mut self, _: &$action, _: &mut Window, cx: &mut Context<Self>) {
                let $this = &*self;
                let to = $to;
                self.go(to, $extend, cx);
            })*
        }
    };
}

motions! {
    word_left: WordLeft, false, |t| t.buffer.prev_word(t.buffer.caret());
    word_right: WordRight, false, |t| t.buffer.next_word(t.buffer.caret());
    to_line_start: LineStart, false, |t| t.line_start();
    to_line_end: LineEnd, false, |t| t.line_end();
    doc_start: DocStart, false, |_t| 0;
    doc_end: DocEnd, false, |t| t.buffer.text().len();
    select_left: SelectLeft, true, |t| t.buffer.prev_grapheme(t.buffer.caret());
    select_right: SelectRight, true, |t| t.buffer.next_grapheme(t.buffer.caret());
    select_word_left: SelectWordLeft, true, |t| t.buffer.prev_word(t.buffer.caret());
    select_word_right: SelectWordRight, true, |t| t.buffer.next_word(t.buffer.caret());
    select_line_start: SelectLineStart, true, |t| t.line_start();
    select_line_end: SelectLineEnd, true, |t| t.line_end();
}

impl SourceEditor {
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let to = if self.buffer.sel.is_empty() { self.buffer.prev_grapheme(self.buffer.caret()) } else { self.buffer.sel.start };
        self.go(to, false, cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let to = if self.buffer.sel.is_empty() { self.buffer.next_grapheme(self.buffer.caret()) } else { self.buffer.sel.end };
        self.go(to, false, cx);
    }
    fn vertical_move(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let to = self.vertical(down);
        let goal = self.goal_x;
        self.go(to, extend, cx);
        self.goal_x = goal;
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) { self.vertical_move(false, false, cx); }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) { self.vertical_move(true, false, cx); }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) { self.vertical_move(false, true, cx); }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) { self.vertical_move(true, true, cx); }
    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.prev_grapheme(self.buffer.caret());
        self.delete_to(to, cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_grapheme(self.buffer.caret());
        self.delete_to(to, cx);
    }
    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.prev_word(self.buffer.caret());
        self.delete_to(to, cx);
    }
    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_word(self.buffer.caret());
        self.delete_to(to, cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Place::Select(0..self.buffer.text().len()), false, cx);
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.sel.is_empty() { crate::system::copy(self.buffer.selected().to_string(), cx); }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.sel.is_empty() {
            crate::system::copy(self.buffer.selected().to_string(), cx);
            self.insert("", EditKind::Other, cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let images = if self.typst { crate::system::pasted_images(cx) } else { vec![] };
        if !images.is_empty() { return cx.emit(SourceEvent::Images(images)); }
        if let Some(text) = crate::system::paste(cx) {
            self.insert(&plain_newlines(&text), EditKind::Other, cx);
        }
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) { if let Some(change) = self.buffer.undo() { self.changed(change, Trigger::Moved, cx); } }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) { if let Some(change) = self.buffer.redo() { self.changed(change, Trigger::Moved, cx); } }
    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.line_start();
        let indent: String = self.buffer.text()[start..self.buffer.sel.start.max(start)].chars().take_while(|ch| *ch == ' ' || *ch == '\t').collect();
        self.insert(&format!("\n{indent}"), EditKind::Other, cx);
    }
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.stops.is_empty() { return self.insert("  ", EditKind::Typing, cx); }
        let mut rest = std::mem::take(&mut self.stops);
        let next = rest.remove(0);
        self.move_caret(Place::Select(next), true, cx);
        self.stops = rest;
    }
}

impl EntityInputHandler for SourceEditor {
    fn text_for_range(&mut self, range: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let range = self.range8(&range);
        *actual = Some(self.range16(&range));
        Some(self.buffer.text()[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range16(&self.buffer.sel), reversed: self.buffer.reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.range16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) { self.marked = None; }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let range = range.map(|r| self.range8(&r)).or(self.marked_range()).unwrap_or(self.buffer.sel.clone());
        self.marked = None;
        let change = self.buffer.replace(range, text, EditKind::Typing);
        self.changed(change, Trigger::Typed, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, new_sel: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        let range = range.map(|r| self.range8(&r)).or(self.marked_range()).unwrap_or(self.buffer.sel.clone());
        let base = range.start;
        let sel = new_sel.map_or(base + text.len()..base + text.len(), |sel| base + from_utf16(text, sel.start)..base + from_utf16(text, sel.end));
        let change = self.buffer.replace_selecting(range, text, sel, EditKind::Typing);
        self.marked = (!text.is_empty()).then(|| base..base + text.len());
        self.changed(change, Trigger::Moved, cx);
    }

    fn bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let offset = self.range8(&range).start;
        let line = self.buffer.line_of(offset);
        let painted = self.painted.borrow();
        let row = painted.lines.get(&line)?;
        let local = row.position(offset - self.buffer.line_range(line).start).unwrap_or_default();
        Some(Bounds::new(row.bounds.origin + local, size(px(1.), px(LINE_HEIGHT))))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(to_utf16(self.buffer.text(), self.offset_at(point)))
    }
}

impl Render for SourceEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.painted.borrow_mut().begin_frame();
        if self.reveal.is_some() { cx.on_next_frame(window, |editor, _, cx| editor.settle_reveal(cx)); }
        let completion = self.render_completion(window, cx);
        let hover = self.render_hover();
        let mut context = KeyContext::new_with_defaults();
        context.add("SourceEditor");
        if completion.is_some() { context.add("completing"); }
        if hover.is_some() { context.add("hovering"); }
        let entity = cx.entity();
        let focus = self.focus.clone();
        div()
            .id("source-editor")
            .key_context(context)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .relative()
            .size_full()
            .bg(Theme::bg_body())
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::to_line_start))
            .on_action(cx.listener(Self::to_line_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(|e, _: &CompletionUp, _, cx| e.completion_step(false, cx)))
            .on_action(cx.listener(|e, _: &CompletionDown, _, cx| e.completion_step(true, cx)))
            .on_action(cx.listener(Self::completion_accept))
            .on_action(cx.listener(Self::completion_dismiss))
            .on_action(cx.listener(Self::hover_dismiss))
            .on_action(cx.listener(|e, _: &CompletionTrigger, _, cx| e.refresh_completion(Trigger::Asked, cx)))
            .when(self.typst, |d| d.drag_over::<ExternalPaths>(|s, _, _, _| s.bg(Theme::fill_hover())).on_drop(cx.listener(Self::on_file_drop)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_hover(cx.listener(|e, hovered: &bool, _, cx| if !hovered { e.stop_pointing(cx); }))
            .on_scroll_wheel(cx.listener(|e, _: &ScrollWheelEvent, _, cx| e.stop_pointing(cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_up))
            .child(div().size_full().pl(px(PAD_X)).pr(px(PAD_X)).pt(px(16.)).child(list(self.list.clone(), cx.processor(Self::render_line)).size_full().pb(px(END_PADDING))))
            .child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
            }).absolute().size_full())
            .children(completion)
            .children(hover)
    }
}

struct LineElement {
    line: usize,
    text: SharedString,
    runs: Vec<TextRun>,
    selection: Option<Range<usize>>,
    through_newline: bool,
    caret: Option<usize>,
    decorations: Vec<(Range<usize>, Hsla)>,
    painted: Painted,
}

impl IntoElement for LineElement {
    type Element = Self;
    fn into_element(self) -> Self { self }
}

impl Element for LineElement {
    type RequestLayoutState = Shaped;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> { None }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, _: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        request_line(self.text.clone(), self.runs.clone(), px(FONT_SIZE), px(LINE_HEIGHT), true, window)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Window, _: &mut App) {}

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, state: &mut Self::RequestLayoutState, _: &mut (), window: &mut Window, cx: &mut App) {
        let line = PaintedLine { bounds, layout: state.borrow().clone(), line_height: px(LINE_HEIGHT) };
        let marks = LineMarks { selection: self.selection.clone(), selection_tail: if self.through_newline { px(7.) } else { px(0.) }, caret: self.caret.map(|at| CaretMark { at, width: px(2.), inset: px(2.) }), decorations: std::mem::take(&mut self.decorations) };
        paint_line(&line, &marks, window, cx);
        self.painted.borrow_mut().insert(self.line, line);
    }
}
