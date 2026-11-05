//! The Library page, its entry form, the citation picker, and keeping each
//! document's bibliography in step with the library.

use crate::app::{Confirm, View, Workspace};
use crate::input::{TextField, TextFieldEvent};
use crate::app::RowMenu;
use crate::library::{self, Added, Library, Mode, PAPERS_DIR, Row, note_file, paper_file};
use crate::source::SourceEditor;
use thesio_core::lookup::{self, Identifier};
use thesio_core::source_type::{Field, SOURCE_TYPES, SourceType};
use crate::store::natural_cmp;
use crate::theme::{self, Theme, craft};
use crate::trash::{dialog, list_header_text, list_line, menu_label, page_header, primary_button, scrim, secondary_button};
use crate::ui::{Icon, TipExt, TitleBarExt, ToastKind, field_well, icon, menu_glyph, menu_row, popover, primary_icon_button, search_well, toolbar_group, toolbar_icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use thesio_core::references::{bibliography_style, cited_keys, citations_of, included_files, note_link, rename_citation, set_bibliography_style};

const ROW_H: f32 = 32.;
/// Rows inset their cells, so cell text lines up with the page title.
const TABLE_X: f32 = 14.;
const CELL_X: f32 = 12.;
const CITE_W: f32 = 56.;
const INSPECTOR_W: f32 = 320.;
/// Narrower than this and the title gives up the columns it can spare.
const MIN_TITLE_W: f32 = 220.;
const MIN_COLUMN_W: f32 = 56.;
const MAX_COLUMN_W: f32 = 480.;
const ADD_W: f32 = 68.;
const PICKER_ROWS: usize = 8;
/// Fixed columns, widest to narrowest; the title takes what is left.
const COLS: [(Col, &str, Option<f32>); 6] = [
    (Col::Title, "Title", None),
    (Col::Authors, "Authors", Some(200.)),
    (Col::Year, "Year", Some(64.)),
    (Col::Kind, "Type", Some(132.)),
    (Col::Tags, "Tags", Some(120.)),
    (Col::Key, "Key", Some(150.)),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col { Title, Authors, Year, Kind, Tags, Key }

fn cell(row: &Row, col: Col) -> Cow<'_, str> {
    match col {
        Col::Title => (&row.title).into(),
        Col::Authors => (&row.authors).into(),
        Col::Year => (&row.year).into(),
        Col::Kind => (&row.kind).into(),
        Col::Tags => row.tags.join(", ").into(),
        Col::Key => (&row.key).into(),
    }
}

/// A borderless dialog action, beside the buttons but quieter than them.
fn plain_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div().id(id).flex_none().h(px(crate::ui::BUTTON_H)).px(px(4.)).flex().items_center().rounded(px(crate::ui::BUTTON_RADIUS)).cursor_pointer()
        .text_size(px(13.)).line_height(px(20.)).text_color(Theme::primary()).child(label)
}

pub(crate) fn matches(row: &Row, query: &str) -> bool {
    [&row.title, &row.authors, &row.year, &row.key].into_iter().chain(&row.tags).any(|text| text.to_lowercase().contains(query))
}

/// The panel beside the table that edits the selected entry.
pub struct Inspector {
    key: String,
    key_field: Entity<TextField>,
    fields: Vec<(Field, Entity<TextField>)>,
    error: Option<String>,
    /// The type menu, and where it opened.
    type_menu: Option<Point<Pixels>>,
    /// The documents citing this entry: file, times cited, and where it first is.
    cited_in: Vec<(String, usize, usize)>,
    _subs: Vec<Subscription>,
}

/// The popover that tags the selected entries.
pub struct TagEditor {
    keys: Vec<String>,
    field: Entity<TextField>,
    position: Point<Pixels>,
    _subs: Vec<Subscription>,
}

/// A column being resized: its name, and the pointer and width at the start.
#[derive(Clone, Copy)]
struct ColumnDrag {
    name: &'static str,
    start_x: Pixels,
    start_width: f32,
}

pub struct CitePicker {
    field: Entity<TextField>,
    selected: usize,
    purpose: PickFor,
    _subs: Vec<Subscription>,
}

/// What choosing a source in the picker does.
#[derive(Clone)]
enum PickFor {
    Cite,
    /// Copy this PDF in as the source's paper.
    Attach(PathBuf),
    /// Make this unlinked paper the source's.
    Link(String),
}

/// The Add by Identifier dialog.
pub struct Lookup {
    field: Entity<TextField>,
    busy: bool,
    /// What could not be added, one line each.
    problems: Vec<String>,
    _subs: Vec<Subscription>,
}

pub struct Citations {
    pub library: Option<Library>,
    /// Why `library.yaml` could not be read. Nothing writes to it meanwhile.
    pub error: Option<String>,
    search: Entity<TextField>,
    sort: (Col, bool),
    inspector: Option<Inspector>,
    /// The user closed the details panel.
    details_hidden: bool,
    picker: Option<CitePicker>,
    lookup: Option<Lookup>,
    /// The toolbar's Import and Export menu.
    menu: bool,
    /// The context menu: an entry's key and where it was opened.
    row_menu: Option<(String, Point<Pixels>)>,
    /// The selected rows, by key.
    selected: BTreeSet<String>,
    /// Where a shift-selection extends from.
    anchor: Option<String>,
    /// The row the arrow keys move from.
    cursor: Option<String>,
    /// Holds the keyboard for the table.
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    tag_filter: Option<String>,
    tag_editor: Option<TagEditor>,
    /// A filter chip's menu: its tag and where it was opened.
    tag_menu: Option<(String, Point<Pixels>)>,
    /// The tag being renamed, with its field and where it opened.
    tag_rename: Option<(String, Entity<TextField>, Point<Pixels>, Subscription)>,
    /// Widths of resized columns, by column name.
    widths: BTreeMap<String, f32>,
    column_drag: Option<ColumnDrag>,
    /// The Unpaywall email in Settings.
    pub email: Entity<TextField>,
    /// Papers still downloading.
    fetching: usize,
    _subs: Vec<Subscription>,
}

impl Citations {
    pub(crate) fn close_inspector(&mut self) { self.inspector = None; }

    pub fn new(root: &Path, cx: &mut Context<Workspace>) -> Self {
        let search = cx.new(|cx| {
            let mut f = TextField::new("Search library", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let email = cx.new(|cx| {
            let mut f = TextField::new("name@university.edu", cx);
            f.placeholder_color = Theme::text_disabled();
            f.set_text(&crate::settings::prefs().unpaywall_email, cx);
            f
        });
        let subs = vec![
            cx.subscribe(&search, |_, _, ev: &TextFieldEvent, cx| if matches!(ev, TextFieldEvent::Changed) { cx.notify(); }),
            cx.subscribe(&email, |w, field, ev: &TextFieldEvent, cx| if matches!(ev, TextFieldEvent::Changed) {
                let email = field.read(cx).text.trim().to_string();
                w.set_pref(|p| p.unpaywall_email = email, cx);
            }),
        ];
        let (library, error) = match Library::load(root) {
            Ok(library) => (Some(library), None),
            Err(e) => (None, Some(e.to_string())),
        };
        Self { library, error, search, sort: (Col::Title, true), inspector: None, details_hidden: crate::settings::prefs().library_details_hidden, picker: None, lookup: None, menu: false, row_menu: None, selected: BTreeSet::new(), anchor: None, cursor: None, focus: cx.focus_handle(), scroll: UniformListScrollHandle::new(), tag_filter: None, tag_editor: None, tag_menu: None, tag_rename: None, widths: crate::settings::prefs().library_columns, column_drag: None, email, fetching: 0, _subs: subs }
    }
}

impl Workspace {
    pub(crate) fn reload_library(&mut self) {
        match Library::load(&self.store.root()) {
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

    fn visible_rows(&self, query: &str) -> Vec<Row> {
        let Some(library) = &self.citations.library else { return vec![] };
        let (col, ascending) = self.citations.sort;
        let mut rows: Vec<Row> = library.rows().into_iter().filter(|r| query.is_empty() || matches(r, query)).collect();
        rows.sort_by(|a, b| {
            let order = natural_cmp(&cell(a, col), &cell(b, col));
            if ascending { order } else { order.reverse() }
        });
        rows
    }

    /// The table's rows, in order: the search and the tag filter applied.
    fn library_rows(&self, cx: &App) -> Vec<Row> {
        let query = self.citations.search.read(cx).text.trim().to_lowercase();
        let mut rows = self.visible_rows(&query);
        if let Some(tag) = &self.citations.tag_filter { rows.retain(|row| row.tags.contains(tag)); }
        rows
    }

    /// The selected keys among `rows`, in table order. Rows a search or filter
    /// hides stay out, so nothing acts on an entry the user cannot see.
    fn selected_in(&self, rows: &[Row]) -> Vec<String> {
        rows.iter().filter(|row| self.citations.selected.contains(&row.key)).map(|row| row.key.clone()).collect()
    }

    fn select_only(&mut self, key: &str) {
        self.citations.selected = BTreeSet::from([key.to_string()]);
        self.citations.anchor = Some(key.to_string());
        self.citations.cursor = Some(key.to_string());
    }

    /// Selects from the anchor through `key`.
    fn select_range(&mut self, key: &str, rows: &[Row]) {
        let position = |key: &str| rows.iter().position(|row| row.key == key);
        let Some(end) = position(key) else { return };
        let start = self.citations.anchor.as_deref().and_then(position).unwrap_or(end);
        self.citations.selected = rows[start.min(end)..=start.max(end)].iter().map(|row| row.key.clone()).collect();
        self.citations.cursor = Some(key.to_string());
    }

    fn click_row(&mut self, key: &str, modifiers: Modifiers, cx: &mut Context<Self>) {
        if modifiers.shift {
            self.select_range(key, &self.library_rows(cx));
        } else if modifiers.platform {
            if !self.citations.selected.remove(key) { self.citations.selected.insert(key.to_string()); }
            self.citations.anchor = Some(key.to_string());
            self.citations.cursor = Some(key.to_string());
        } else {
            self.select_only(key);
        }
        cx.notify();
    }

    fn library_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke.modifiers;
        let rows = self.library_rows(cx);
        match event.keystroke.key.as_str() {
            step @ ("up" | "down") => {
                let current = self.citations.cursor.as_deref().and_then(|key| rows.iter().position(|row| row.key == key));
                let next = match (step, current) {
                    ("up", Some(i)) => i.saturating_sub(1),
                    ("down", Some(i)) => (i + 1).min(rows.len().saturating_sub(1)),
                    _ => 0,
                };
                let Some(key) = rows.get(next).map(|row| row.key.clone()) else { return };
                if keys.shift { self.select_range(&key, &rows); } else { self.select_only(&key); }
                self.citations.scroll.scroll_to_item(next, ScrollStrategy::Top);
            }
            "a" if keys.platform => self.citations.selected = rows.iter().map(|row| row.key.clone()).collect(),
            "enter" => {
                if let [key] = self.selected_in(&rows).as_slice() { self.open_entry(Some(key.clone()), window, cx); }
            }
            "backspace" | "delete" => {
                let selected = self.selected_in(&rows);
                if !selected.is_empty() { self.confirm = Some(Confirm::DeleteEntries(selected)); }
            }
            "escape" => self.citations.selected.clear(),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn open_tag_editor(&mut self, keys: Vec<String>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut f = TextField::new("Add a tag", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![cx.subscribe_in(&field, window, |w, field, ev: &TextFieldEvent, window, cx| match ev {
            TextFieldEvent::Submit => {
                let tag = field.read(cx).text.clone();
                if tag.trim().is_empty() { return; }
                if w.set_tag(&tag, true, cx) { field.update(cx, |f, cx| f.set_text("", cx)); }
            }
            TextFieldEvent::Cancel => w.close_tag_editor(window, cx),
            _ => cx.notify(),
        })];
        window.focus(&field.focus_handle(cx));
        self.citations.tag_editor = Some(TagEditor { keys, field, position, _subs: subs });
        cx.notify();
    }

    fn close_tag_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.citations.tag_editor = None;
        window.focus(&self.citations.focus);
        cx.notify();
    }

    fn open_tag_rename(&mut self, tag: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut f = TextField::new("Tag name", cx);
            f.set_text(&tag, cx);
            f
        });
        let sub = cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| match ev {
            TextFieldEvent::Submit => w.finish_tag_rename(window, cx),
            TextFieldEvent::Cancel => {
                w.citations.tag_rename = None;
                window.focus(&w.citations.focus);
                cx.notify();
            }
            _ => {}
        });
        window.focus(&field.focus_handle(cx));
        self.citations.tag_rename = Some((tag, field, position, sub));
        cx.notify();
    }

    fn finish_tag_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((old, field, ..)) = self.citations.tag_rename.take() else { return };
        let new = field.read(cx).text.trim().to_string();
        window.focus(&self.citations.focus);
        let Some(library) = &mut self.citations.library else { return };
        if new != old {
            match library.rename_tag(&old, &new) {
                Ok(()) => if self.citations.tag_filter.as_ref() == Some(&old) {
                    self.citations.tag_filter = library.tags().into_iter().map(|(tag, _)| tag).find(|tag| tag.eq_ignore_ascii_case(&new));
                },
                Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not rename the tag: {e}"), cx),
            }
        }
        cx.notify();
    }

    pub(crate) fn delete_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        let Some(library) = &mut self.citations.library else { return };
        if let Err(e) = library.delete_tag(tag) { self.show_toast_kind(ToastKind::Error, &format!("Could not delete the tag: {e}"), cx); }
        cx.notify();
    }

    /// How many entries carry `tag`.
    pub(crate) fn tag_count(&self, tag: &str) -> usize {
        self.citations.library.as_ref().map_or(0, |library| library.tags().into_iter().find(|(name, _)| name == tag).map_or(0, |(_, count)| count))
    }

    /// Applies `tag` to the entries in the tag editor. False if it was refused.
    fn set_tag(&mut self, tag: &str, on: bool, cx: &mut Context<Self>) -> bool {
        let Some(keys) = self.citations.tag_editor.as_ref().map(|editor| editor.keys.clone()) else { return false };
        let Some(library) = &mut self.citations.library else { return false };
        let result = library.set_tag(&keys, tag, on);
        if let Err(e) = &result { self.show_toast_kind(ToastKind::Error, &format!("Could not change tags: {e}"), cx); }
        cx.notify();
        result.is_ok()
    }

    fn column_width(&self, name: &str, default: f32) -> f32 {
        self.citations.widths.get(name).copied().unwrap_or(default)
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

    /// Every key cited in a document and the files it includes.
    fn cited_in(&self, id: &str) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for (_, text) in self.document_files(id) {
            keys.extend(cited_keys(&text));
        }
        keys
    }

    /// A document and the files it includes, each once.
    fn document_files(&self, id: &str) -> Vec<(String, String)> {
        let mut stack = vec![id.to_string()];
        let mut seen = BTreeSet::new();
        let mut out = vec![];
        while let Some(file) = stack.pop() {
            if !seen.insert(file.clone()) { continue; }
            let Ok(text) = self.store.load(&file) else { continue };
            stack.extend(included_files(&text, &file));
            out.push((file, text));
        }
        out
    }

    /// Rewrites each generated block to hold what its document cites.
    pub(crate) fn sync_bibliographies(&mut self, cx: &mut Context<Self>) {
        let Some(library) = &self.citations.library else { return };
        let documents: Vec<String> = self.entries.iter().filter(|e| e.typst && e.body.contains(library::BLOCK_MARKER)).map(|e| e.id.clone()).collect();
        let mut updates = vec![];
        for id in documents {
            let keys: BTreeSet<String> = self.cited_in(&id).into_iter().filter(|k| library.contains(k)).collect();
            let (Ok(yaml), Ok(text)) = (library.subset(&keys), self.store.load(&id)) else { continue };
            if let Some(updated) = library::sync_block(&text, &yaml) {
                updates.push((id, updated));
            }
        }
        let changed = !updates.is_empty();
        for (id, text) in updates {
            if let Err(e) = self.store.save(&id, &text) {
                self.show_toast_kind(ToastKind::Error, &format!("Could not update the bibliography in {id}: {e}"), cx);
                continue;
            }
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) { entry.body = text.clone(); }
            if self.current.as_deref() == Some(id.as_str()) && let Some(editor) = &self.editor {
                editor.update(cx, |e, cx| e.sync_text(&text, cx));
            }
        }
        // An included file's preview shows the document whose block just changed.
        if changed && let (Some(editor), Some(preview)) = (self.editor.clone(), self.preview.clone()) {
            let text = editor.read(cx).buffer.text().to_string();
            preview.update(cx, |p, cx| p.update(text, cx));
        }
    }

    pub(crate) fn bibliography_mode(&self, cx: &App) -> Mode {
        match (&self.current, &self.editor) {
            (Some(id), Some(editor)) => library::mode(editor.read(cx).buffer.text(), id),
            _ => Mode::None,
        }
    }

    /// Switches the open document's bibliography as one undoable edit.
    pub(crate) fn set_bibliography(&mut self, target: Mode, cx: &mut Context<Self>) {
        let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone()) else { return };
        let text = library::remove(editor.read(cx).buffer.text(), &id);
        let text = match target {
            Mode::Embedded => library::embed(&text, ""),
            Mode::Linked => library::link(&text, &id),
            _ => text,
        };
        editor.update(cx, |e, cx| e.reload(&text, cx));
        self.flush(cx);
    }

    /// The document holding the open file's bibliography, and its style. A
    /// chapter's bibliography is usually in the document that includes it.
    pub(crate) fn style_owner(&self, cx: &App) -> Option<(String, String)> {
        let id = self.current.clone()?;
        [id.clone(), self.top_document(&id)].into_iter().find_map(|doc| {
            let text = self.text_of_saved(&doc, cx)?;
            let mode = library::mode(&text, &doc);
            (mode != Mode::None).then(|| {
                let style = bibliography_style(&text, &doc).unwrap_or_else(|| library::DEFAULT_STYLE.to_string());
                (doc, style)
            })
        })
    }

    /// A document's text without saving: the open editor's copy, or the file.
    fn text_of_saved(&self, id: &str, cx: &App) -> Option<String> {
        match (&self.current, &self.editor) {
            (Some(current), Some(editor)) if current == id => Some(editor.read(cx).buffer.text().to_string()),
            _ => self.store.load(id).ok(),
        }
    }

    /// Gives the open file's bibliography a citation style.
    pub(crate) fn set_citation_style(&mut self, style: &str, cx: &mut Context<Self>) {
        self.style_menu = false;
        let Some((doc, _)) = self.style_owner(cx) else { return cx.notify() };
        let Some(updated) = self.text_of_saved(&doc, cx).and_then(|text| set_bibliography_style(&text, &doc, style)) else { return };
        if self.current.as_deref() == Some(doc.as_str()) && let Some(editor) = self.editor.clone() {
            editor.update(cx, |e, cx| e.reload(&updated, cx));
            self.flush(cx);
        } else {
            if let Err(e) = self.store.save(&doc, &updated) {
                return self.show_toast_kind(ToastKind::Error, &format!("Could not change the style in {doc}: {e}"), cx);
            }
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id == doc) { entry.body = updated; }
            if let (Some(editor), Some(preview)) = (self.editor.clone(), self.preview.clone()) {
                let text = editor.read(cx).buffer.text().to_string();
                preview.update(cx, |p, cx| p.update(text, cx));
            }
        }
        cx.notify();
    }

    /// The documents that include `id`, directly or through other files.
    fn includers(&self, id: &str) -> Vec<String> {
        self.entries.iter().filter(|e| e.typst && e.id != id)
            .filter(|e| self.document_files(&e.id).iter().any(|(file, _)| file == id))
            .map(|e| e.id.clone()).collect()
    }

    /// The top-level document `id` belongs to: a document has one bibliography,
    /// so it goes there and not in an included chapter.
    pub(crate) fn top_document(&self, id: &str) -> String {
        let includers = self.includers(id);
        includers.iter().find(|doc| self.includers(doc).is_empty()).cloned().unwrap_or_else(|| id.to_string())
    }

    /// Embeds a bibliography in `id`'s top-level document unless the document
    /// or any file above it already has one.
    fn ensure_bibliography(&mut self, id: &str, cx: &mut Context<Self>) {
        let body = |w: &Self, doc: &str| w.entries.iter().find(|e| e.id == doc).map_or(String::new(), |e| e.body.clone());
        if self.bibliography_mode(cx) != Mode::None { return; }
        if self.includers(id).iter().any(|doc| library::mode(&body(self, doc), doc) != Mode::None) { return; }
        let top = self.top_document(id);
        if top == id {
            let Some(editor) = self.editor.clone() else { return };
            let text = library::embed(editor.read(cx).buffer.text(), "");
            editor.update(cx, |e, cx| e.sync_text(&text, cx));
        } else if let Ok(text) = self.store.load(&top) {
            let text = library::embed(&text, "");
            if let Err(e) = self.store.save(&top, &text) {
                return self.show_toast_kind(ToastKind::Error, &format!("Could not add a bibliography to {top}: {e}"), cx);
            }
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id == top) { entry.body = text; }
        }
    }

    /// Cites `key` at the caret, embedding a bibliography if nothing covers this
    /// document yet.
    pub(crate) fn insert_citation(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone()) else {
            return self.show_toast_kind(ToastKind::Error, "Open a document to insert a citation", cx);
        };
        if !id.ends_with(".typ") {
            return self.show_toast_kind(ToastKind::Error, "Citations go in Typst documents", cx);
        }
        self.show_view(View::Doc, cx);
        editor.update(cx, |e, cx| e.insert_citation(key, cx));
        self.citation_added(&id, cx);
        editor.read(cx).focus(window);
        cx.notify();
    }

    /// Saves a new citation and makes sure its document has a bibliography.
    pub(crate) fn citation_added(&mut self, id: &str, cx: &mut Context<Self>) {
        self.flush(cx);
        self.ensure_bibliography(id, cx);
        self.mark_unsaved(cx);
    }

    pub(crate) fn open_cite_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_source_picker(PickFor::Cite, window, cx);
    }

    fn open_source_picker(&mut self, purpose: PickFor, window: &mut Window, cx: &mut Context<Self>) {
        self.reload_library();
        let field = cx.new(|cx| {
            let mut f = TextField::new("Search library", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| {
            let shown = w.picker_rows(cx).len();
            let Some(picker) = w.citations.picker.as_mut() else { return };
            match ev {
                TextFieldEvent::Changed => picker.selected = 0,
                TextFieldEvent::Up => picker.selected = picker.selected.saturating_sub(1),
                TextFieldEvent::Down => picker.selected = (picker.selected + 1).min(shown.saturating_sub(1)),
                TextFieldEvent::Cancel => w.close_cite_picker(window, cx),
                TextFieldEvent::Submit => {
                    let selected = picker.selected;
                    if let Some(row) = w.picker_rows(cx).get(selected).cloned() { w.pick_source(&row.key, window, cx); }
                }
                _ => {}
            }
            cx.notify();
        })];
        window.focus(&field.focus_handle(cx));
        self.citations.picker = Some(CitePicker { field, selected: 0, purpose, _subs: subs });
        cx.notify();
    }

    fn pick_source(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(purpose) = self.citations.picker.as_ref().map(|p| p.purpose.clone()) else { return };
        self.close_cite_picker(window, cx);
        match purpose {
            PickFor::Cite => self.insert_citation(key, window, cx),
            PickFor::Attach(path) => self.attach_paper(&path, &paper_file(key), cx),
            PickFor::Link(id) => self.link_paper(&id, key, cx),
        }
    }

    fn close_cite_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.citations.picker = None;
        if let Some(editor) = &self.editor { editor.read(cx).focus(window); }
        cx.notify();
    }

    fn picker_rows(&self, cx: &App) -> Vec<Row> {
        let query = self.citations.picker.as_ref().map_or(String::new(), |p| p.field.read(cx).text.trim().to_lowercase());
        let mut rows = self.visible_rows(&query);
        // A source holds one paper.
        if self.citations.picker.as_ref().is_some_and(|p| !matches!(p.purpose, PickFor::Cite)) { rows.retain(|row| !self.has_paper(&row.key)); }
        rows.truncate(PICKER_ROWS);
        rows
    }

    pub(crate) fn render_cite_picker(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.citations.picker.as_ref()?;
        let rows = self.picker_rows(cx);
        let empty = rows.is_empty();
        let list = rows.into_iter().enumerate().map(|(i, row)| {
            let key = row.key.clone();
            div().id(("cite-row", i)).px(px(12.)).py(px(6.)).rounded(px(8.)).cursor_pointer().flex().flex_col()
                .when(i == picker.selected, |d| d.bg(Theme::fill_hover_strong()))
                .child(div().text_size(px(13.)).line_height(px(18.)).truncate().child(ui_text(if row.title.is_empty() { row.key.clone() } else { row.title.clone() })))
                .child(div().text_size(px(12.)).line_height(px(16.)).truncate().text_color(Theme::text_caption()).child(format!("{} · {} · @{}", row.authors, row.year, row.key)))
                .on_click(cx.listener(move |w, _, window, cx| w.pick_source(&key, window, cx)))
        }).collect::<Vec<_>>();
        let hint = (!matches!(picker.purpose, PickFor::Cite)).then(|| div().px(px(4.)).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Choose the source for this paper"));
        let unlinked = if let PickFor::Attach(path) = &picker.purpose { Some(path.clone()) } else { None };
        let panel = dialog("cite-picker").w(px(520.)).p(px(12.)).flex().flex_col().gap(px(8.))
            .children(hint)
            .child(search_well(&picker.field, window, cx))
            .when(empty, |d| d.child(div().px(px(12.)).py(px(8.)).text_size(px(13.)).text_color(Theme::text_caption()).child("No matching entries")))
            .children(list)
            .when_some(unlinked, |d, path| d.child(div().flex().child(plain_button("picker-unlinked", "Add Without a Source").on_click(cx.listener(move |w, _, window, cx| {
                w.close_cite_picker(window, cx);
                let name = path.file_stem().map_or("paper".into(), |name| name.to_string_lossy().into_owned());
                w.attach_paper(&path, &format!("{PAPERS_DIR}/{name}.pdf"), cx);
            })))));
        Some(deferred(div().id("cite-scrim").absolute().inset_0().bg(scrim()).flex().items_start().justify_center().pt(px(96.)).occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|w, _, window, cx| w.close_cite_picker(window, cx)))
            .child(panel.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))).with_priority(3).into_any_element())
    }

    /// Shows `key` in the inspector and puts the caret in its first field, or
    /// without a key, adds a blank entry to fill in.
    fn open_entry(&mut self, key: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let key = match key {
            Some(key) => key,
            None => {
                let Some(library) = &mut self.citations.library else { return };
                let key = match library.add_blank(&SOURCE_TYPES[0]) {
                    Ok(key) => key,
                    Err(e) => return self.show_toast_kind(ToastKind::Error, &format!("Could not add an entry: {e}"), cx),
                };
                self.citations.tag_filter = None;
                self.citations.search.update(cx, |search, cx| search.set_text("", cx));
                self.refresh_after_library_change(cx);
                key
            }
        };
        if self.view != View::Library { self.show_view(View::Library, cx); }
        if self.citations.details_hidden { self.set_details_hidden(false, window, cx); }
        self.select_only(&key);
        self.sync_inspector(window, cx);
        if let Some((_, field)) = self.citations.inspector.as_ref().and_then(|inspector| inspector.fields.first()) { window.focus(&field.focus_handle(cx)); }
        cx.notify();
    }

    fn set_details_hidden(&mut self, hidden: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.citations.details_hidden = hidden;
        self.sync_inspector(window, cx);
        if hidden { window.focus(&self.citations.focus); }
        self.set_pref(|p| p.library_details_hidden = hidden, cx);
        cx.notify();
    }

    /// Matches the inspector to the selection: one visible entry, or none.
    fn sync_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_in(&self.library_rows(cx));
        let wanted = match selected.as_slice() {
            [key] if !self.citations.details_hidden => Some(key.clone()),
            _ => None,
        };
        if self.citations.inspector.as_ref().map(|inspector| &inspector.key) == wanted.as_ref() { return; }
        // A field's blur arrives after this frame, once the field is gone.
        self.save_inspector(cx);
        self.citations.inspector = wanted.and_then(|key| self.build_inspector(key, window, cx));
    }

    fn inspector_field(&mut self, hint: &'static str, text: &str, wrap: bool, subs: &mut Vec<Subscription>, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextField> {
        let field = cx.new(|cx| {
            let mut f = TextField::new(hint, cx);
            f.placeholder_color = Theme::text_disabled();
            f.tab_events = true;
            f.wrap = wrap;
            f.set_text(text, cx);
            f
        });
        subs.push(cx.subscribe_in(&field, window, |w, field, ev: &TextFieldEvent, window, cx| {
            let step = match ev {
                TextFieldEvent::Submit => return w.save_inspector(cx),
                TextFieldEvent::Cancel => return w.revert_inspector(window, cx),
                TextFieldEvent::Tab => 1,
                TextFieldEvent::ShiftTab => -1,
                _ => return,
            };
            w.save_inspector(cx);
            let Some(inspector) = &w.citations.inspector else { return };
            let order: Vec<&Entity<TextField>> = std::iter::once(&inspector.key_field).chain(inspector.fields.iter().map(|(_, field)| field)).collect();
            let Some(at) = order.iter().position(|f| *f == field) else { return };
            let next = (at as i32 + step).rem_euclid(order.len() as i32) as usize;
            window.focus(&order[next].focus_handle(cx));
        }));
        let handle = field.focus_handle(cx);
        subs.push(cx.on_blur(&handle, window, |w, _, cx| w.save_inspector(cx)));
        field
    }

    fn build_inspector(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) -> Option<Inspector> {
        let library = self.citations.library.as_ref()?;
        let kind = library.source_type(&key)?;
        let values: Vec<(Field, String)> = kind.fields.iter().map(|field| (*field, library.field_text(&key, field))).collect();
        let mut subs = vec![];
        let key_field = self.inspector_field("knuth1984", &key, false, &mut subs, window, cx);
        let fields = values.into_iter().map(|(field, text)| (field, self.inspector_field(field.hint, &text, field.long, &mut subs, window, cx))).collect();
        let cited_in = self.entries.iter().filter(|e| e.typst).filter_map(|e| {
            let places = citations_of(&e.body, &key);
            places.first().map(|first| (e.id.clone(), places.len(), *first))
        }).collect();
        Some(Inspector { key, key_field, fields, error: None, type_menu: None, cited_in, _subs: subs })
    }

    /// Esc: back to what the file holds.
    fn revert_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.citations.inspector.take().map(|inspector| inspector.key) else { return };
        self.citations.inspector = self.build_inspector(key, window, cx);
        window.focus(&self.citations.focus);
        cx.notify();
    }

    /// Writes what the inspector holds. The fields stay as typed when the
    /// library refuses them, with the reason beside them.
    pub(crate) fn save_inspector(&mut self, cx: &mut Context<Self>) {
        let Some(inspector) = &self.citations.inspector else { return };
        let key = inspector.key.clone();
        let new_key = inspector.key_field.read(cx).text.trim().to_string();
        let fields: Vec<(Field, String)> = inspector.fields.iter().map(|(field, entity)| (*field, entity.read(cx).text.clone())).collect();
        let Some(library) = &mut self.citations.library else { return };
        let before = library.paper_ids(&key);
        let result = library.save_fields(&key, &new_key, &fields);
        let found_by_new_id = library.paper_ids(&new_key) != before;
        // Beside the field, the file's path only gets in the way.
        let error = result.as_ref().err().map(|e| { let e = e.to_string(); e.split_once(" is not a valid library: ").map_or(e.clone(), |(_, why)| why.to_string()) });
        if let Some(inspector) = &mut self.citations.inspector { inspector.error = error; }
        if matches!(result, Ok(true)) {
            // Before the refresh: bibliographies list what documents cite.
            if new_key != key { self.rename_citations(&key, &new_key, cx); }
            self.refresh_after_library_change(cx);
            if new_key != key {
                self.select_only(&new_key);
                if let Some(inspector) = &mut self.citations.inspector { inspector.key = new_key.clone(); }
                self.rename_paper(&key, &new_key, cx);
            }
            if found_by_new_id { self.fetch_papers(vec![new_key], cx); }
        }
        cx.notify();
    }

    /// Follows a key change through every document that cites it.
    fn rename_citations(&mut self, old: &str, new: &str, cx: &mut Context<Self>) {
        let mut changed = 0;
        for id in self.entries.iter().filter(|e| e.typst).map(|e| e.id.clone()).collect::<Vec<_>>() {
            let open = self.editor.clone().filter(|_| self.current.as_deref() == Some(id.as_str()));
            let text = match &open {
                Some(editor) => editor.read(cx).buffer.text().to_string(),
                None => self.store.load(&id).unwrap_or_default(),
            };
            let Some(updated) = rename_citation(&text, old, new) else { continue };
            if let Err(e) = self.store.save(&id, &updated) {
                self.show_toast_kind(ToastKind::Error, &format!("Could not update the citation in {id}: {e}"), cx);
                continue;
            }
            if let Some(editor) = &open { editor.update(cx, |e, cx| e.reload(&updated, cx)); }
            changed += 1;
        }
        match changed {
            0 => {}
            1 => self.show_toast("Updated the citation in 1 document", cx),
            n => self.show_toast(&format!("Updated citations in {n} documents"), cx),
        }
    }

    /// Changes the type, asking first if that would lose data.
    fn choose_source_type(&mut self, kind: &'static SourceType, window: &mut Window, cx: &mut Context<Self>) {
        let Some(inspector) = &mut self.citations.inspector else { return };
        inspector.type_menu = None;
        let key = inspector.key.clone();
        if self.citations.library.as_ref().is_some_and(|library| library.type_change_drops_parent(&key, kind)) {
            self.confirm = Some(Confirm::ChangeSourceType(kind));
            return cx.notify();
        }
        self.set_source_type(kind, window, cx);
    }

    pub(crate) fn set_source_type(&mut self, kind: &'static SourceType, window: &mut Window, cx: &mut Context<Self>) {
        self.save_inspector(cx);
        let Some(key) = self.citations.inspector.take().map(|inspector| inspector.key) else { return };
        let Some(library) = &mut self.citations.library else { return };
        if let Err(e) = library.set_source_type(&key, kind) { self.show_toast_kind(ToastKind::Error, &format!("Could not change the type: {e}"), cx); }
        self.citations.inspector = self.build_inspector(key, window, cx);
        self.refresh_after_library_change(cx);
        cx.notify();
    }

    pub(crate) fn delete_entries(&mut self, keys: &[String], cx: &mut Context<Self>) {
        let Some(library) = &mut self.citations.library else { return };
        match library.delete_entries(keys) {
            Ok(()) => {
                for key in keys { self.citations.selected.remove(key); }
                self.refresh_after_library_change(cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not delete entry: {e}"), cx),
        }
        cx.notify();
    }

    /// Takes in a library version: the panel, documents, and open file follow it.
    pub(crate) fn library_restored(&mut self, text: &str, cx: &mut Context<Self>) {
        self.citations.inspector = None;
        self.citations.selected.clear();
        self.refresh_after_library_change(cx);
        if self.current.as_deref() == Some(library::LIBRARY_FILE) && let Some(editor) = self.editor.clone() {
            editor.update(cx, |e, cx| e.reload(text, cx));
        }
    }

    fn refresh_after_library_change(&mut self, cx: &mut Context<Self>) {
        self.entries = self.store.list();
        self.sort();
        self.sync_bibliographies(cx);
    }

    fn open_lookup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut f = TextField::new("DOI, arXiv ID, PMID, or ISBN", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| match ev {
            TextFieldEvent::Submit => w.run_lookup(window, cx),
            TextFieldEvent::Cancel => {
                w.citations.lookup = None;
                cx.notify();
            }
            _ => {}
        })];
        window.focus(&field.focus_handle(cx));
        self.citations.lookup = Some(Lookup { field, busy: false, problems: vec![], _subs: subs });
        cx.notify();
    }

    /// Fetches everything in the field, then adds what came back.
    fn run_lookup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.citations.lookup else { return };
        if dialog.busy { return; }
        let text = dialog.field.read(cx).text.clone();
        let parsed = lookup::identifiers(&text);
        if parsed.is_empty() { return; }
        dialog.busy = true;
        dialog.problems.clear();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let fetched: Vec<(Option<Identifier>, Result<serde_yaml::Mapping, String>)> = cx.background_spawn(async move {
                parsed.into_iter().map(|token| match token {
                    Ok(id) => {
                        let result = lookup::fetch(&id).map_err(|e| format!("{id}: {e}"));
                        (Some(id), result)
                    }
                    Err(text) => (None, Err(format!("{text}: not a DOI, arXiv ID, PMID, or ISBN"))),
                }).collect()
            }).await;
            let _ = this.update(cx, |w, cx| w.finish_lookup(fetched, cx));
        }).detach();
    }

    fn finish_lookup(&mut self, fetched: Vec<(Option<Identifier>, Result<serde_yaml::Mapping, String>)>, cx: &mut Context<Self>) {
        self.reload_library();
        let (mut added, mut already) = (0, 0);
        let mut problems = vec![];
        let mut keys = vec![];
        for (id, result) in fetched {
            let entry = match result {
                Ok(entry) => entry,
                Err(problem) => {
                    problems.push(problem);
                    continue;
                }
            };
            let Some(library) = &mut self.citations.library else {
                problems.push("The library could not be read".to_string());
                break;
            };
            match library.add_fetched(entry) {
                Ok(Added::New(key)) => {
                    added += 1;
                    keys.push(key);
                }
                Ok(Added::Duplicate(_)) => already += 1,
                Err(e) => problems.push(format!("{}: {e}", id.map_or(String::new(), |id| id.to_string()))),
            }
        }
        if added > 0 { self.refresh_after_library_change(cx); }
        self.fetch_papers(keys, cx);
        let summary = match (added, already) {
            (0, 0) => None,
            (1, 0) => Some("Added 1 entry".to_string()),
            (n, 0) => Some(format!("Added {n} entries")),
            (0, 1) => Some("Already in the library".to_string()),
            (0, k) => Some(format!("All {k} are already in the library")),
            (n, k) => Some(format!("Added {n}, {k} already in the library")),
        };
        if let Some(summary) = summary { self.show_toast(&summary, cx); }
        match &mut self.citations.lookup {
            Some(dialog) if !problems.is_empty() => {
                dialog.busy = false;
                dialog.problems = problems;
            }
            _ => self.citations.lookup = None,
        }
        cx.notify();
    }

    pub(crate) fn render_lookup(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let lookup = self.citations.lookup.as_ref()?;
        let problems = lookup.problems.iter().map(|p| div().text_size(px(13.)).line_height(px(18.)).text_color(theme::c(craft::R400)).child(p.clone())).collect::<Vec<_>>();
        let panel = dialog("lookup").w(px(480.)).p(px(20.)).flex().flex_col().gap(px(10.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Add by identifier"))
            .child(div().text_size(px(13.)).line_height(px(18.)).text_color(Theme::text_caption()).child("Enter one or more DOIs, arXiv IDs, PMIDs, or ISBNs. Each is sent to doi.org, NCBI, or Open Library, and Thesio looks for an open-access PDF through arXiv and Unpaywall."))
            .child(field_well(&lookup.field, window, cx))
            .children(problems)
            .child(div().mt(px(8.)).flex().items_center().gap(px(8.))
                .child(plain_button("lookup-manual", "Enter Manually…").on_click(cx.listener(|w, _, window, cx| {
                    w.citations.lookup = None;
                    w.open_entry(None, window, cx);
                })))
                .child(div().flex_1())
                .child(secondary_button("lookup-cancel", "Cancel").on_click(cx.listener(|w, _, _, cx| {
                    w.citations.lookup = None;
                    cx.notify();
                })))
                .when(lookup.busy, |d| d.child(div().px(px(12.)).text_size(px(13.)).text_color(Theme::text_caption()).child("Looking up…")))
                .when(!lookup.busy, |d| d.child(primary_button("lookup-add", "Add").on_click(cx.listener(|w, _, window, cx| w.run_lookup(window, cx))))));
        Some(deferred(div().id("lookup-scrim").absolute().inset_0().bg(scrim()).flex().items_start().justify_center().pt(px(96.)).occlude().child(panel)).with_priority(3).into_any_element())
    }

    fn import_bib(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions {
            files: true, directories: false, multiple: true, prompt: None,
            extensions: Some(vec!["bib".into()]),
        }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt {
                Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(paths) = paths else { return };
            let _ = this.update(cx, |w, cx| {
                let (mut added, mut skipped) = (0, 0);
                for path in &paths {
                    let result = std::fs::read_to_string(path).map_err(anyhow::Error::from)
                        .and_then(|text| w.citations.library.as_mut().ok_or_else(|| anyhow::anyhow!("The library could not be read"))?.import_biblatex(&text));
                    match result {
                        Ok((a, s)) => { added += a; skipped += s; }
                        Err(e) => w.show_toast_kind(ToastKind::Error, &format!("Could not import {}: {e}", path.display()), cx),
                    }
                }
                if added + skipped > 0 {
                    w.refresh_after_library_change(cx);
                    let already = if skipped > 0 { format!(", {skipped} already in the library") } else { String::new() };
                    w.show_toast(&format!("Imported {added} entries{already}"), cx);
                }
                cx.notify();
            });
        }).detach();
    }

    fn export_bib(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.citations.library.as_ref().map(Library::export_biblatex) else { return };
        let root = self.store.root();
        let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
        let prompt = picked.is_none().then(|| cx.prompt_for_new_path(&root, Some("library.bib")));
        cx.spawn(async move |this, cx| {
            let path = match prompt {
                Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(path) = path else { return };
            let result = std::fs::write(&path, text);
            let _ = this.update(cx, |w, cx| match result {
                Ok(()) => w.show_toast("Exported BibLaTeX", cx),
                Err(e) => w.show_toast_kind(ToastKind::Error, &format!("Could not export: {e}"), cx),
            });
        }).detach();
    }

    pub(crate) fn render_library(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_inspector(window, cx);
        let tags = self.citations.library.as_ref().map(Library::tags).unwrap_or_default();
        if !tags.iter().any(|(tag, _)| self.citations.tag_filter.as_ref() == Some(tag)) { self.citations.tag_filter = None; }
        let rows = self.library_rows(cx);
        let total = self.citations.library.as_ref().map_or(0, Library::len);
        let can_edit = self.citations.library.is_some();
        let header = page_header(self, "Library", (total > 0).then_some(total), 26., cx)
            .child(div().id("library-search").no_drag().w(px(220.)).mr(px(8.)).child(search_well(&self.citations.search, window, cx)))
            .when(can_edit, |d| d
                .child(toolbar_group().mr(px(8.)).child(toolbar_icon("library-details", Icon::Details).no_drag().tip(if self.citations.details_hidden { "Show details" } else { "Hide details" }, None)
                    .on_click(cx.listener(|w, _, window, cx| w.set_details_hidden(!w.citations.details_hidden, window, cx)))))
                .child(toolbar_group().mr(px(8.)).child(toolbar_icon("library-more", Icon::Ellipsis).no_drag().when(self.citations.menu, |d| d.bg(Theme::fill_hover_strong())).tip("More", None)
                    .on_click(cx.listener(|w, _, _, cx| { w.citations.menu = !w.citations.menu; cx.notify(); }))))
                .child(primary_icon_button("library-add", Icon::Plus, "Add").no_drag().on_click(cx.listener(|w, _, window, cx| w.open_lookup(window, cx)))));
        let table = self.render_library_table(&tags, rows, total, window, cx);
        let inspector = self.render_inspector(cx);
        div().flex_1().min_w_0().h_full().flex().flex_col().child(header)
            .child(div().flex_1().min_h_0().flex().child(table).children(inspector))
            .into_any_element()
    }

    /// The fixed columns that fit beside a usable title, at their current widths.
    fn visible_columns(&self, window: &Window) -> Vec<(Col, &'static str, Option<f32>)> {
        let panel = if self.citations.inspector.is_some() { INSPECTOR_W } else { 0. };
        let sidebar = if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let room = f32::from(window.viewport_size().width) - sidebar - panel - 2. * (TABLE_X + CELL_X) - CITE_W - MIN_TITLE_W;
        let mut columns: Vec<_> = COLS.iter().map(|(col, label, width)| (*col, *label, width.map(|w| self.column_width(label, w)))).collect();
        for col in [Col::Key, Col::Kind, Col::Tags, Col::Year, Col::Authors] {
            if columns.iter().filter_map(|(.., width)| *width).sum::<f32>() <= room { break; }
            columns.retain(|(c, ..)| *c != col);
        }
        columns
    }

    fn render_library_table(&mut self, tags: &[(String, usize)], rows: Vec<Row>, total: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let mut page = div().flex_1().min_w_0().min_h_0().flex().flex_col();
        if let Some(error) = &self.citations.error {
            return page.child(div().p(px(26.)).text_size(px(13.)).text_color(Theme::danger())
                .child(format!("{error}. Fix the file to use the Library; Thesio won't change it until it can be read."))).into_any_element();
        }
        if total == 0 {
            return page.child(crate::trash::empty_state(Icon::Library, "Your library is empty", "Add an entry by identifier, or import a BibLaTeX file.")).into_any_element();
        }
        if !tags.is_empty() {
            let chip = |id: ElementId, label: String, count: usize, filter: Option<String>, on: bool| {
                div().id(id).h(px(24.)).px(px(8.)).flex().items_center().gap(px(5.)).rounded(px(5.)).cursor_pointer().text_size(px(12.))
                    .text_color(if on { Theme::text() } else { Theme::text_caption() })
                    .when(on, |d| d.bg(Theme::fill_active()))
                    .active(|s| s.bg(Theme::fill_pressed()))
                    .child(label)
                    .child(div().text_color(Theme::text_placeholder()).child(count.to_string()))
                    .when_some(filter.clone(), |d, tag| d.on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                        w.citations.tag_menu = Some((tag.clone(), ev.position));
                        cx.notify();
                    })))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        w.citations.tag_filter = filter.clone();
                        cx.notify();
                    }))
            };
            let filter = self.citations.tag_filter.clone();
            page = page.child(div().flex_none().mx(px(TABLE_X)).px(px(4.)).pt(px(8.)).pb(px(6.)).flex().flex_wrap().gap(px(2.))
                .child(chip("tag-all".into(), "All".to_string(), total, None, filter.is_none()))
                .children(tags.iter().enumerate().map(|(i, (tag, count))| chip(("tag", i).into(), tag.clone(), *count, Some(tag.clone()), filter.as_ref() == Some(tag)))));
        }
        let (sort_col, ascending) = self.citations.sort;
        let columns = self.visible_columns(window);
        let dragged = self.citations.column_drag.map(|drag| drag.name);
        let headers = columns.iter().enumerate().map(|(i, (col, label, width))| {
            let (col, name) = (*col, *label);
            let sorted = sort_col == col;
            let start_width = *width;
            div().id(*label).relative()
                .when_some(*width, |d, w| d.w(px(w)).flex_none())
                .when(width.is_none(), |d| d.flex_1().min_w_0())
                .pr(px(CELL_X)).flex().items_center().gap(px(4.)).text_size(px(12.)).line_height(px(24.)).cursor_pointer()
                .text_color(if sorted { Theme::text() } else { list_header_text() })
                .child(*label)
                .when(sorted, |d| d.child(icon(if ascending { Icon::ChevronUp } else { Icon::ChevronDown }, 10., Theme::text_caption())))
                .on_click(cx.listener(move |w, _, _, cx| {
                    let (current, ascending) = w.citations.sort;
                    w.citations.sort = (col, current != col || !ascending);
                    cx.notify();
                }))
                // Columns hang from the right edge, so a column's left edge is what moves.
                .when_some(start_width, |d, start_width| d.child(
                    div().id(("library-column-resize", i)).absolute().top_0().bottom_0().left(px(-(CELL_X + 4.))).w(px(9.)).cursor(CursorStyle::ResizeLeftRight)
                        .child(div().absolute().top_0().bottom_0().left(px(4.)).w(px(1.)).when(dragged == Some(name), |d| d.bg(Theme::primary())))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Left, cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            w.citations.column_drag = Some(ColumnDrag { name, start_x: ev.position.x, start_width });
                            cx.notify();
                        }))
                ))
        }).collect::<Vec<_>>();
        let table_header = div().h(px(26.)).flex_none().mx(px(TABLE_X)).px(px(CELL_X)).flex().items_center().border_b_1().border_color(list_line()).children(headers).child(div().w(px(CITE_W)).flex_none());
        let count = rows.len();
        if count == 0 {
            return page.child(table_header).child(crate::trash::empty_state(Icon::Search, "No results", "Try a different title, author, year, or key.")).into_any_element();
        }
        let can_cite = self.can_cite();
        let selected = self.citations.selected.clone();
        // As on macOS: the accent while the table has the keyboard, gray once it moves on.
        let active = self.citations.focus.is_focused(window);
        let papers = self.paper_ids();
        let table = uniform_list("library-rows", count, cx.processor(move |_: &mut Workspace, range: std::ops::Range<usize>, _, cx| {
            range.map(|i| {
                let row = &rows[i];
                let (open, cite, menu) = (row.key.clone(), row.key.clone(), row.key.clone());
                let on = selected.contains(&row.key);
                let accent = on && active;
                let cells = columns.iter().map(|(col, _, width)| {
                    let paper = *col == Col::Title && papers.contains(&paper_file(&row.key));
                    div().when_some(*width, |d, w| d.w(px(w)).flex_none())
                        .when(width.is_none(), |d| d.flex_1().min_w_0())
                        .pr(px(CELL_X)).flex().items_center().gap(px(6.)).text_size(px(13.))
                        .when(*col == Col::Key, |d| d.font_family(theme::MONO_FONT).text_size(px(12.)))
                        .text_color(if accent { gpui::white() } else if *col == Col::Title { Theme::text() } else { Theme::text_caption() })
                        .child(div().min_w_0().truncate().child(ui_text(cell(row, *col).to_string())))
                        .when(paper, |d| d.child(icon(Icon::Paper, 13., if accent { gpui::white() } else { Theme::text_placeholder() })))
                }).collect::<Vec<_>>();
                div().id(("library-row", i)).group("library-row").w_full().h(px(ROW_H)).px(px(CELL_X)).flex().items_center().cursor_pointer().rounded(px(crate::ui::ROW_RADIUS))
                    .when(i % 2 == 1, |d| d.bg(Theme::fill_group()))
                    .when(on, |d| d.bg(if active { Theme::primary() } else { Theme::sidebar_selected() }))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, window, cx| {
                        window.focus(&w.citations.focus);
                        if !w.citations.selected.contains(&menu) { w.select_only(&menu); }
                        w.citations.row_menu = Some((menu.clone(), ev.position));
                        cx.notify();
                    }))
                    .children(cells)
                    .child(div().w(px(CITE_W)).flex_none().flex().justify_end().when(can_cite, |d| d.child(
                        div().id(("library-cite", i)).px(px(8.)).h(px(22.)).flex().items_center().rounded(px(5.)).text_size(px(12.))
                            .border_1().border_color(Theme::line_border())
                            .when(accent, |d| d.text_color(gpui::white()).border_color(gpui::white().opacity(0.5)))
                            .opacity(0.).group_hover("library-row", |s| s.opacity(1.))
                            .active(|s| s.bg(Theme::fill_pressed()))
                            .child("Cite")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |w, _, window, cx| {
                                cx.stop_propagation();
                                w.insert_citation(&cite, window, cx);
                            })),
                    )))
                    .on_click(cx.listener(move |w, ev: &ClickEvent, window, cx| {
                        window.focus(&w.citations.focus);
                        if ev.click_count() >= 2 && !ev.modifiers().modified() { w.open_entry(Some(open.clone()), window, cx); } else { w.click_row(&open, ev.modifiers(), cx); }
                    }))
            }).collect()
        })).track_scroll(self.citations.scroll.clone()).flex_1().min_h_0().px(px(TABLE_X)).pt(px(4.));
        let table = div().id("library-table").flex_1().min_h_0().flex().flex_col().track_focus(&self.citations.focus)
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| w.library_key(ev, window, cx)))
            .child(table);
        page = page.child(table_header).child(table);
        page.into_any_element()
    }

    /// The ids of the papers in the vault.
    fn paper_ids(&self) -> BTreeSet<String> {
        self.entries.iter().filter(|e| !e.folder && e.meta.parent.as_deref() == Some(PAPERS_DIR)).map(|e| e.id.clone()).collect()
    }

    pub(crate) fn has_paper(&self, key: &str) -> bool {
        let file = paper_file(key);
        self.entries.iter().any(|e| e.id == file)
    }

    /// Shows a paper in the app, or where there is no viewer, in the system's app.
    pub(crate) fn open_paper(&mut self, id: &str, cx: &mut Context<Self>) {
        if !crate::native_pdf::supported() { return self.open_paper_externally(id, cx); }
        self.paper = Some(id.to_string());
        self.paper_failed.set(false);
        self.load_paper_notes(id, cx);
        self.show_view(View::Paper, cx);
    }

    /// Scripted runs must not start other apps.
    pub(crate) fn open_paper_externally(&self, id: &str, cx: &App) {
        if std::env::var_os("THESIO_SCRIPT").is_none() { cx.open_with_system(&self.store.root().join(id)); }
    }

    /// The name a paper goes by: its source's title, or its file name.
    pub(crate) fn paper_title(&self, id: &str) -> String {
        let row = self.citations.library.as_ref().and_then(|library| library.rows().into_iter().find(|row| paper_file(&row.key) == id));
        row.map(|row| row.title).filter(|title| !title.is_empty()).unwrap_or_else(|| id.rsplit('/').next().unwrap_or(id).to_string())
    }

    /// The viewer goes under what GPUI draws over the pane, or it would hide it.
    pub(crate) fn paper_covered(&self, window: &Window) -> bool {
        let c = &self.citations;
        // Most row menus stay over the sidebar, clear of the pane.
        let edge = if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        self.row_menu.as_ref().is_some_and(|menu| f32::from(menu.pos.x) + crate::sidebar::ROW_MENU_W > edge) || self.paper_panel.quoting() || self.confirm.is_some() || self.search_open || self.move_dlg.is_some() || c.picker.is_some() || c.lookup.is_some()
    }

    fn choose_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static) {
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions {
            files: true, directories: false, multiple: false, prompt: None,
            extensions: Some(vec!["pdf".into()]),
        }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt {
                Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(path) = paths.and_then(|paths| paths.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, window, cx| then(w, path, window, cx));
        }).detach();
    }

    /// Copies a PDF from outside the vault in as paper `id`.
    fn attach_paper(&mut self, path: &Path, id: &str, cx: &mut Context<Self>) {
        let result = std::fs::File::open(path).and_then(|mut file| {
            let mut head = [0; 5];
            std::io::Read::read_exact(&mut file, &mut head)?;
            Ok(head)
        });
        if !result.is_ok_and(|head| head == *b"%PDF-") {
            return self.show_toast_kind(ToastKind::Error, "That file is not a PDF", cx);
        }
        match self.store.attach(path, id) {
            Ok(()) => {
                self.entries = self.store.list();
                self.sort();
                self.show_toast("Added the paper", cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not add the paper: {e}"), cx),
        }
        cx.notify();
    }

    /// Makes an unlinked paper, and any notes on it, the paper of `key`.
    fn link_paper(&mut self, id: &str, key: &str, cx: &mut Context<Self>) {
        let to = paper_file(key);
        self.follow_key(id, &to, cx);
        self.follow_key(&note_file(id), &note_file(&to), cx);
        cx.notify();
    }

    /// A paper keeps following its entry when the citation key changes.
    fn rename_paper(&mut self, old: &str, new: &str, cx: &mut Context<Self>) {
        self.follow_key(&paper_file(old), &paper_file(new), cx);
        self.follow_key(&note_file(&paper_file(old)), &note_file(&paper_file(new)), cx);
    }

    /// Renames a file that is named after a citation key.
    fn follow_key(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        if from == to || !self.entries.iter().any(|e| e.id == from) { return; }
        let name = to.rsplit('/').next().unwrap_or(to).to_string();
        match self.store.rename(from, &name) {
            Ok((id, rewritten)) => self.path_changed(from, &id, &rewritten, cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not rename {from}: {e}"), cx),
        }
        self.entries = self.store.list();
        self.sort();
    }

    /// Downloads open-access papers for `keys`, one after another, skipping
    /// entries that have one or have nothing to find it by.
    fn fetch_papers(&mut self, keys: Vec<String>, cx: &mut Context<Self>) {
        let Some(library) = &self.citations.library else { return };
        let email = Some(crate::settings::prefs().unpaywall_email).filter(|email| email.split_once('@').is_some_and(|(name, host)| !name.is_empty() && host.contains('.')));
        let jobs: Vec<_> = keys.iter().filter(|key| !self.has_paper(key)).filter_map(|key| {
            let ids = library.paper_ids(key);
            let dest = self.store.path(&paper_file(key)).ok()?;
            (ids.doi.is_some() || ids.arxiv.is_some()).then_some((ids, dest))
        }).collect();
        if jobs.is_empty() {
            if keys.len() == 1 && !self.has_paper(&keys[0]) { self.show_toast_kind(ToastKind::Error, "This entry has no DOI or arXiv ID to find a paper by", cx); }
            return;
        }
        let total = jobs.len();
        self.citations.fetching += total;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (mut found, mut problem) = (0, None);
            for (ids, dest) in jobs {
                let email = email.clone();
                let result = cx.background_spawn(async move { lookup::fetch_paper(&ids, email.as_deref(), &dest) }).await;
                let downloaded = matches!(result, Ok(true));
                if downloaded { found += 1; }
                if let Err(e) = result { problem = Some(e.to_string()); }
                let _ = this.update(cx, |w, cx| {
                    w.citations.fetching -= 1;
                    if downloaded {
                        w.entries = w.store.list();
                        w.sort();
                    }
                    cx.notify();
                });
            }
            let _ = this.update(cx, |w, cx| match (found, problem) {
                (0, Some(problem)) => w.show_toast_kind(ToastKind::Error, &format!("Could not download the paper: {problem}"), cx),
                (0, None) if email.is_none() => w.show_toast_kind(ToastKind::Error, "No PDF on arXiv. Add an email in Settings to search Unpaywall", cx),
                (0, None) => w.show_toast_kind(ToastKind::Error, "No open-access PDF found", cx),
                _ if total == 1 => w.show_toast("Downloaded the paper", cx),
                _ => w.show_toast(&format!("Downloaded {found} of {total} papers"), cx),
            });
        }).detach();
    }

    pub(crate) fn render_sources(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.citations.library.as_ref().map_or(vec![], Library::rows);
        let mut papers: Vec<(String, Option<Row>, String)> = self.paper_ids().into_iter().filter(|id| id.ends_with(".pdf")).map(|id| {
            let row = rows.iter().find(|row| paper_file(&row.key) == id).cloned();
            let title = row.as_ref().map(|row| row.title.clone()).filter(|title| !title.is_empty())
                .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(&id).to_string());
            (id, row, title)
        }).collect();
        papers.sort_by(|a, b| natural_cmp(&a.2, &b.2));
        let fetching = self.citations.fetching;
        let header = page_header(self, "Sources", (!papers.is_empty()).then_some(papers.len()), 26., cx)
            .when(fetching > 0, |d| d.child(div().mr(px(12.)).text_size(px(13.)).text_color(Theme::text_caption()).child(if fetching == 1 { "Downloading 1 paper…".to_string() } else { format!("Downloading {fetching} papers…") })))
            .child(primary_icon_button("sources-add", Icon::Plus, "Add").no_drag().tip("Add a PDF", None).on_click(cx.listener(|w, _, window, cx| {
                w.choose_pdf(window, cx, |w, path, window, cx| w.open_source_picker(PickFor::Attach(path), window, cx));
            })));
        let page = div().flex_1().min_w_0().h_full().flex().flex_col().child(header);
        if papers.is_empty() {
            return page.child(crate::trash::empty_state(Icon::Paper, "No papers yet", "Open-access papers download here, or add a PDF yourself.")).into_any_element();
        }
        let column = |label: &'static str, width: Option<f32>| div().when_some(width, |d, w| d.w(px(w)).flex_none()).when(width.is_none(), |d| d.flex_1().min_w_0()).pr(px(CELL_X)).child(label);
        let table_header = div().h(px(26.)).flex_none().mx(px(TABLE_X)).px(px(CELL_X)).flex().items_center().border_b_1().border_color(list_line())
            .text_size(px(12.)).line_height(px(24.)).text_color(list_header_text())
            .child(column("Title", None)).child(column("Authors", Some(200.))).child(column("Year", Some(64.))).child(column("Key", Some(150.)));
        let count = papers.len();
        let table = uniform_list("source-rows", count, cx.processor(move |_: &mut Workspace, range: std::ops::Range<usize>, _, cx| {
            range.map(|i| {
                let (id, row, title) = &papers[i];
                let text = |pick: fn(&Row) -> &String| row.as_ref().map_or(String::new(), |row| pick(row).clone());
                let cell = |width: Option<f32>, value: String| div().when_some(width, |d, w| d.w(px(w)).flex_none()).when(width.is_none(), |d| d.flex_1().min_w_0()).pr(px(CELL_X)).truncate().child(ui_text(value));
                let (open, menu, link) = (id.clone(), id.clone(), id.clone());
                let unlinked = row.is_none();
                div().id(("source-row", i)).w_full().h(px(ROW_H)).px(px(CELL_X)).flex().items_center().cursor_pointer().rounded(px(crate::ui::ROW_RADIUS)).text_size(px(13.)).text_color(Theme::text_caption())
                    .child(cell(None, title.clone()).text_color(Theme::text()))
                    .child(cell(Some(200.), text(|row| &row.authors)))
                    .child(cell(Some(64.), text(|row| &row.year)))
                    .when(!unlinked, |d| d.child(cell(Some(150.), text(|row| &row.key)).font_family(theme::MONO_FONT).text_size(px(12.))))
                    .when(unlinked, |d| d.child(div().w(px(150.)).flex_none().flex().child(
                        div().id(("source-link", i)).px(px(8.)).h(px(22.)).flex().items_center().rounded(px(5.)).text_size(px(12.)).text_color(Theme::text())
                            .border_1().border_color(Theme::line_border()).active(|s| s.bg(Theme::fill_pressed()))
                            .child("Link to Source…")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |w, _, window, cx| {
                                cx.stop_propagation();
                                w.open_source_picker(PickFor::Link(link.clone()), window, cx);
                            })),
                    )))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                        w.row_menu = Some(RowMenu { id: menu.clone(), pos: ev.position, corner: Corner::TopLeft, batch: false });
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |w, _, _, cx| w.open_paper(&open, cx)))
            }).collect()
        })).flex_1().min_h_0().px(px(TABLE_X)).pt(px(4.));
        page.child(table_header).child(table).into_any_element()
    }

    fn can_cite(&self) -> bool {
        self.current.as_deref().is_some_and(|id| id.ends_with(".typ"))
    }

    pub(crate) fn render_library_row_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.view != View::Library { return None; }
        let (key, position) = self.citations.row_menu.clone()?;
        let item = |id: &'static str, glyph: Icon, label: &'static str| {
            menu_row(id).child(menu_glyph(glyph, 16.)).child(menu_label(label))
        };
        let (edit, cite, copy, paper, attach, notes) = (key.clone(), key.clone(), key.clone(), key.clone(), key.clone(), key);
        let selected = self.selected_in(&self.library_rows(cx));
        let single = selected.len() == 1;
        let (tagged, delete) = (selected.clone(), selected);
        let delete_label = if single { "Delete".to_string() } else { format!("Delete {} Entries", delete.len()) };
        let has_paper = self.has_paper(&paper);
        let menu = popover().id("entry-menu").occlude().w(px(180.)).py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.citations.row_menu = None; cx.notify(); }))
            .when(single, |d| d.child(item("entry-menu-edit", Icon::Edit, "Edit").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                w.open_entry(Some(edit.clone()), window, cx);
            }))))
            .child(item("entry-menu-tags", Icon::Tag, "Tags…").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                w.open_tag_editor(tagged.clone(), position, window, cx);
            })))
            .when(single && self.can_cite(), |d| d.child(item("entry-menu-cite", Icon::Quote, "Cite").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                w.insert_citation(&cite, window, cx);
            }))))
            .when(single, |d| d.child(item("entry-menu-paper", if has_paper { Icon::Paper } else { Icon::Search }, if has_paper { "Open Paper" } else { "Find Paper" }).on_click(cx.listener(move |w, _, _, cx| {
                w.citations.row_menu = None;
                if has_paper { w.open_paper(&paper_file(&paper), cx); } else { w.fetch_papers(vec![paper.clone()], cx); }
                cx.notify();
            }))))
            .when(single && !has_paper, |d| d.child(item("entry-menu-attach", Icon::Import, "Attach Paper…").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                let id = paper_file(&attach);
                w.choose_pdf(window, cx, move |w, path, _, cx| w.attach_paper(&path, &id, cx));
            }))))
            .when(single, |d| d.child(item("entry-menu-notes", Icon::Notes, "Notes").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                w.open_source_notes(&notes, window, cx);
            }))))
            .when(single, |d| d.child(item("entry-menu-copy", Icon::Copy, "Copy Key").on_click(cx.listener(move |w, _, _, cx| {
                w.citations.row_menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                cx.notify();
            }))))
            .child(div().mx(px(13.)).my(px(5.)).h(px(1.)).bg(Theme::line_border()))
            .child(menu_row("entry-menu-delete").text_color(Theme::danger())
                .child(icon(Icon::Delete, 16., Theme::danger()))
                .child(menu_label(delete_label))
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.citations.row_menu = None;
                    w.confirm = Some(Confirm::DeleteEntries(delete.clone()));
                    cx.notify();
                })));
        Some(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }

    pub(crate) fn render_tag_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.view != View::Library { return None; }
        if let Some((_, field, position, _)) = &self.citations.tag_rename {
            let panel = popover().id("tag-rename").occlude().w(px(220.)).p(px(8.))
                .on_mouse_down_out(cx.listener(|w, _, window, cx| w.finish_tag_rename(window, cx)))
                .child(field_well(field, window, cx));
            return Some(deferred(anchored().position(*position).snap_to_window_with_margin(px(8.)).child(panel)).with_priority(2).into_any_element());
        }
        let (tag, position) = self.citations.tag_menu.clone()?;
        let (rename, delete) = (tag.clone(), tag);
        let menu = popover().id("tag-menu").occlude().w(px(180.)).py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.citations.tag_menu = None; cx.notify(); }))
            .child(menu_row("tag-menu-rename").child(menu_glyph(Icon::Edit, 16.)).child(menu_label("Rename…")).on_click(cx.listener(move |w, _, window, cx| {
                w.citations.tag_menu = None;
                w.open_tag_rename(rename.clone(), position, window, cx);
            })))
            .child(menu_row("tag-menu-delete").text_color(Theme::danger()).child(icon(Icon::Delete, 16., Theme::danger())).child(menu_label("Delete")).on_click(cx.listener(move |w, _, _, cx| {
                w.citations.tag_menu = None;
                w.confirm = Some(Confirm::DeleteTag(delete.clone()));
                cx.notify();
            })));
        Some(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }

    pub(crate) fn render_tag_editor(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.view != View::Library { return None; }
        let editor = self.citations.tag_editor.as_ref()?;
        let library = self.citations.library.as_ref()?;
        let typed = editor.field.read(cx).text.trim().to_string();
        let query = typed.to_lowercase();
        let keys: BTreeSet<&String> = editor.keys.iter().collect();
        let rows: Vec<Row> = library.rows().into_iter().filter(|row| keys.contains(&row.key)).collect();
        let tags = library.tags();
        let exists = tags.iter().any(|(tag, _)| tag.eq_ignore_ascii_case(&typed));
        let list = tags.into_iter().filter(|(tag, _)| tag.to_lowercase().contains(&query)).enumerate().map(|(i, (tag, _))| {
            let holders = rows.iter().filter(|row| row.tags.contains(&tag)).count();
            let all = holders == rows.len();
            let mark = if all { Some(Icon::Check) } else if holders > 0 { Some(Icon::Minus) } else { None };
            menu_row(("tag-editor-row", i))
                .child(div().w(px(16.)).flex_none().children(mark.map(|mark| menu_glyph(mark, 14.))))
                .child(menu_label(tag.clone()))
                .on_click(cx.listener(move |w, _, _, cx| { w.set_tag(&tag, !all, cx); }))
        }).collect::<Vec<_>>();
        let panel = popover().id("tag-editor").occlude().w(px(240.)).py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, window, cx| w.close_tag_editor(window, cx)))
            .child(div().px(px(8.)).pb(px(4.)).child(field_well(&editor.field, window, cx)))
            .child(div().id("tag-editor-list").max_h(px(220.)).overflow_y_scroll().children(list))
            .when(!typed.is_empty() && !exists, |d| d.child(menu_row("tag-editor-create").child(menu_glyph(Icon::Plus, 14.)).child(menu_label(format!("Create “{typed}”"))).on_click(cx.listener({
                let typed = typed.clone();
                let field = editor.field.clone();
                move |w, _, _, cx| if w.set_tag(&typed, true, cx) { field.update(cx, |f, cx| f.set_text("", cx)); }
            }))));
        Some(deferred(anchored().position(editor.position).snap_to_window_with_margin(px(8.)).child(panel)).with_priority(2).into_any_element())
    }

    /// While resizing a column, a full-window layer follows the mouse, as the sidebar's does.
    pub(crate) fn render_column_drag(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let drag = self.citations.column_drag?;
        let end = |w: &mut Workspace, cx: &mut Context<Workspace>| {
            w.citations.column_drag = None;
            let widths = w.citations.widths.clone();
            w.set_pref(|p| p.library_columns = widths, cx);
            cx.notify();
        };
        Some(deferred(div().id("library-column-drag").absolute().inset_0().occlude().cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_move(cx.listener(move |w, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button != Some(MouseButton::Left) { return end(w, cx); }
                let width = drag.start_width - f32::from(ev.position.x - drag.start_x);
                w.citations.widths.insert(drag.name.to_string(), width.clamp(MIN_COLUMN_W, MAX_COLUMN_W));
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(move |w, _, _, cx| end(w, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(move |w, _, _, cx| end(w, cx)))).with_priority(4).into_any_element())
    }

    pub(crate) fn render_library_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.citations.menu || self.view != View::Library { return None; }
        let item = |id: &'static str, glyph: Icon, label: &'static str| {
            menu_row(id).child(menu_glyph(glyph, 16.)).child(menu_label(label))
        };
        let menu = popover().id("library-menu").occlude().w(px(220.)).py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.citations.menu = false; cx.notify(); }))
            .child(item("library-import", Icon::Import, "Import BibLaTeX…").on_click(cx.listener(|w, _, window, cx| {
                w.citations.menu = false;
                w.import_bib(window, cx);
            })))
            .child(item("library-export", Icon::Export, "Export BibLaTeX…").on_click(cx.listener(|w, _, _, cx| {
                w.citations.menu = false;
                w.export_bib(cx);
            })))
            .child(item("library-history", Icon::Restore, "Version History…").on_click(cx.listener(|w, _, _, cx| {
                w.citations.menu = false;
                w.open_library_history(cx);
            })))
            .child(item("library-find-papers", Icon::Search, "Find Missing Papers").on_click(cx.listener(|w, _, _, cx| {
                w.citations.menu = false;
                let keys = w.citations.library.iter().flat_map(Library::rows).map(|row| row.key).collect();
                w.fetch_papers(keys, cx);
                cx.notify();
            })));
        // Under the "…" button, which sits left of the Add button.
        let right = window.viewport_size().width - px(crate::ui::header_right_inset() + ADD_W);
        Some(deferred(anchored().anchor(Corner::TopRight).position(point(right, px(crate::app::HEADER_H - 4.))).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }

    fn render_inspector(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let inspector = self.citations.inspector.as_ref()?;
        let kind = self.citations.library.as_ref()?.source_type(&inspector.key)?;
        let row = |label: SharedString, value: AnyElement| {
            div().w_full().flex().items_start().gap(px(10.)).py(px(4.))
                .child(div().w(px(92.)).flex_none().text_right().text_size(px(12.)).line_height(px(20.)).text_color(Theme::text_caption()).child(label))
                .child(div().flex_1().min_w_0().text_size(px(13.)).line_height(px(20.)).child(value))
                .into_any_element()
        };
        let type_button = div().id("inspector-type").flex().items_center().gap(px(4.)).cursor_pointer().rounded(px(4.))
            .active(|s| s.bg(Theme::fill_pressed()))
            .child(kind.label)
            .child(icon(Icon::ChevronDown, 10., Theme::text_caption()))
            .on_click(cx.listener(|w, ev: &ClickEvent, _, cx| {
                if let Some(inspector) = &mut w.citations.inspector { inspector.type_menu = Some(ev.position()); }
                cx.notify();
            }));
        Some(div().id("inspector").w(px(INSPECTOR_W)).flex_none().h_full().bg(Theme::fill_group()).border_l_1().border_color(list_line()).px(px(12.)).pb(px(16.)).overflow_y_scroll().flex().flex_col()
            .child(row("Type".into(), type_button.into_any_element()))
            .children(inspector.fields.iter().map(|(field, entity)| row(field.label.into(), entity.clone().into_any_element())))
            .child(row("Citation Key".into(), inspector.key_field.clone().into_any_element()))
            .child(div().mt(px(14.)).mb(px(2.)).pl(px(4.)).text_size(px(12.)).text_color(Theme::text_caption()).child("Cited In"))
            .when(inspector.cited_in.is_empty(), |d| d.child(div().pl(px(4.)).text_size(px(13.)).text_color(Theme::text_placeholder()).child("No document yet")))
            .children(inspector.cited_in.iter().enumerate().map(|(i, (id, count, first))| {
                let (id, offset) = (id.clone(), *first);
                div().id(("cited-in", i)).h(px(28.)).px(px(4.)).flex().items_center().gap(px(8.)).rounded(px(crate::ui::ROW_RADIUS)).cursor_pointer().text_size(px(13.))
                    .active(|s| s.bg(Theme::fill_pressed()))
                    .child(div().flex_1().min_w_0().truncate().child(ui_text(id.clone())))
                    .child(div().flex_none().text_size(px(12.)).text_color(Theme::text_caption()).child(count.to_string()))
                    .on_click(cx.listener(move |w, _, window, cx| w.go_to_source(&id, offset, window, cx)))
            }))
            .when_some(inspector.error.clone(), |d, error| d.child(div().mt(px(8.)).text_size(px(12.)).line_height(px(16.)).text_color(theme::c(craft::R400)).child(error)))
            .into_any_element())
    }

    pub(crate) fn render_inspector_type_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.view != View::Library { return None; }
        let inspector = self.citations.inspector.as_ref()?;
        let position = inspector.type_menu?;
        let current = self.citations.library.as_ref()?.source_type(&inspector.key)?;
        let rows = SOURCE_TYPES.iter().enumerate().map(|(i, kind)| {
            menu_row(("inspector-type-row", i))
                .child(div().w(px(16.)).flex_none().when(std::ptr::eq(kind, current), |d| d.child(menu_glyph(Icon::Check, 14.))))
                .child(menu_label(kind.label))
                .on_click(cx.listener(move |w, _, window, cx| w.choose_source_type(kind, window, cx)))
        }).collect::<Vec<_>>();
        let menu = popover().id("inspector-type-menu").occlude().w(px(220.)).max_h(px(420.)).overflow_y_scroll().py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                if let Some(inspector) = &mut w.citations.inspector { inspector.type_menu = None; }
                cx.notify();
            }))
            .children(rows);
        Some(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }
}
