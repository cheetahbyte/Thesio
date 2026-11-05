//! What floats over the window: one menu or dialog at a time. Opening one
//! replaces the last, Esc closes it, and focus goes back where it was.

use crate::app::{AfterSave, Confirm, RowMenu, View, Workspace};
use crate::citations::lookup::Lookup;
use crate::citations::picker::CitePicker;
use crate::citations::tags::{TagEditor, TagRename};
use crate::history::History;
use crate::papers::quote::QuoteDialog;
use crate::sidebar::ROW_MENU_W;
use crate::tree::MoveDialog;
use gpui::*;

pub enum Overlay {
    RowMenu(RowMenu),
    /// The sidebar list's own menu, and where it opened.
    FilesMenu(Point<Pixels>),
    DocMenu,
    StyleMenu,
    TrashMenu { id: String, position: Point<Pixels>, corner: Corner },
    LibraryMenu,
    /// An entry's context menu.
    EntryMenu { key: String, position: Point<Pixels> },
    /// A filter chip's menu.
    TagMenu { tag: String, position: Point<Pixels> },
    TagRename(TagRename),
    TagEditor(TagEditor),
    /// The inspector's type menu, and where it opened.
    TypeMenu(Point<Pixels>),
    Search,
    WordCount,
    History(History),
    Move(MoveDialog),
    Picker(CitePicker),
    Lookup(Lookup),
    Quote(QuoteDialog),
    Confirm(Confirm),
}

impl Overlay {
    /// Whether it reaches past `edge`, the left of the main pane. Menus opened
    /// on the sidebar mostly stay clear of it.
    fn covers_pane(&self, edge: f32) -> bool {
        let past = |position: Point<Pixels>| f32::from(position.x) + ROW_MENU_W > edge;
        match self {
            Overlay::RowMenu(menu) => past(menu.pos),
            Overlay::FilesMenu(position) => past(*position),
            _ => true,
        }
    }
}

impl Workspace {
    /// Shows `overlay` in place of any other. The first one remembers the focus.
    pub(crate) fn open_overlay(&mut self, overlay: Overlay, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay.is_none() {
            self.overlay_return = window.focused(cx);
            // Keys only reach the workspace, and so Esc, along a focus path.
            if self.overlay_return.is_none() { window.focus(&self.focus); }
        }
        self.overlay = Some(overlay);
        cx.notify();
    }

    /// Closes the overlay and returns it. Focus goes back where it was, or
    /// with nothing to go back to, to the open document.
    pub(crate) fn take_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Overlay> {
        let overlay = self.overlay.take()?;
        match (self.overlay_return.take().filter(|focus| *focus != self.focus), &self.editor) {
            (Some(focus), _) => window.focus(&focus),
            (None, Some(editor)) if self.view == View::Doc => editor.read(cx).focus(window),
            // A closed dialog's field takes the keys with it otherwise.
            _ => window.focus(&self.focus),
        }
        cx.notify();
        Some(overlay)
    }

    pub(crate) fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_overlay(window, cx);
    }

    /// Closes the overlay for what is replacing the page under it, which takes the focus.
    pub(crate) fn clear_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = None;
        self.overlay_return = None;
        cx.notify();
    }

    /// Esc: closes the overlay, then leaves Selection mode, even from the editor.
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay.is_some() {
            self.close_overlay(window, cx);
        } else if self.batch.is_some() && self.rename.is_none() {
            self.batch = None;
            cx.notify();
        } else {
            return;
        }
        cx.stop_propagation();
    }

    pub(crate) fn ask_save_failed(&mut self, then: AfterSave, window: &mut Window, cx: &mut Context<Self>) {
        self.open_overlay(Overlay::Confirm(Confirm::SaveFailed(then)), window, cx);
    }

    pub(crate) fn search_open(&self) -> bool {
        matches!(self.overlay, Some(Overlay::Search))
    }

    /// The viewer goes under what GPUI draws over the pane, or it would hide it.
    pub(crate) fn paper_covered(&self, window: &Window) -> bool {
        let edge = if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        self.overlay.as_ref().is_some_and(|overlay| overlay.covers_pane(edge))
    }

    pub(crate) fn render_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        match self.overlay.as_ref()? {
            Overlay::RowMenu(menu) => self.render_row_menu(menu, cx),
            Overlay::FilesMenu(position) => Some(self.render_files_menu(*position, cx)),
            Overlay::DocMenu => self.render_doc_menu(window, cx),
            Overlay::StyleMenu => self.render_style_menu(window, cx),
            Overlay::TrashMenu { id, position, corner } => Some(self.render_trash_menu(id, *position, *corner, cx)),
            Overlay::LibraryMenu => Some(self.render_library_menu(window, cx)),
            Overlay::EntryMenu { key, position } => Some(self.render_entry_menu(key, *position, cx)),
            Overlay::TagMenu { tag, position } => Some(self.render_tag_menu(tag, *position, cx)),
            Overlay::TagRename(rename) => Some(self.render_tag_rename(rename, window, cx)),
            Overlay::TagEditor(editor) => self.render_tag_editor(editor, window, cx),
            Overlay::TypeMenu(position) => self.render_type_menu(*position, cx),
            Overlay::Search => Some(self.render_search(window, cx)),
            Overlay::WordCount => self.render_word_count(cx),
            Overlay::History(history) => Some(self.render_history(history, cx)),
            Overlay::Move(dialog) => Some(self.render_move_dialog(dialog, cx)),
            Overlay::Picker(picker) => Some(self.render_cite_picker(picker, window, cx)),
            Overlay::Lookup(lookup) => Some(self.render_lookup(lookup, window, cx)),
            Overlay::Quote(quote) => Some(self.render_quote_dialog(quote, window, cx)),
            Overlay::Confirm(confirm) => Some(self.render_confirm(confirm, cx)),
        }
    }
}
