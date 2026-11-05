//! The Typst source editor: one virtualized list row per logical line, soft
//! wrapped, with the selection and caret painted by each row.

use crate::citations::matches;
use crate::library::Row;
use crate::theme::{self, MONO_FONT, Theme, c, craft};
use crate::ui::{MENU_PAD_Y, ROW_RADIUS, popover, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc};
use thesio_core::buffer::{Buffer, EditKind};
use thesio_core::library;
use thesio_core::references::citation_at;
use thesio_core::syntax::{SyntaxTag, highlights};

const FONT_SIZE: f32 = 14.;
const LINE_HEIGHT: f32 = 22.;
const PAD_X: f32 = 24.;

actions!(
    source_editor,
    [Backspace, Delete, DeleteWordLeft, DeleteWordRight, Left, Right, Up, Down, WordLeft, WordRight, LineStart, LineEnd,
     DocStart, DocEnd, SelectLeft, SelectRight, SelectUp, SelectDown, SelectWordLeft, SelectWordRight, SelectLineStart,
     SelectLineEnd, SelectAll, Copy, Cut, Paste, Undo, Redo, Enter, Tab, CompletionUp, CompletionDown, CompletionAccept,
     CompletionDismiss]
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
        KeyBinding::new("up", CompletionUp, COMPLETING),
        KeyBinding::new("down", CompletionDown, COMPLETING),
        KeyBinding::new("enter", CompletionAccept, COMPLETING),
        KeyBinding::new("tab", CompletionAccept, COMPLETING),
        KeyBinding::new("escape", CompletionDismiss, COMPLETING),
    ]);
}

const COMPLETING: Option<&str> = Some("SourceEditor && completing");
/// Sources the popup offers at once.
const COMPLETION_ROWS: usize = 6;
/// Frames to wait for a new `@` to be laid out before giving up on the popup.
const COMPLETION_WAIT: u8 = 3;

pub enum SourceEvent {
    Changed,
    /// A citation was started: the editor wants fresh `sources`.
    Citing,
    /// A citation was completed.
    Cited,
}

/// The source popup over the `@key` at `range`.
struct Completion {
    range: Range<usize>,
    query: String,
    selected: usize,
    waited: u8,
}

/// A painted row: where it landed and how it wrapped, for hit testing.
#[derive(Clone)]
struct PaintedLine {
    bounds: Bounds<Pixels>,
    layout: Option<WrappedLine>,
}

type Painted = Rc<RefCell<HashMap<usize, PaintedLine>>>;

pub struct SourceEditor {
    pub buffer: Buffer,
    /// Highlight as Typst; other text files stay plain.
    typst: bool,
    focus: FocusHandle,
    list: ListState,
    painted: Painted,
    spans: Vec<(Range<usize>, SyntaxTag)>,
    marked: Option<Range<usize>>,
    selecting: bool,
    /// Column kept while moving vertically.
    goal_x: Option<Pixels>,
    /// The generated bibliography, folded into one row, as bytes and lines.
    folded: Option<(Range<usize>, Range<usize>)>,
    /// The user asked to see the generated block.
    pub block_shown: bool,
    /// What `@` completes to.
    pub sources: Vec<Row>,
    /// Width of the source popup.
    pub completion_w: f32,
    completion: Option<Completion>,
    /// The `@` whose popup was accepted or dismissed: it stays closed there.
    settled: Option<usize>,
}

impl EventEmitter<SourceEvent> for SourceEditor {}

impl Focusable for SourceEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle { self.focus.clone() }
}

impl SourceEditor {
    pub fn new(text: String, typst: bool, cx: &mut Context<Self>) -> Self {
        let buffer = Buffer::new(text);
        let list = ListState::new(buffer.line_count(), ListAlignment::Top, px(400.));
        let spans = if typst { highlights(buffer.text()) } else { vec![] };
        let mut editor = Self { buffer, typst, focus: cx.focus_handle(), list, painted: Default::default(), spans, marked: None, selecting: false, goal_x: None, folded: None, block_shown: false, sources: vec![], completion_w: 380., completion: None, settled: None };
        editor.refold();
        editor
    }

    fn refold(&mut self) {
        let text = self.buffer.text();
        self.folded = library::block_range(text).filter(|_| self.typst && !self.block_shown).map(|bytes| {
            let first = self.buffer.line_of(bytes.start);
            // At the end of the file the empty last line goes too, so nothing
            // can be typed below the bibliography.
            let last = if bytes.end == text.len() { self.buffer.line_count() } else { self.buffer.line_of(bytes.end) };
            (bytes, first..last)
        });
    }

    pub fn toggle_block(&mut self, cx: &mut Context<Self>) {
        self.block_shown = !self.block_shown;
        self.refold();
        self.list.reset(self.buffer.line_count());
        self.completion = None;
        cx.notify();
    }

    /// Moves an offset out of the folded block, the way the caret was heading.
    fn outside_fold(&self, offset: usize) -> usize {
        let Some((bytes, _)) = &self.folded else { return offset };
        let at_end = bytes.end == self.buffer.text().len();
        if offset < bytes.start || (offset >= bytes.end && !at_end) { return offset; }
        let before = bytes.start.saturating_sub(1);
        if at_end || offset < self.buffer.caret() { before } else { bytes.end }
    }

    pub fn focus(&self, window: &mut Window) { window.focus(&self.focus); }

    /// Moves the caret, as when a diagnostic is clicked.
    pub fn go_to(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_to(offset.min(self.buffer.text().len()));
        self.focus(window);
        self.reveal(cx);
    }

    /// The differing span between the buffer and `text`, and where the caret
    /// lands: `(old range, new end, caret)`.
    fn diff(&self, text: &str) -> (std::ops::Range<usize>, usize, usize) {
        let old = self.buffer.text();
        let mut start = old.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
        while !old.is_char_boundary(start) || !text.is_char_boundary(start) { start -= 1; }
        let longest = old.len().min(text.len()) - start;
        let mut tail = old.bytes().rev().zip(text.bytes().rev()).take(longest).take_while(|(a, b)| a == b).count();
        while !old.is_char_boundary(old.len() - tail) || !text.is_char_boundary(text.len() - tail) { tail -= 1; }
        let (old_end, new_end) = (old.len() - tail, text.len() - tail);
        let caret = self.buffer.caret();
        let caret = if caret <= start { caret } else if caret >= old_end { caret + new_end - old_end } else { new_end };
        (start..old_end, new_end, caret)
    }

    /// Takes in text changed on disk as one undoable edit, keeping the caret
    /// on the same text where it can.
    pub fn reload(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.buffer.text() == text { return; }
        let (range, new_end, caret) = self.diff(text);
        let start = range.start;
        self.buffer.replace_selecting(range, &text[start..new_end], caret..caret, EditKind::Other);
        self.changed(cx);
    }

    /// Takes in text Thesio generated, outside the undo history and without
    /// announcing a change: it is already saved.
    pub fn sync_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.buffer.text() == text { return; }
        let (range, new_end, caret) = self.diff(text);
        let start = range.start;
        self.buffer.replace_silently(range, &text[start..new_end], caret);
        if self.typst { self.spans = highlights(self.buffer.text()); }
        self.refold();
        self.list.reset(self.buffer.line_count());
        cx.notify();
    }

    /// Inserts a citation at the caret, spaced off the word before it.
    pub fn insert_citation(&mut self, key: &str, cx: &mut Context<Self>) {
        let before = self.buffer.text()[..self.buffer.sel.start].chars().next_back();
        let glue = if before.is_some_and(|c| !c.is_whitespace() && !"([{".contains(c)) { " " } else { "" };
        self.insert(&format!("{glue}@{key}"), EditKind::Other, cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.typst { self.spans = highlights(self.buffer.text()); }
        self.refold();
        self.list.reset(self.buffer.line_count());
        self.goal_x = None;
        self.reveal(cx);
        self.refresh_completion(false, cx);
        cx.emit(SourceEvent::Changed);
    }

    /// Follows the caret. Only typing opens the popup, so moving through
    /// existing citations stays quiet.
    fn refresh_completion(&mut self, typed: bool, cx: &mut Context<Self>) {
        let caret = self.buffer.caret();
        let found = (self.typst && self.buffer.sel.is_empty() && self.marked.is_none()).then(|| citation_at(self.buffer.text(), caret)).flatten();
        let Some(range) = found else {
            self.settled = None;
            self.completion = None;
            return;
        };
        if self.settled == Some(range.start) { return; }
        let query = self.buffer.text()[range.start + 1..caret].to_lowercase();
        match &mut self.completion {
            Some(open) => {
                if open.query != query { open.selected = 0; }
                (open.range, open.query) = (range, query);
            }
            None if typed => {
                self.completion = Some(Completion { range, query, selected: 0, waited: 0 });
                cx.emit(SourceEvent::Citing);
            }
            None => {}
        }
    }

    /// The sources matching the open popup, keys that start with the query first.
    fn completions(&self) -> Vec<&Row> {
        let Some(open) = &self.completion else { return vec![] };
        let mut rows: Vec<&Row> = self.sources.iter().filter(|row| open.query.is_empty() || matches(row, &open.query)).collect();
        rows.sort_by_key(|row| !row.key.to_lowercase().starts_with(&open.query));
        rows.truncate(COMPLETION_ROWS);
        rows
    }

    fn complete(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(key) = self.completions().get(index).map(|row| row.key.clone()) else { return };
        let Some(open) = self.completion.take() else { return };
        self.settled = Some(open.range.start);
        self.buffer.replace(open.range, &format!("@{key}"), EditKind::Other);
        self.changed(cx);
        cx.emit(SourceEvent::Cited);
    }

    fn completion_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let shown = self.completions().len();
        let Some(open) = &mut self.completion else { return };
        if shown == 0 { return; }
        open.selected = (open.selected + if down { 1 } else { shown - 1 }) % shown;
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

    /// Where the row holding `offset` was last painted, in window coordinates.
    fn painted_at(&self, offset: usize) -> Option<Point<Pixels>> {
        let line = self.buffer.line_of(offset);
        let painted = self.painted.borrow();
        let row = painted.get(&line)?;
        Some(row.bounds.origin + row.layout.as_ref()?.position_for_index(offset - self.buffer.line_range(line).start, px(LINE_HEIGHT))?)
    }

    fn render_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.focus.is_focused(window) { return None; }
        let open = self.completion.as_ref()?;
        let (start, selected) = (open.range.start, open.selected.min(COMPLETION_ROWS - 1));
        let rows: Vec<Row> = self.completions().into_iter().cloned().collect();
        if rows.is_empty() { return None; }
        let Some(at) = self.painted_at(start) else {
            // A new `@` has no layout until the frame after it was typed.
            let open = self.completion.as_mut()?;
            if open.waited < COMPLETION_WAIT {
                open.waited += 1;
                cx.on_next_frame(window, |_, _, cx| cx.notify());
            }
            return None;
        };
        let rows = rows.into_iter().enumerate().map(|(i, row)| {
            let on = i == selected;
            let detail = [row.authors.as_str(), row.year.as_str(), row.key.as_str()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ");
            let title = if row.title.is_empty() { row.key.clone() } else { row.title.clone() };
            div().id(("completion", i)).mx(px(4.)).px(px(8.)).py(px(4.)).flex().flex_col().rounded(px(ROW_RADIUS)).cursor_pointer().overflow_hidden()
                .when(on, |d| d.bg(Theme::menu_highlight()))
                .when(!on, |d| d.hover(|s| s.bg(Theme::fill_hover())))
                .child(div().text_size(px(13.)).line_height(px(18.)).truncate().child(ui_text(title)))
                .child(div().text_size(px(12.)).line_height(px(16.)).truncate().text_color(Theme::text_caption()).child(ui_text(detail)))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(move |e, _, _, cx| e.complete(i, cx)))
        }).collect::<Vec<_>>();
        let menu = popover().id("completion").occlude().w(px(self.completion_w)).py(px(MENU_PAD_Y)).flex().flex_col().shadow(theme::shadow_panel()).children(rows);
        Some(deferred(anchored().position_mode(AnchoredPositionMode::Window).position(at).offset(point(px(0.), px(LINE_HEIGHT + 4.))).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(1).into_any_element())
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to_reveal_item(self.buffer.line_of(self.buffer.caret()));
        cx.notify();
    }

    fn go(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        let offset = self.outside_fold(offset);
        if extend { self.buffer.select_to(offset); } else { self.buffer.move_to(offset); }
        self.goal_x = None;
        self.reveal(cx);
        self.refresh_completion(false, cx);
    }

    fn delete_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let range = if self.buffer.sel.is_empty() {
            let caret = self.buffer.caret();
            caret.min(offset)..caret.max(offset)
        } else { self.buffer.sel.clone() };
        if !range.is_empty() {
            self.buffer.replace(range, "", EditKind::Deleting);
            self.changed(cx);
        }
    }

    fn insert(&mut self, text: &str, kind: EditKind, cx: &mut Context<Self>) {
        self.buffer.replace(self.buffer.sel.clone(), text, kind);
        self.changed(cx);
    }

    /// The offset one visual row away, keeping the column.
    fn vertical(&mut self, down: bool) -> usize {
        let caret = self.buffer.caret();
        let line = self.buffer.line_of(caret);
        let range = self.buffer.line_range(line);
        let height = px(LINE_HEIGHT);
        let painted = self.painted.borrow();
        let current = painted.get(&line).and_then(|p| p.layout.as_ref());
        let here = current.and_then(|l| l.position_for_index(caret - range.start, height));
        let x = self.goal_x.or(here.map(|p| p.x)).unwrap_or(px(0.));
        // A move inside a wrapped line stays on it.
        if let (Some(here), Some(layout)) = (here, current) {
            let y = if down { here.y + height } else { here.y - height };
            if y >= px(0.) && y < layout.size(height).height {
                let offset = range.start + layout.closest_index_for_position(point(x, y), height).unwrap_or_else(|i| i);
                drop(painted);
                self.goal_x = Some(x);
                return offset;
            }
        }
        let mut target = if down { line + 1 } else { line.wrapping_sub(1) };
        if let Some((_, lines)) = &self.folded && lines.contains(&target) {
            target = if down { lines.end } else { lines.start.wrapping_sub(1) };
        }
        if target >= self.buffer.line_count() {
            return if down { self.buffer.text().len() } else { 0 };
        }
        let target_range = self.buffer.line_range(target);
        let offset = match painted.get(&target).and_then(|p| p.layout.as_ref()) {
            Some(layout) => {
                let y = if down { px(0.) } else { layout.size(height).height - height };
                target_range.start + layout.closest_index_for_position(point(x, y), height).unwrap_or_else(|i| i)
            }
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
        let row = painted.iter().min_by_key(|(_, p)| {
            let (top, bottom) = (p.bounds.top(), p.bounds.bottom());
            if position.y < top { top - position.y } else if position.y >= bottom { position.y - bottom + px(1.) } else { px(0.) }
        });
        let Some((line, row)) = row else { return self.buffer.text().len() };
        let range = self.buffer.line_range(*line);
        let Some(layout) = &row.layout else { return range.start };
        let local = point(position.x - row.bounds.left(), (position.y - row.bounds.top()).clamp(px(0.), row.bounds.size.height - px(1.)));
        range.start + layout.closest_index_for_position(local, px(LINE_HEIGHT)).unwrap_or_else(|i| i)
    }

    fn on_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.completion = None;
        let offset = self.outside_fold(self.offset_at(event.position));
        if event.click_count == 2 {
            let word = self.buffer.word_at(offset);
            self.buffer.move_to(word.start);
            self.buffer.select_to(word.end);
        } else if event.click_count >= 3 {
            let range = self.buffer.line_range(self.buffer.line_of(offset));
            self.buffer.move_to(range.start);
            self.buffer.select_to(range.end);
        } else if event.modifiers.shift {
            self.buffer.select_to(offset);
        } else {
            self.buffer.move_to(offset);
        }
        self.selecting = true;
        self.goal_x = None;
        cx.notify();
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.buffer.select_to(self.outside_fold(self.offset_at(event.position)));
            cx.notify();
        }
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
        if let Some((_, lines)) = &self.folded && lines.contains(&line) {
            if line != lines.start { return div().h(px(0.)).into_any_element(); }
            return div().h(px(LINE_HEIGHT + 8.)).pt(px(4.)).pb(px(4.)).child(
                div().size_full().px(px(8.)).flex().items_center().rounded(px(6.)).bg(Theme::fill_hover())
                    .text_size(px(12.)).text_color(Theme::text_caption()).child("Bibliography, generated by Thesio")
            ).into_any_element();
        }
        let range = self.buffer.line_range(line);
        let sel = &self.buffer.sel;
        let selection = (!sel.is_empty() && sel.start <= range.end && sel.end > range.start)
            .then(|| sel.start.max(range.start) - range.start..sel.end.min(range.end) - range.start);
        let caret = self.buffer.caret();
        let caret = (self.focus.is_focused(window) && sel.is_empty() && caret >= range.start && caret <= range.end).then(|| caret - range.start);
        LineElement {
            line,
            text: SharedString::from(self.buffer.text()[range.clone()].to_string()),
            runs: self.runs(&range),
            selection,
            through_newline: sel.end > range.end,
            caret,
            painted: self.painted.clone(),
        }.into_any_element()
    }

    fn to16(&self, offset: usize) -> usize { self.buffer.text()[..offset].encode_utf16().count() }

    fn from16(&self, offset16: usize) -> usize {
        let mut units = 0;
        for (i, ch) in self.buffer.text().char_indices() {
            if units >= offset16 { return i; }
            units += ch.len_utf16();
        }
        self.buffer.text().len()
    }

    fn range8(&self, range: &Range<usize>) -> Range<usize> { self.from16(range.start)..self.from16(range.end) }
}

fn tag_color(tag: SyntaxTag) -> Hsla {
    match tag {
        SyntaxTag::Comment => c(craft::N500),
        SyntaxTag::Punctuation | SyntaxTag::ListMarker | SyntaxTag::Escape => c(craft::N600),
        SyntaxTag::Heading | SyntaxTag::Keyword | SyntaxTag::Label | SyntaxTag::Ref | SyntaxTag::Link => Theme::primary(),
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
        self.buffer.select_all();
        cx.notify();
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.sel.is_empty() { cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().to_string())); }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.sel.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().to_string()));
            self.insert("", EditKind::Other, cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert(&text.replace("\r\n", "\n"), EditKind::Other, cx);
        }
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) { if self.buffer.undo() { self.changed(cx); } }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) { if self.buffer.redo() { self.changed(cx); } }
    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.line_start();
        let indent: String = self.buffer.text()[start..self.buffer.sel.start.max(start)].chars().take_while(|ch| *ch == ' ' || *ch == '\t').collect();
        self.insert(&format!("\n{indent}"), EditKind::Other, cx);
    }
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) { self.insert("  ", EditKind::Typing, cx); }
}

impl EntityInputHandler for SourceEditor {
    fn text_for_range(&mut self, range: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let range = self.range8(&range);
        *actual = Some(self.to16(range.start)..self.to16(range.end));
        Some(self.buffer.text()[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.to16(self.buffer.sel.start)..self.to16(self.buffer.sel.end), reversed: self.buffer.reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.to16(r.start)..self.to16(r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) { self.marked = None; }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let range = range.map(|r| self.range8(&r)).or(self.marked.take()).unwrap_or(self.buffer.sel.clone());
        self.marked = None;
        self.buffer.replace(range, text, EditKind::Typing);
        self.changed(cx);
        self.refresh_completion(true, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, new_sel: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        let range = range.map(|r| self.range8(&r)).or(self.marked.clone()).unwrap_or(self.buffer.sel.clone());
        let base = range.start;
        let byte_at = |units: usize| {
            let mut seen = 0;
            for (i, ch) in text.char_indices() {
                if seen >= units { return i; }
                seen += ch.len_utf16();
            }
            text.len()
        };
        let sel = new_sel.map_or(base + text.len()..base + text.len(), |sel| base + byte_at(sel.start)..base + byte_at(sel.end));
        self.buffer.replace_selecting(range, text, sel, EditKind::Typing);
        self.marked = (!text.is_empty()).then(|| base..base + text.len());
        self.changed(cx);
    }

    fn bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let offset = self.range8(&range).start;
        let line = self.buffer.line_of(offset);
        let painted = self.painted.borrow();
        let row = painted.get(&line)?;
        let local = row.layout.as_ref().and_then(|l| l.position_for_index(offset - self.buffer.line_range(line).start, px(LINE_HEIGHT))).unwrap_or_default();
        Some(Bounds::new(row.bounds.origin + local, size(px(1.), px(LINE_HEIGHT))))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.to16(self.offset_at(point)))
    }
}

impl Render for SourceEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Before `painted` is cleared: the popup sits on last frame's layout.
        let completion = self.render_completion(window, cx);
        let mut context = KeyContext::new_with_defaults();
        context.add("SourceEditor");
        if completion.is_some() { context.add("completing"); }
        self.painted.borrow_mut().clear();
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
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_up))
            .child(div().size_full().pl(px(PAD_X)).pr(px(PAD_X)).pt(px(16.)).child(list(self.list.clone(), cx.processor(Self::render_line)).size_full()))
            .child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
            }).absolute().size_full())
            .children(completion)
    }
}

struct LineElement {
    line: usize,
    text: SharedString,
    runs: Vec<TextRun>,
    selection: Option<Range<usize>>,
    through_newline: bool,
    caret: Option<usize>,
    painted: Painted,
}

impl IntoElement for LineElement {
    type Element = Self;
    fn into_element(self) -> Self { self }
}

impl Element for LineElement {
    type RequestLayoutState = Rc<RefCell<Option<WrappedLine>>>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> { None }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, _: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let state: Self::RequestLayoutState = Default::default();
        let (slot, text, runs) = (state.clone(), self.text.clone(), self.runs.clone());
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let id = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.or(match available.width { AvailableSpace::Definite(w) => Some(w), _ => None });
            let height = px(LINE_HEIGHT);
            if text.is_empty() { return size(width.unwrap_or(px(0.)), height); }
            let lines = window.text_system().shape_text(text.clone(), px(FONT_SIZE), &runs, width, None).unwrap_or_default();
            let line = lines.into_iter().next();
            let measured = line.as_ref().map_or(height, |l| l.size(height).height);
            *slot.borrow_mut() = line;
            size(width.unwrap_or(px(0.)), measured)
        });
        (id, state)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Window, _: &mut App) {}

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, state: &mut Self::RequestLayoutState, _: &mut (), window: &mut Window, cx: &mut App) {
        let layout = state.borrow().clone();
        let height = px(LINE_HEIGHT);
        let position = |index: usize| layout.as_ref().and_then(|l| l.position_for_index(index, height)).unwrap_or_default();
        if let Some(selection) = &self.selection {
            let (a, b) = (position(selection.start), position(selection.end));
            let mut y = a.y;
            while y <= b.y {
                let left = if y == a.y { a.x } else { px(0.) };
                let right = if y == b.y { b.x + if self.through_newline { px(7.) } else { px(0.) } } else { bounds.size.width };
                window.paint_quad(fill(Bounds::from_corners(bounds.origin + point(left, y), bounds.origin + point(right, y + height)), Theme::selection()));
                y += height;
            }
        }
        if let Some(line) = &layout {
            let _ = line.paint(bounds.origin, height, TextAlign::Left, None, window, cx);
        }
        if let Some(caret) = self.caret {
            let at = bounds.origin + position(caret);
            window.paint_quad(fill(Bounds::new(at + point(px(0.), px(2.)), size(px(2.), height - px(4.))), Theme::primary()));
        }
        self.painted.borrow_mut().insert(self.line, PaintedLine { bounds, layout });
    }
}
