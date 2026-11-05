//! The Library: the citation library and everything that shows or edits it.

mod bibliography;
pub(crate) mod inspector;
pub(crate) mod lookup;
pub(crate) mod picker;
pub(crate) mod table;
pub(crate) mod tags;

use crate::app::{View, Workspace};
use crate::document::Edit;
use crate::input::{TextField, TextFieldEvent};
use crate::library::{Library, Row};
use crate::source::SourceEditor;
use crate::theme::Theme;
use crate::ui::ToastKind;
use gpui::*;
use inspector::Details;
use std::path::Path;
use table::{Table, cell};
use thesio_core::bibliography::LIBRARY_FILE;
use thesio_core::paths::natural_cmp;
use thesio_core::references::note_link;

pub struct Citations {
    pub library: Option<Library>,
    /// Why `library.yaml` could not be read. Nothing writes to it meanwhile.
    pub error: Option<String>,
    table: Table,
    details: Details,
    tag_filter: Option<String>,
    _subs: Vec<Subscription>,
}

impl Citations {
    pub(crate) fn close_inspector(&mut self) { self.details.panel = None; }

    pub fn new(root: &Path, window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let search = cx.new(|cx| {
            let mut f = TextField::new("Search library", cx);
            f.placeholder_color = Theme::text_disabled;
            f
        });
        let subs = vec![
            cx.subscribe_in(&search, window, |w, _, ev: &TextFieldEvent, window, cx| if matches!(ev, TextFieldEvent::Changed) { w.table_changed(window, cx); }),
        ];
        let (library, error) = match Library::load(root) {
            Ok(library) => (Some(library), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let details = Details { panel: None, hidden: crate::preferences::prefs().library_details_hidden };
        Self { library, error, table: Table::new(search, cx), details, tag_filter: None, _subs: subs }
    }
}

impl Workspace {
    pub(crate) fn reload_library(&mut self) {
        let result = Library::load(self.store.borrow().root());
        match result {
            Ok(library) => {
                self.citations.library = Some(library);
                self.citations.error = None;
            }
            Err(e) => {
                self.citations.library = None;
                self.citations.error = Some(e.to_string());
            }
        }
    }

    pub(crate) fn visible_rows(&self, query: &str) -> Vec<Row> {
        let Some(library) = &self.citations.library else { return vec![] };
        let (col, ascending) = self.citations.table.sort;
        let mut rows: Vec<Row> = library.rows().into_iter().filter(|r| query.is_empty() || r.matches(query)).collect();
        rows.sort_by(|a, b| {
            let order = natural_cmp(&cell(a, col), &cell(b, col));
            if ascending { order } else { order.reverse() }
        });
        rows
    }

    /// Follows what changes the table's rows: a filter on a tag that is gone
    /// ends, and the panel matches the selection.
    pub(crate) fn table_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view != View::Library { return; }
        let tags = self.citations.library.as_ref().map(Library::tags).unwrap_or_default();
        if !tags.iter().any(|(tag, _)| self.citations.tag_filter.as_ref() == Some(tag)) { self.citations.tag_filter = None; }
        self.sync_inspector(window, cx);
        cx.notify();
    }

    /// Gives the editor the library as it is on disk now.
    pub(crate) fn offer_sources(&mut self, editor: &Entity<SourceEditor>, cx: &mut Context<Self>) {
        self.reload_library();
        let rows = self.visible_rows("");
        // Paper notes have no path of their own here, so they offer no links.
        let file = self.current.clone().filter(|_| self.editor.as_ref() == Some(editor));
        let notes = file.map_or(vec![], |file| self.entries.iter().filter(|e| e.typst && e.id != file)
            .map(|e| (note_link(&file, &e.id, &e.title), e.title.clone(), e.id.clone())).collect());
        editor.update(cx, |e, cx| { e.sources = rows; e.notes = notes; cx.notify(); });
    }

    pub(crate) fn delete_entries(&mut self, keys: &[String], window: &mut Window, cx: &mut Context<Self>) {
        let Some(library) = &mut self.citations.library else { return };
        match library.delete_entries(keys) {
            Ok(()) => {
                for key in keys { self.citations.table.selected.remove(key); }
                self.refresh_after_library_change(cx);
                self.table_changed(window, cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not delete entry: {e}"), cx),
        }
        cx.notify();
    }

    /// Takes in a library version: the panel, documents, and open file follow it.
    pub(crate) fn library_restored(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.citations.details.panel = None;
        self.citations.table.selected.clear();
        self.refresh_after_library_change(cx);
        self.show_text(LIBRARY_FILE, text, Edit::Undoable, cx);
        self.table_changed(window, cx);
    }

    pub(crate) fn refresh_after_library_change(&mut self, cx: &mut Context<Self>) {
        self.refresh_entries(cx);
        self.sync_bibliographies(cx);
    }
}
