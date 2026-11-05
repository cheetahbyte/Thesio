//! The text field: one line by default, or several that wrap. Its text and
//! selection are a `Buffer`, and it shapes and paints with `crate::text`.

use crate::text::{CaretMark, LineMarks, PaintedLine, from_utf16, paint_line, plain_newlines, range_from_utf16, range_to_utf16, request_line, to_utf16};
use crate::theme::{Theme, UI_FONT};
use std::{cell::RefCell, rc::Rc};
use gpui::*;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use thesio_core::buffer::Buffer;

actions!(
    field,
    [Backspace, Delete, Left, Right, SelectLeft, SelectRight, Home, End, SelectAll, Copy, Cut, Paste, Submit, ShiftEnter, Cancel, Up, Down, Swallow, Tab, ShiftTab]
);

pub fn bind_keys(cx: &mut App) {
    let c = Some("TextField");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("cmd-left", Home, c),
        KeyBinding::new("cmd-right", End, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("shift-enter", ShiftEnter, c),
        KeyBinding::new("escape", Cancel, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("ctrl-p", Up, c),
        KeyBinding::new("ctrl-n", Down, c),
        KeyBinding::new("tab", Tab, c),
        KeyBinding::new("shift-tab", ShiftTab, c),
        KeyBinding::new("cmd-z", Swallow, c),
        KeyBinding::new("cmd-shift-z", Swallow, c),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFieldEvent {
    Changed,
    Submit,
    Cancel,
    Up,
    Down,
    /// Tab / Shift+Tab when `tab_events` is set (moving between fields).
    Tab,
    ShiftTab,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub struct TextField {
    focus: FocusHandle,
    buffer: Buffer,
    marked: Option<Range<usize>>,
    placeholder: SharedString,
    painted: Rc<RefCell<Option<PaintedLine>>>,
    id: u64,
    pub size: f32,
    pub line_height: f32,
    /// Read when drawn, so a change of theme applies.
    pub placeholder_color: fn() -> Hsla,
    /// Wrap long text onto more lines instead of scrolling it.
    pub wrap: bool,
    /// Enter and pasted newlines insert line breaks; Up/Down move by line.
    pub multiline: bool,
    /// Tab / Shift+Tab emit events instead of reaching the parent.
    pub tab_events: bool,
    selecting: bool,
    /// Typing hides the mouse pointer until it moves.
    pointer_hidden: bool,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextField {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        TextField {
            focus: cx.focus_handle(),
            buffer: Buffer::default(),
            marked: None,
            placeholder: placeholder.into(),
            painted: Default::default(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            // macOS's control text: 13pt.
            size: 13.,
            line_height: 20.,
            placeholder_color: Theme::text_placeholder,
            wrap: false,
            multiline: false,
            tab_events: false,
            selecting: false,
            pointer_hidden: false,
        }
    }

    pub fn text(&self) -> &str { self.buffer.text() }

    pub fn set_text(&mut self, s: &str, cx: &mut Context<Self>) {
        self.buffer.replace_silently(0..self.text().len(), s, s.len()..s.len());
        self.marked = None;
        cx.notify();
    }

    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    fn move_to(&mut self, o: usize, cx: &mut Context<Self>) {
        self.buffer.move_to(o);
        cx.notify();
    }

    fn select_to(&mut self, o: usize, cx: &mut Context<Self>) {
        self.buffer.select_to(o);
        cx.notify();
    }

    /// Replaces text outside the buffer's undo history: fields have no undo.
    fn replace(&mut self, range: Range<usize>, s: &str, cx: &mut Context<Self>) {
        let s = if self.multiline { plain_newlines(s) } else { s.replace(['\n', '\r'], " ") };
        let end = range.start + s.len();
        self.buffer.replace_silently(range, &s, end..end);
        cx.emit(TextFieldEvent::Changed);
        cx.notify();
    }

    /// Deletes the selection, or up to `to` without one.
    fn delete_to(&mut self, to: usize, cx: &mut Context<Self>) {
        if self.buffer.sel.is_empty() { self.buffer.select_to(to); }
        self.replace(self.buffer.sel.clone(), "", cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.text().is_empty() { return; }
        self.delete_to(self.buffer.prev_grapheme(self.buffer.caret()), cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.delete_to(self.buffer.next_grapheme(self.buffer.caret()), cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.buffer.sel.is_empty() { self.buffer.prev_grapheme(self.buffer.caret()) } else { self.buffer.sel.start };
        self.move_to(o, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.buffer.sel.is_empty() { self.buffer.next_grapheme(self.buffer.caret()) } else { self.buffer.sel.end };
        self.move_to(o, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.buffer.prev_grapheme(self.buffer.caret()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.buffer.next_grapheme(self.buffer.caret()), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.text().len(), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all_text(cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.sel.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.sel.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().to_string()));
            self.replace(self.buffer.sel.clone(), "", cx);
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace(self.buffer.sel.clone(), &t, cx);
        }
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            return self.replace(self.buffer.sel.clone(), "\n", cx);
        }
        cx.emit(TextFieldEvent::Submit);
    }

    fn shift_enter(&mut self, _: &ShiftEnter, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace(self.buffer.sel.clone(), "\n", cx);
        }
    }

    fn swallow(&mut self, _: &Swallow, _: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            cx.propagate();
        }
    }

    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        match (self.tab_events, self.multiline) {
            (true, _) => cx.emit(TextFieldEvent::Tab),
            (false, false) => cx.propagate(),
            _ => {}
        }
    }

    fn shift_tab(&mut self, _: &ShiftTab, _: &mut Window, cx: &mut Context<Self>) {
        match (self.tab_events, self.multiline) {
            (true, _) => cx.emit(TextFieldEvent::ShiftTab),
            (false, false) => cx.propagate(),
            _ => {}
        }
    }

    /// Move the caret a line up (`-1`) or down; past the first or last line
    /// it goes to the start or end.
    fn move_line(&mut self, dir: f32, cx: &mut Context<Self>) {
        let Some(line) = self.painted.borrow().clone() else { return };
        let at = line.position(self.buffer.caret()).unwrap_or_default();
        let y = at.y + line.line_height * (0.5 + dir);
        let target = if y < px(0.) { 0 } else if y >= line.height() { self.text().len() } else { line.index_at(point(at.x, y)).min(self.text().len()) };
        self.move_to(target, cx);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextFieldEvent::Cancel);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            return self.move_line(-1., cx);
        }
        cx.emit(TextFieldEvent::Up);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            return self.move_line(1., cx);
        }
        cx.emit(TextFieldEvent::Down);
    }

    fn index_for(&self, p: Point<Pixels>) -> usize {
        self.painted.borrow().as_ref().map_or(0, |line| line.index_for(p).min(self.text().len()))
    }

    fn on_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        let i = self.index_for(ev.position);
        if ev.click_count >= 2 {
            self.select_all_text(cx);
            return;
        }
        self.selecting = true;
        if ev.modifiers.shift { self.select_to(i, cx) } else { self.move_to(i, cx) }
    }

    fn on_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting && ev.pressed_button == Some(MouseButton::Left) {
            let i = self.index_for(ev.position);
            self.select_to(i, cx);
        }
    }

    fn on_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(&mut self, range: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let r = range_from_utf16(self.text(), &range);
        actual.replace(range_to_utf16(self.text(), &r));
        Some(self.text()[r].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: range_to_utf16(self.text(), &self.buffer.sel), reversed: self.buffer.reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| range_to_utf16(self.text(), r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, s: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.pointer_hidden = true;
        let r = range.map(|r| range_from_utf16(self.text(), &r)).or(self.marked.clone()).unwrap_or(self.buffer.sel.clone());
        self.marked = None;
        self.replace(r, s, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, s: &str, new_sel: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        self.pointer_hidden = true;
        let r = range.map(|r| range_from_utf16(self.text(), &r)).or(self.marked.clone()).unwrap_or(self.buffer.sel.clone());
        let base = r.start;
        let sel = new_sel.map_or(base + s.len()..base + s.len(), |sel| base + from_utf16(s, sel.start)..base + from_utf16(s, sel.end));
        self.buffer.replace_silently(r, s, sel);
        self.marked = (!s.is_empty()).then(|| base..base + s.len());
        cx.emit(TextFieldEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let r = range_from_utf16(self.text(), &range);
        let painted = self.painted.borrow();
        let line = painted.as_ref()?;
        let (a, b) = (line.position(r.start)?, line.position(r.end)?);
        Some(Bounds::from_corners(line.bounds.origin + a, line.bounds.origin + point(b.x, b.y + line.line_height)))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(to_utf16(self.text(), self.index_for(p)))
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let sel = &self.buffer.sel;
        let el = FieldText {
            text: if self.text().is_empty() { self.placeholder.clone() } else { self.text().to_string().into() },
            color: if self.text().is_empty() { (self.placeholder_color)() } else { Theme::text() },
            size: self.size,
            line_height: self.line_height,
            wrap: self.wrap,
            selection: (focused && !sel.is_empty()).then(|| sel.clone()),
            caret: (focused && sel.is_empty()).then(|| self.buffer.caret()),
            marked: self.marked.clone(),
            painted: self.painted.clone(),
        };
        let entity = cx.entity();
        let focus = self.focus.clone();
        let pointer_hidden = self.pointer_hidden;
        div()
            .id(("text-field", self.id))
            .track_focus(&self.focus)
            .key_context("TextField")
            .cursor(CursorStyle::IBeam)
            .relative()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::shift_enter))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::swallow))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::shift_tab))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_up))
            .text_color(Theme::text())
            .child(el)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                        if pointer_hidden {
                            window.set_window_cursor_style(CursorStyle::None);
                            let e = entity.clone();
                            window.on_mouse_event(move |_: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    e.update(cx, |f, cx| {
                                        f.pointer_hidden = false;
                                        cx.notify();
                                    });
                                }
                            });
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

struct FieldText {
    text: SharedString,
    color: Hsla,
    size: f32,
    line_height: f32,
    wrap: bool,
    selection: Option<Range<usize>>,
    caret: Option<usize>,
    marked: Option<Range<usize>>,
    painted: Rc<RefCell<Option<PaintedLine>>>,
}

impl IntoElement for FieldText {
    type Element = Self;
    fn into_element(self) -> Self { self }
}

impl Element for FieldText {
    type RequestLayoutState = crate::text::Shaped;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> { None }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, _: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut run = TextRun { len: self.text.len(), font: font(UI_FONT), color: self.color, background_color: None, underline: None, strikethrough: None };
        if let Some(marked) = &self.marked && !marked.is_empty() {
            run.underline = Some(UnderlineStyle { thickness: px(1.), color: None, wavy: false });
        }
        request_line(self.text.clone(), vec![run], px(self.size), px(self.line_height), self.wrap, window)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Window, _: &mut App) {}

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, state: &mut Self::RequestLayoutState, _: &mut (), window: &mut Window, cx: &mut App) {
        let line = PaintedLine { bounds, layout: state.borrow().clone(), line_height: px(self.line_height) };
        let marks = LineMarks { selection: self.selection.clone(), selection_tail: px(0.), caret: self.caret.map(|at| CaretMark { at, width: px(1.5), inset: px(0.) }) };
        paint_line(&line, &marks, window, cx);
        *self.painted.borrow_mut() = Some(line);
    }
}
