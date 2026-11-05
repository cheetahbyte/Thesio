//! The Library page, its entry form, the citation picker, and keeping each
//! document's bibliography in step with the library.

use crate::app::{Confirm, View, Workspace};
use crate::input::{TextField, TextFieldEvent};
use crate::app::RowMenu;
use crate::library::{self, Added, Fields, Library, Mode, PAPERS_DIR, Row, paper_file};
use thesio_core::lookup::{self, Identifier};
use crate::store::natural_cmp;
use crate::theme::{self, Theme, craft};
use crate::trash::{dialog, list_header_text, list_line, menu_label, page_header, primary_button, row_hover, scrim, secondary_button};
use crate::ui::{Icon, TipExt, TipSide, TitleBarExt, ToastKind, field_well, icon, menu_glyph, menu_row, popover, primary_icon_button, search_well, toolbar_icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use thesio_core::references::{cited_keys, included_files};

const ROW_H: f32 = 36.;
/// Rows inset their hover fill, so cell text lines up with the page title.
const TABLE_X: f32 = 14.;
const CELL_X: f32 = 12.;
const CITE_W: f32 = 56.;
const ADD_W: f32 = 70.;
/// Rows of the entry form, grouped; the numbers count `EntryForm::fields`.
const FORM_GROUPS: [usize; 4] = [2, 3, 4, 2];
const PICKER_ROWS: usize = 8;
/// Fixed columns, widest to narrowest; the title takes what is left.
const COLS: [(Col, &str, Option<f32>); 5] = [
    (Col::Title, "Title", None),
    (Col::Authors, "Authors", Some(200.)),
    (Col::Year, "Year", Some(64.)),
    (Col::Kind, "Type", Some(96.)),
    (Col::Key, "Key", Some(150.)),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col { Title, Authors, Year, Kind, Key }

fn cell(row: &Row, col: Col) -> &str {
    match col {
        Col::Title => &row.title,
        Col::Authors => &row.authors,
        Col::Year => &row.year,
        Col::Kind => &row.kind,
        Col::Key => &row.key,
    }
}

/// A borderless dialog action, beside the buttons but quieter than them.
fn plain_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div().id(id).flex_none().h(px(crate::ui::BUTTON_H)).px(px(10.)).flex().items_center().rounded(px(crate::ui::BUTTON_RADIUS)).cursor_pointer()
        .text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::MEDIUM).text_color(Theme::primary())
        .hover(|s| s.bg(Theme::fill_hover())).active(|s| s.bg(Theme::fill_pressed())).child(label)
}

pub(crate) fn matches(row: &Row, query: &str) -> bool {
    [&row.title, &row.authors, &row.year, &row.key].iter().any(|text| text.to_lowercase().contains(query))
}

/// The add and edit dialog.
pub struct EntryForm {
    original: Option<String>,
    fields: Vec<(&'static str, &'static str, Entity<TextField>)>,
    error: Option<String>,
    _subs: Vec<Subscription>,
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
    form: Option<EntryForm>,
    picker: Option<CitePicker>,
    lookup: Option<Lookup>,
    /// The toolbar's Import and Export menu.
    menu: bool,
    /// The context menu: an entry's key and where it was opened.
    row_menu: Option<(String, Point<Pixels>)>,
    /// The Unpaywall email in Settings.
    pub email: Entity<TextField>,
    /// Papers still downloading.
    fetching: usize,
    _subs: Vec<Subscription>,
}

impl Citations {
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
        Self { library, error, search, sort: (Col::Title, true), form: None, picker: None, lookup: None, menu: false, row_menu: None, email, fetching: 0, _subs: subs }
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
            let order = natural_cmp(cell(a, col), cell(b, col));
            if ascending { order } else { order.reverse() }
        });
        rows
    }

    /// Gives the editor the library as it is on disk now.
    pub(crate) fn offer_sources(&mut self, cx: &mut Context<Self>) {
        self.reload_library();
        let rows = self.visible_rows("");
        if let Some(editor) = &self.editor { editor.update(cx, |e, cx| { e.sources = rows; cx.notify(); }); }
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

    pub(crate) fn render_cite_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.citations.picker.as_ref()?;
        let rows = self.picker_rows(cx);
        let empty = rows.is_empty();
        let list = rows.into_iter().enumerate().map(|(i, row)| {
            let key = row.key.clone();
            div().id(("cite-row", i)).px(px(12.)).py(px(6.)).rounded(px(8.)).cursor_pointer().flex().flex_col()
                .when(i == picker.selected, |d| d.bg(Theme::fill_hover_strong()))
                .hover(|s| s.bg(Theme::fill_hover()))
                .child(div().text_size(px(13.)).line_height(px(18.)).truncate().child(ui_text(if row.title.is_empty() { row.key.clone() } else { row.title.clone() })))
                .child(div().text_size(px(12.)).line_height(px(16.)).truncate().text_color(Theme::text_caption()).child(format!("{} · {} · @{}", row.authors, row.year, row.key)))
                .on_click(cx.listener(move |w, _, window, cx| w.pick_source(&key, window, cx)))
        }).collect::<Vec<_>>();
        let hint = (!matches!(picker.purpose, PickFor::Cite)).then(|| div().px(px(4.)).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Choose the source for this paper"));
        let unlinked = if let PickFor::Attach(path) = &picker.purpose { Some(path.clone()) } else { None };
        let panel = dialog("cite-picker").w(px(520.)).p(px(12.)).flex().flex_col().gap(px(8.))
            .children(hint)
            .child(search_well(picker.field.clone().into()))
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

    fn open_entry(&mut self, key: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let values = key.as_deref().and_then(|k| self.citations.library.as_ref()?.fields(k)).unwrap_or(Fields { kind: "Article".into(), ..Default::default() });
        let spec: [(&'static str, &'static str, String); 11] = [
            ("Key", "knuth1984", values.key),
            ("Type", "Article, Book, Thesis, Web, Report…", values.kind),
            ("Title", "", values.title),
            ("Authors", "Family, Given; Family, Given", values.authors),
            ("Year", "", values.year),
            ("Published in", "Journal, book, or conference", values.container),
            ("Publisher", "", values.publisher),
            ("Volume", "", values.volume),
            ("Pages", "891-921", values.pages),
            ("DOI", "", values.doi),
            ("URL", "", values.url),
        ];
        let mut fields = vec![];
        let mut subs = vec![];
        for (label, hint, text) in spec {
            let field = cx.new(|cx| {
                let mut f = TextField::new(hint, cx);
                f.placeholder_color = Theme::text_disabled();
                f.tab_events = true;
                f.set_text(&text, cx);
                f
            });
            subs.push(cx.subscribe_in(&field, window, |w, field, ev: &TextFieldEvent, window, cx| {
                let step = match ev {
                    TextFieldEvent::Submit => return w.save_entry(window, cx),
                    TextFieldEvent::Cancel => {
                        w.citations.form = None;
                        return cx.notify();
                    }
                    TextFieldEvent::Tab => 1,
                    TextFieldEvent::ShiftTab => -1,
                    _ => return,
                };
                let Some(form) = &w.citations.form else { return };
                let Some(at) = form.fields.iter().position(|(_, _, f)| f == field) else { return };
                let next = (at as i32 + step).rem_euclid(form.fields.len() as i32) as usize;
                window.focus(&form.fields[next].2.focus_handle(cx));
            }));
            fields.push((label, hint, field));
        }
        window.focus(&fields[if key.is_some() { 2 } else { 0 }].2.focus_handle(cx));
        self.citations.form = Some(EntryForm { original: key, fields, error: None, _subs: subs });
        cx.notify();
    }

    fn save_entry(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.citations.form else { return };
        let value = |i: usize| form.fields[i].2.read(cx).text.clone();
        let fields = Fields {
            key: value(0), kind: value(1), title: value(2), authors: value(3), year: value(4), container: value(5),
            publisher: value(6), volume: value(7), pages: value(8), doi: value(9), url: value(10),
        };
        let original = form.original.clone();
        let Some(library) = &mut self.citations.library else { return };
        match library.save_entry(original.as_deref(), &fields) {
            Ok(()) => {
                self.citations.form = None;
                self.refresh_after_library_change(cx);
                let key = fields.key.trim().to_string();
                if let Some(old) = original.filter(|old| *old != key) { self.rename_paper(&old, &key, cx); }
                self.fetch_papers(vec![key], cx);
            }
            Err(e) => if let Some(form) = &mut self.citations.form { form.error = Some(e.to_string()); },
        }
        cx.notify();
    }

    pub(crate) fn delete_entry(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(library) = &mut self.citations.library else { return };
        match library.delete_entry(key) {
            Ok(()) => {
                self.citations.form = None;
                self.refresh_after_library_change(cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not delete entry: {e}"), cx),
        }
        cx.notify();
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

    pub(crate) fn render_lookup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let lookup = self.citations.lookup.as_ref()?;
        let problems = lookup.problems.iter().map(|p| div().text_size(px(13.)).line_height(px(18.)).text_color(theme::c(craft::R400)).child(p.clone())).collect::<Vec<_>>();
        let panel = dialog("lookup").w(px(480.)).p(px(20.)).flex().flex_col().gap(px(10.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Add by identifier"))
            .child(div().text_size(px(13.)).line_height(px(18.)).text_color(Theme::text_caption()).child("Enter one or more DOIs, arXiv IDs, PMIDs, or ISBNs. Each is sent to doi.org, NCBI, or Open Library, and Thesio looks for an open-access PDF through arXiv and Unpaywall."))
            .child(field_well().child(lookup.field.clone()))
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
                let before: BTreeSet<String> = w.citations.library.iter().flat_map(Library::rows).map(|row| row.key).collect();
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
                    let keys = w.citations.library.iter().flat_map(Library::rows).map(|row| row.key).filter(|key| !before.contains(key)).collect();
                    w.fetch_papers(keys, cx);
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

    pub(crate) fn render_library(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.citations.search.read(cx).text.trim().to_lowercase();
        let rows = self.visible_rows(&query);
        let total = self.citations.library.as_ref().map_or(0, Library::len);
        let can_edit = self.citations.library.is_some();
        let header = page_header(self, "Library", (total > 0).then_some(total), 26., cx)
            .child(div().id("library-search").no_drag().w(px(220.)).mr(px(4.)).child(search_well(self.citations.search.clone().into())))
            .when(can_edit, |d| d
                .child(toolbar_icon("library-more", Icon::Ellipsis).no_drag().mr(px(8.)).when(self.citations.menu, |d| d.bg(Theme::fill_hover_strong())).tip("Import and export", None)
                    .on_click(cx.listener(|w, _, _, cx| { w.citations.menu = !w.citations.menu; cx.notify(); })))
                .child(primary_icon_button("library-add", Icon::Plus, "Add").no_drag().on_click(cx.listener(|w, _, window, cx| w.open_lookup(window, cx)))));
        let mut page = div().flex_1().min_w_0().h_full().flex().flex_col().child(header);
        if let Some(error) = &self.citations.error {
            return page.child(div().p(px(26.)).text_size(px(13.)).text_color(Theme::danger())
                .child(format!("{error}. Fix the file to use the Library; Thesio won't change it until it can be read."))).into_any_element();
        }
        if total == 0 {
            return page.child(crate::trash::empty_state(Icon::Library, "Your library is empty", "Add an entry by identifier, or import a BibLaTeX file.")).into_any_element();
        }
        let (sort_col, ascending) = self.citations.sort;
        let columns = COLS.iter().map(|(col, label, width)| {
            let col = *col;
            let sorted = sort_col == col;
            div().id(*label)
                .when_some(*width, |d, w| d.w(px(w)).flex_none())
                .when(width.is_none(), |d| d.flex_1().min_w_0())
                .pr(px(CELL_X)).flex().items_center().gap(px(4.)).text_size(px(12.)).line_height(px(24.)).cursor_pointer()
                .font_weight(FontWeight::MEDIUM).text_color(if sorted { Theme::text() } else { list_header_text() })
                .child(*label)
                .when(sorted, |d| d.child(icon(if ascending { Icon::ChevronUp } else { Icon::ChevronDown }, 10., Theme::text_caption())))
                .on_click(cx.listener(move |w, _, _, cx| {
                    let (current, ascending) = w.citations.sort;
                    w.citations.sort = (col, current != col || !ascending);
                    cx.notify();
                }))
        }).collect::<Vec<_>>();
        let table_header = div().h(px(30.)).flex_none().mx(px(TABLE_X)).px(px(CELL_X)).flex().items_center().border_b_1().border_color(list_line()).children(columns).child(div().w(px(CITE_W)).flex_none());
        let count = rows.len();
        if count == 0 {
            return page.child(table_header).child(crate::trash::empty_state(Icon::Search, "No results", "Try a different title, author, year, or key.")).into_any_element();
        }
        let can_cite = self.can_cite();
        let menu_key = self.citations.row_menu.as_ref().map(|(key, _)| key.clone());
        let papers = self.paper_ids();
        let table = uniform_list("library-rows", count, cx.processor(move |_: &mut Workspace, range: std::ops::Range<usize>, _, cx| {
            range.map(|i| {
                let row = &rows[i];
                let (open, cite, menu) = (row.key.clone(), row.key.clone(), row.key.clone());
                let cells = COLS.iter().map(|(col, _, width)| {
                    let paper = *col == Col::Title && papers.contains(&paper_file(&row.key));
                    div().when_some(*width, |d, w| d.w(px(w)).flex_none())
                        .when(width.is_none(), |d| d.flex_1().min_w_0())
                        .pr(px(CELL_X)).flex().items_center().gap(px(6.)).text_size(px(13.))
                        .when(*col == Col::Key, |d| d.font_family(theme::MONO_FONT).text_size(px(12.)))
                        .text_color(if *col == Col::Title { Theme::text() } else { Theme::text_caption() })
                        .child(div().min_w_0().truncate().child(ui_text(cell(row, *col).to_string())))
                        .when(paper, |d| d.child(icon(Icon::Paper, 13., Theme::text_placeholder())))
                }).collect::<Vec<_>>();
                div().id(("library-row", i)).group("library-row").w_full().h(px(ROW_H)).px(px(CELL_X)).flex().items_center().cursor_pointer().rounded(px(crate::ui::ROW_RADIUS))
                    .hover(row_hover)
                    .when(menu_key.as_deref() == Some(row.key.as_str()), |d| d.bg(Theme::fill_hover()))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                        w.citations.row_menu = Some((menu.clone(), ev.position));
                        cx.notify();
                    }))
                    .children(cells)
                    .child(div().w(px(CITE_W)).flex_none().flex().justify_end().when(can_cite, |d| d.child(
                        div().id(("library-cite", i)).px(px(10.)).h(px(24.)).flex().items_center().rounded(px(6.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM)
                            .bg(Theme::fill_hover_strong())
                            .opacity(0.).group_hover("library-row", |s| s.opacity(1.))
                            .hover(|s| s.bg(Theme::fill_pressed()))
                            .child("Cite")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |w, _, window, cx| {
                                cx.stop_propagation();
                                w.insert_citation(&cite, window, cx);
                            })),
                    )))
                    .on_click(cx.listener(move |w, _, window, cx| w.open_entry(Some(open.clone()), window, cx)))
            }).collect()
        })).flex_1().min_h_0().px(px(TABLE_X)).pt(px(4.));
        page = page.child(table_header).child(table);
        page.into_any_element()
    }

    /// The ids of the papers in the vault.
    fn paper_ids(&self) -> BTreeSet<String> {
        self.entries.iter().filter(|e| !e.folder && e.meta.parent.as_deref() == Some(PAPERS_DIR)).map(|e| e.id.clone()).collect()
    }

    fn has_paper(&self, key: &str) -> bool {
        let file = paper_file(key);
        self.entries.iter().any(|e| e.id == file)
    }

    /// Shows a paper in the app, or where there is no viewer, in the system's app.
    pub(crate) fn open_paper(&mut self, id: &str, cx: &mut Context<Self>) {
        if !crate::native_pdf::supported() { return self.open_paper_externally(id, cx); }
        self.paper = Some(id.to_string());
        self.paper_failed.set(false);
        self.show_view(View::Paper, cx);
    }

    /// Scripted runs must not start other apps.
    fn open_paper_externally(&self, id: &str, cx: &App) {
        if std::env::var_os("THESIO_SCRIPT").is_none() { cx.open_with_system(&self.store.root().join(id)); }
    }

    /// The name a paper goes by: its source's title, or its file name.
    fn paper_title(&self, id: &str) -> String {
        let row = self.citations.library.as_ref().and_then(|library| library.rows().into_iter().find(|row| paper_file(&row.key) == id));
        row.map(|row| row.title).filter(|title| !title.is_empty()).unwrap_or_else(|| id.rsplit('/').next().unwrap_or(id).to_string())
    }

    /// The viewer goes under what GPUI draws over the pane, or it would hide it.
    fn paper_covered(&self, window: &Window) -> bool {
        let c = &self.citations;
        // Most row menus stay over the sidebar, clear of the pane.
        let edge = if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        self.row_menu.as_ref().is_some_and(|menu| f32::from(menu.pos.x) + crate::sidebar::ROW_MENU_W > edge) || self.confirm.is_some() || self.search_open || self.move_dlg.is_some() || c.picker.is_some() || c.form.is_some() || c.lookup.is_some()
    }

    pub(crate) fn render_paper(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(id) = self.paper.clone() else { return div().into_any_element() };
        let title = self.paper_title(&id);
        let external = id.clone();
        let header = page_header(self, &title, None, 26., cx)
            .child(toolbar_icon("paper-external", Icon::External).no_drag().tip_label("Open in Default App".to_string(), TipSide::Right)
                .on_click(cx.listener(move |w, _, _, cx| w.open_paper_externally(&external, cx))));
        let (pdf, covered, path) = (self.pdf.clone(), self.paper_covered(window), self.store.path(&id).ok());
        let failed = self.paper_failed.clone();
        let unreadable = failed.get();
        let pane = canvas(|_, _, _| (), move |bounds, _, window, _| {
            let shown = path.as_ref().is_some_and(|path| pdf.show(path, bounds, covered, window));
            if !shown && !failed.replace(true) { window.on_next_frame(|window, _| window.refresh()); }
        }).absolute().size_full();
        div().flex_1().min_w_0().h_full().flex().flex_col().child(header)
            .child(div().relative().flex_1().min_h_0().flex().items_center().justify_center().bg(Theme::bg_overlay())
                .child(pane)
                .when(unreadable, |d| d.child(div().text_size(px(13.)).text_color(Theme::text_caption()).child("This PDF could not be shown here. Try opening it in your default app."))))
            .into_any_element()
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

    /// Makes an unlinked paper the paper of `key`.
    fn link_paper(&mut self, id: &str, key: &str, cx: &mut Context<Self>) {
        let to = paper_file(key);
        let name = to.rsplit('/').next().unwrap_or(&to).to_string();
        match self.store.rename(id, &name) {
            Ok((new, rewritten)) => self.path_changed(id, &new, &rewritten, cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not link the paper: {e}"), cx),
        }
        self.entries = self.store.list();
        self.sort();
        cx.notify();
    }

    /// A paper keeps following its entry when the citation key changes.
    fn rename_paper(&mut self, old: &str, new: &str, cx: &mut Context<Self>) {
        let (from, to) = (paper_file(old), paper_file(new));
        if from == to || !self.entries.iter().any(|e| e.id == from) { return; }
        let name = to.rsplit('/').next().unwrap_or(&to).to_string();
        match self.store.rename(&from, &name) {
            Ok((id, rewritten)) => self.path_changed(&from, &id, &rewritten, cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not rename the paper: {e}"), cx),
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
        let table_header = div().h(px(30.)).flex_none().mx(px(TABLE_X)).px(px(CELL_X)).flex().items_center().border_b_1().border_color(list_line())
            .text_size(px(12.)).line_height(px(24.)).font_weight(FontWeight::MEDIUM).text_color(list_header_text())
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
                    .hover(row_hover)
                    .child(cell(None, title.clone()).text_color(Theme::text()))
                    .child(cell(Some(200.), text(|row| &row.authors)))
                    .child(cell(Some(64.), text(|row| &row.year)))
                    .when(!unlinked, |d| d.child(cell(Some(150.), text(|row| &row.key)).font_family(theme::MONO_FONT).text_size(px(12.))))
                    .when(unlinked, |d| d.child(div().w(px(150.)).flex_none().flex().child(
                        div().id(("source-link", i)).px(px(10.)).h(px(24.)).flex().items_center().rounded(px(6.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(Theme::text())
                            .bg(Theme::fill_hover_strong()).hover(|s| s.bg(Theme::fill_pressed()))
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
            let g = SharedString::from(id);
            menu_row(id, g.clone()).child(menu_glyph(glyph, 16., g.clone())).child(menu_label(label, g, gpui::white()))
        };
        let (edit, cite, copy, paper, attach, delete) = (key.clone(), key.clone(), key.clone(), key.clone(), key.clone(), key);
        let has_paper = self.has_paper(&paper);
        let g = SharedString::from("entry-menu-delete");
        let menu = popover().id("entry-menu").occlude().w(px(180.)).py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.citations.row_menu = None; cx.notify(); }))
            .child(item("entry-menu-edit", Icon::Edit, "Edit").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                w.open_entry(Some(edit.clone()), window, cx);
            })))
            .when(self.can_cite(), |d| d.child(item("entry-menu-cite", Icon::Quote, "Cite").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                w.insert_citation(&cite, window, cx);
            }))))
            .child(item("entry-menu-paper", if has_paper { Icon::Paper } else { Icon::Search }, if has_paper { "Open Paper" } else { "Find Paper" }).on_click(cx.listener(move |w, _, _, cx| {
                w.citations.row_menu = None;
                if has_paper { w.open_paper(&paper_file(&paper), cx); } else { w.fetch_papers(vec![paper.clone()], cx); }
                cx.notify();
            })))
            .when(!has_paper, |d| d.child(item("entry-menu-attach", Icon::Import, "Attach Paper…").on_click(cx.listener(move |w, _, window, cx| {
                w.citations.row_menu = None;
                let id = paper_file(&attach);
                w.choose_pdf(window, cx, move |w, path, _, cx| w.attach_paper(&path, &id, cx));
            }))))
            .child(item("entry-menu-copy", Icon::Copy, "Copy Key").on_click(cx.listener(move |w, _, _, cx| {
                w.citations.row_menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                cx.notify();
            })))
            .child(div().mx(px(13.)).my(px(5.)).h(px(1.)).bg(Theme::line_border()))
            .child(menu_row("entry-menu-delete", g.clone()).text_color(Theme::danger())
                .child(icon(Icon::Delete, 16., Theme::danger()).group_hover(g.clone(), |s| s.text_color(gpui::white())))
                .child(menu_label("Delete", g, gpui::white()))
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.citations.row_menu = None;
                    w.confirm = Some(Confirm::DeleteEntry(delete.clone()));
                    cx.notify();
                })));
        Some(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }

    pub(crate) fn render_library_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.citations.menu || self.view != View::Library { return None; }
        let item = |id: &'static str, glyph: Icon, label: &'static str| {
            let g = SharedString::from(id);
            menu_row(id, g.clone()).child(menu_glyph(glyph, 16., g.clone())).child(menu_label(label, g, gpui::white()))
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
            })));
        // Under the "…" button, which sits left of the Add button.
        let right = window.viewport_size().width - px(crate::ui::header_right_inset() + ADD_W);
        Some(deferred(anchored().anchor(Corner::TopRight).position(point(right, px(crate::app::HEADER_H - 4.))).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }

    pub(crate) fn render_entry_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.citations.form.as_ref()?;
        let mut fields = form.fields.iter();
        let groups = FORM_GROUPS.iter().map(|n| {
            let rows = fields.by_ref().take(*n).map(|(label, _, field)| {
                div().w_full().h(px(32.)).px(px(12.)).flex().items_center().gap(px(12.))
                    .child(div().w(px(88.)).flex_none().text_size(px(13.)).text_color(Theme::text_caption()).child(*label))
                    .child(div().flex_1().min_w_0().text_size(px(13.)).child(field.clone()))
                    .into_any_element()
            }).collect();
            crate::settings::group(None, rows)
        }).collect::<Vec<_>>();
        let original = form.original.clone();
        let close = |w: &mut Workspace, cx: &mut Context<Workspace>| {
            w.citations.form = None;
            cx.notify();
        };
        let panel = dialog("entry-form").w(px(520.)).p(px(20.)).flex().flex_col().gap(px(10.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().mb(px(2.)).text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(if original.is_some() { "Edit entry" } else { "New entry" }))
            .children(groups)
            .when_some(form.error.clone(), |d, error| d.child(div().text_size(px(13.)).text_color(theme::c(craft::R400)).child(error)))
            .child(div().mt(px(8.)).flex().items_center().gap(px(8.))
                .when_some(original, |d, key| d.child(plain_button("entry-delete", "Delete").text_color(Theme::danger()).on_click(cx.listener(move |w, _, _, cx| {
                    w.citations.form = None;
                    w.confirm = Some(Confirm::DeleteEntry(key.clone()));
                    cx.notify();
                }))))
                .child(div().flex_1())
                .child(secondary_button("entry-cancel", "Cancel").on_click(cx.listener(move |w, _, _, cx| close(w, cx))))
                .child(primary_button("entry-save", "Save").on_click(cx.listener(|w, _, window, cx| w.save_entry(window, cx)))));
        Some(deferred(div().id("entry-scrim").absolute().inset_0().bg(scrim()).flex().items_center().justify_center().occlude().child(panel)).with_priority(3).into_any_element())
    }
}
