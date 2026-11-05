//! The Library page, its entry form, the citation picker, and keeping each
//! document's bibliography in step with the library.

use crate::app::{View, Workspace};
use crate::input::{TextField, TextFieldEvent};
use crate::library::{self, Fields, Library, Mode, Row};
use crate::store::natural_cmp;
use crate::theme::{self, Theme, craft};
use crate::trash::{dialog, list_header_text, list_line, page_header, primary_button, row_hover, scrim, secondary_button};
use crate::ui::{TipExt, TitleBarExt, ToastKind, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::collections::BTreeSet;
use std::path::Path;
use thesio_core::references::{cited_keys, included_files};

const ROW_H: f32 = 36.;
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

fn matches(row: &Row, query: &str) -> bool {
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
    _subs: Vec<Subscription>,
}

impl Citations {
    pub fn new(root: &Path, cx: &mut Context<Workspace>) -> Self {
        let search = cx.new(|cx| {
            let mut f = TextField::new("Search library", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![cx.subscribe(&search, |_, _, ev: &TextFieldEvent, cx| if matches!(ev, TextFieldEvent::Changed) { cx.notify(); })];
        let (library, error) = match Library::load(root) {
            Ok(library) => (Some(library), None),
            Err(e) => (None, Some(e.to_string())),
        };
        Self { library, error, search, sort: (Col::Title, true), form: None, picker: None, _subs: subs }
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
        self.flush(cx);
        self.ensure_bibliography(&id, cx);
        self.mark_unsaved(cx);
        editor.read(cx).focus(window);
        cx.notify();
    }

    pub(crate) fn open_cite_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                    if let Some(row) = w.picker_rows(cx).get(selected).cloned() {
                        w.close_cite_picker(window, cx);
                        w.insert_citation(&row.key, window, cx);
                    }
                }
                _ => {}
            }
            cx.notify();
        })];
        window.focus(&field.focus_handle(cx));
        self.citations.picker = Some(CitePicker { field, selected: 0, _subs: subs });
        cx.notify();
    }

    fn close_cite_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.citations.picker = None;
        if let Some(editor) = &self.editor { editor.read(cx).focus(window); }
        cx.notify();
    }

    fn picker_rows(&self, cx: &App) -> Vec<Row> {
        let query = self.citations.picker.as_ref().map_or(String::new(), |p| p.field.read(cx).text.trim().to_lowercase());
        let mut rows = self.visible_rows(&query);
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
                .child(div().text_size(px(12.)).line_height(px(16.)).truncate().text_color(Theme::text_caption()).child(format!("{}  {}  @{}", row.authors, row.year, row.key)))
                .on_click(cx.listener(move |w, _, window, cx| {
                    w.close_cite_picker(window, cx);
                    w.insert_citation(&key, window, cx);
                }))
        }).collect::<Vec<_>>();
        let panel = dialog("cite-picker").w(px(520.)).p(px(12.)).flex().flex_col().gap(px(8.))
            .child(div().h(px(32.)).px(px(10.)).flex().items_center().rounded(px(9.)).bg(Theme::fill_hover()).text_size(px(14.)).child(picker.field.clone()))
            .when(empty, |d| d.child(div().px(px(12.)).py(px(8.)).text_size(px(13.)).text_color(Theme::text_caption()).child("No matching entries")))
            .children(list);
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
            }
            Err(e) => if let Some(form) = &mut self.citations.form { form.error = Some(e.to_string()); },
        }
        cx.notify();
    }

    fn delete_entry(&mut self, key: &str, cx: &mut Context<Self>) {
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

    pub(crate) fn render_library(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.citations.search.read(cx).text.trim().to_lowercase();
        let rows = self.visible_rows(&query);
        let total = self.citations.library.as_ref().map_or(0, Library::len);
        let can_edit = self.citations.library.is_some();
        let header = page_header(self, "Library", 26., cx)
            .child(div().flex_1())
            .child(div().id("library-search").no_drag().w(px(220.)).h(px(30.)).mr(px(8.)).px(px(10.)).flex().items_center().rounded(px(9.)).bg(Theme::fill_hover()).text_size(px(13.)).child(self.citations.search.clone()))
            .when(can_edit, |d| d
                .child(secondary_button("library-import", "Import").mr(px(8.)).tip("Import a BibLaTeX file", None).on_click(cx.listener(|w, _, window, cx| w.import_bib(window, cx))))
                .child(secondary_button("library-export", "Export").mr(px(8.)).tip("Export the library as BibLaTeX", None).on_click(cx.listener(|w, _, _, cx| w.export_bib(cx))))
                .child(primary_button("library-add", "Add").on_click(cx.listener(|w, _, window, cx| w.open_entry(None, window, cx)))));
        let mut page = div().flex_1().min_w_0().h_full().flex().flex_col().child(header);
        if let Some(error) = &self.citations.error {
            return page.child(div().p(px(26.)).text_size(px(13.)).text_color(Theme::danger())
                .child(format!("{error}. Fix the file to use the Library; Thesio won't change it until it can be read."))).into_any_element();
        }
        if total == 0 {
            return page.child(crate::trash::empty_state("illus/empty-list.svg", "Add an entry or import a BibLaTeX file")).into_any_element();
        }
        let (sort_col, ascending) = self.citations.sort;
        let columns = COLS.iter().map(|(col, label, width)| {
            let col = *col;
            let arrow = if sort_col != col { "" } else if ascending { " ↑" } else { " ↓" };
            div().id(*label)
                .when_some(*width, |d, w| d.w(px(w)).flex_none())
                .when(width.is_none(), |d| d.flex_1().min_w_0())
                .pl(px(12.)).text_size(px(13.)).line_height(px(24.)).cursor_pointer().text_color(list_header_text())
                .child(format!("{label}{arrow}"))
                .on_click(cx.listener(move |w, _, _, cx| {
                    let (current, ascending) = w.citations.sort;
                    w.citations.sort = (col, current != col || !ascending);
                    cx.notify();
                }))
        }).collect::<Vec<_>>();
        let table_header = div().h(px(33.)).flex_none().mx(px(24.)).flex().items_center().border_b_1().border_color(list_line()).children(columns).child(div().w(px(56.)).flex_none());
        let count = rows.len();
        let can_cite = self.current.as_deref().is_some_and(|id| id.ends_with(".typ"));
        let table = uniform_list("library-rows", count, cx.processor(move |_: &mut Workspace, range: std::ops::Range<usize>, _, cx| {
            range.map(|i| {
                let row = &rows[i];
                let (open, cite) = (row.key.clone(), row.key.clone());
                let cells = COLS.iter().map(|(col, _, width)| {
                    div().when_some(*width, |d, w| d.w(px(w)).flex_none())
                        .when(width.is_none(), |d| d.flex_1().min_w_0())
                        .pl(px(12.)).truncate().text_size(px(13.))
                        .text_color(if *col == Col::Title { Theme::text() } else { Theme::text_caption() })
                        .child(ui_text(cell(row, *col).to_string()))
                }).collect::<Vec<_>>();
                div().id(("library-row", i)).group("library-row").w_full().h(px(ROW_H)).px(px(24.)).flex().items_center().cursor_pointer()
                    .hover(row_hover)
                    .children(cells)
                    .child(div().w(px(56.)).flex_none().flex().justify_end().pr(px(8.)).when(can_cite, |d| d.child(
                        div().id(("library-cite", i)).px(px(8.)).h(px(24.)).flex().items_center().rounded(px(6.)).text_size(px(12.))
                            .opacity(0.).group_hover("library-row", |s| s.opacity(1.))
                            .hover(|s| s.bg(Theme::fill_hover_strong()))
                            .child("Cite")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |w, _, window, cx| {
                                cx.stop_propagation();
                                w.insert_citation(&cite, window, cx);
                            })),
                    )))
                    .on_click(cx.listener(move |w, _, window, cx| w.open_entry(Some(open.clone()), window, cx)))
            }).collect()
        })).flex_1().min_h_0();
        page = page.child(table_header).child(table);
        page.into_any_element()
    }

    pub(crate) fn render_entry_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.citations.form.as_ref()?;
        let rows = form.fields.iter().map(|(label, _, field)| {
            div().flex().items_center().gap(px(12.))
                .child(div().w(px(96.)).flex_none().text_size(px(13.)).text_color(Theme::text_caption()).child(*label))
                .child(div().flex_1().min_w_0().h(px(30.)).px(px(10.)).flex().items_center().rounded(px(8.)).border_1().border_color(Theme::line_border()).text_size(px(13.)).child(field.clone()))
        }).collect::<Vec<_>>();
        let original = form.original.clone();
        let close = |w: &mut Workspace, cx: &mut Context<Workspace>| {
            w.citations.form = None;
            cx.notify();
        };
        let panel = dialog("entry-form").w(px(560.)).px(px(24.)).pt(px(20.)).pb(px(24.)).flex().flex_col().gap(px(10.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().mb(px(4.)).text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(if original.is_some() { "Edit entry" } else { "New entry" }))
            .children(rows)
            .when_some(form.error.clone(), |d, error| d.child(div().text_size(px(13.)).text_color(theme::c(craft::R400)).child(error)))
            .child(div().mt(px(10.)).flex().items_center().gap(px(12.))
                .when_some(original, |d, key| d.child(secondary_button("entry-delete", "Delete").text_color(Theme::danger()).on_click(cx.listener(move |w, _, _, cx| w.delete_entry(&key, cx)))))
                .child(div().flex_1())
                .child(secondary_button("entry-cancel", "Cancel").on_click(cx.listener(move |w, _, _, cx| close(w, cx))))
                .child(primary_button("entry-save", "Save").on_click(cx.listener(|w, _, window, cx| w.save_entry(window, cx)))));
        Some(deferred(div().id("entry-scrim").absolute().inset_0().bg(scrim()).flex().items_center().justify_center().occlude().child(panel)).with_priority(3).into_any_element())
    }
}
