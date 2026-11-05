//! The Library page: the table of entries, its selection, columns, and menus.

use super::inspector::INSPECTOR_W;
use crate::app::{Confirm, Workspace};
use crate::library::{Library, Row, paper_file};
use crate::input::TextField;
use crate::overlay::Overlay;
use crate::theme::{self, Theme};
use crate::trash::{list_header_text, list_line, menu_label, page_header};
use crate::ui::{Icon, TipExt, TitleBarExt, ToastKind, anchored_menu, icon, menu_action, menu_glyph, menu_panel, menu_row, primary_icon_button, search_well, toolbar_group, toolbar_icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use thesio_core::paths::is_document;

pub(crate) const ROW_H: f32 = 32.;
/// Rows inset their cells, so cell text lines up with the page title.
pub(crate) const TABLE_X: f32 = 14.;
pub(crate) const CELL_X: f32 = 12.;
const CITE_W: f32 = 56.;
/// Narrower than this and the title gives up the columns it can spare.
const MIN_TITLE_W: f32 = 220.;
const MIN_COLUMN_W: f32 = 56.;
const MAX_COLUMN_W: f32 = 480.;
const ADD_W: f32 = 68.;
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

/// A column's box in a header or row: a fixed width, or what is left.
pub(crate) fn column(width: Option<f32>) -> Div {
    div().when_some(width, |d, w| d.w(px(w)).flex_none()).when(width.is_none(), |d| d.flex_1().min_w_0()).pr(px(CELL_X))
}

/// A column's label and width before any resizing.
pub(crate) fn column_spec(col: Col) -> (&'static str, Option<f32>) {
    COLS.iter().find(|(c, ..)| *c == col).map_or(("", None), |(_, label, width)| (*label, *width))
}

pub(crate) fn cell(row: &Row, col: Col) -> Cow<'_, str> {
    match col {
        Col::Title => (&row.title).into(),
        Col::Authors => (&row.authors).into(),
        Col::Year => (&row.year).into(),
        Col::Kind => (&row.kind).into(),
        Col::Tags => row.tags.join(", ").into(),
        Col::Key => (&row.key).into(),
    }
}

/// A column being resized: its name, and the pointer and width at the start.
#[derive(Clone, Copy)]
pub(crate) struct ColumnDrag {
    name: &'static str,
    start_x: Pixels,
    start_width: f32,
}

/// The table's state. Its menus are overlays.
pub(crate) struct Table {
    pub(crate) search: Entity<TextField>,
    pub(crate) sort: (Col, bool),
    /// The selected rows, by key.
    pub(crate) selected: BTreeSet<String>,
    /// Where a shift-selection extends from.
    pub(crate) anchor: Option<String>,
    /// The row the arrow keys move from.
    pub(crate) cursor: Option<String>,
    /// Holds the keyboard for the table.
    pub(crate) focus: FocusHandle,
    pub(crate) scroll: UniformListScrollHandle,
    /// Widths of resized columns, by column name.
    pub(crate) widths: BTreeMap<String, f32>,
    pub(crate) column_drag: Option<ColumnDrag>,
}

impl Table {
    pub(crate) fn new(search: Entity<TextField>, cx: &mut Context<Workspace>) -> Self {
        Self { search, sort: (Col::Title, true), selected: BTreeSet::new(), anchor: None, cursor: None, focus: cx.focus_handle(), scroll: UniformListScrollHandle::new(), widths: crate::settings::prefs().library_columns, column_drag: None }
    }
}

impl Workspace {
    /// The table's rows, in order: the search and the tag filter applied.
    pub(crate) fn library_rows(&self, cx: &App) -> Vec<Row> {
        let query = self.citations.table.search.read(cx).text.trim().to_lowercase();
        let mut rows = self.visible_rows(&query);
        if let Some(tag) = &self.citations.tag_filter { rows.retain(|row| row.tags.contains(tag)); }
        rows
    }

    /// The selected keys among `rows`, in table order. Rows a search or filter
    /// hides stay out, so nothing acts on an entry the user cannot see.
    pub(crate) fn selected_in(&self, rows: &[Row]) -> Vec<String> {
        rows.iter().filter(|row| self.citations.table.selected.contains(&row.key)).map(|row| row.key.clone()).collect()
    }

    pub(crate) fn select_only(&mut self, key: &str) {
        self.citations.table.selected = BTreeSet::from([key.to_string()]);
        self.citations.table.anchor = Some(key.to_string());
        self.citations.table.cursor = Some(key.to_string());
    }

    /// Selects from the anchor through `key`.
    pub(crate) fn select_range(&mut self, key: &str, rows: &[Row]) {
        let position = |key: &str| rows.iter().position(|row| row.key == key);
        let Some(end) = position(key) else { return };
        let start = self.citations.table.anchor.as_deref().and_then(position).unwrap_or(end);
        self.citations.table.selected = rows[start.min(end)..=start.max(end)].iter().map(|row| row.key.clone()).collect();
        self.citations.table.cursor = Some(key.to_string());
    }

    pub(crate) fn click_row(&mut self, key: &str, modifiers: Modifiers, cx: &mut Context<Self>) {
        if modifiers.shift {
            self.select_range(key, &self.library_rows(cx));
        } else if modifiers.platform {
            if !self.citations.table.selected.remove(key) { self.citations.table.selected.insert(key.to_string()); }
            self.citations.table.anchor = Some(key.to_string());
            self.citations.table.cursor = Some(key.to_string());
        } else {
            self.select_only(key);
        }
        cx.notify();
    }

    pub(crate) fn library_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke.modifiers;
        let rows = self.library_rows(cx);
        match event.keystroke.key.as_str() {
            step @ ("up" | "down") => {
                let current = self.citations.table.cursor.as_deref().and_then(|key| rows.iter().position(|row| row.key == key));
                let next = match (step, current) {
                    ("up", Some(i)) => i.saturating_sub(1),
                    ("down", Some(i)) => (i + 1).min(rows.len().saturating_sub(1)),
                    _ => 0,
                };
                let Some(key) = rows.get(next).map(|row| row.key.clone()) else { return };
                if keys.shift { self.select_range(&key, &rows); } else { self.select_only(&key); }
                self.citations.table.scroll.scroll_to_item(next, ScrollStrategy::Top);
            }
            "a" if keys.platform => self.citations.table.selected = rows.iter().map(|row| row.key.clone()).collect(),
            "enter" => {
                if let [key] = self.selected_in(&rows).as_slice() { self.open_entry(Some(key.clone()), window, cx); }
            }
            "backspace" | "delete" => {
                let selected = self.selected_in(&rows);
                if !selected.is_empty() { self.open_overlay(Overlay::Confirm(Confirm::DeleteEntries(selected)), window, cx); }
            }
            "escape" => self.citations.table.selected.clear(),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn column_width(&self, name: &str, default: f32) -> f32 {
        self.citations.table.widths.get(name).copied().unwrap_or(default)
    }

    pub(crate) fn import_bib(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn export_bib(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.citations.library.as_ref().map(Library::export_biblatex) else { return };
        let root = self.store.borrow().root().to_path_buf();
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
        let menu_open = matches!(self.overlay, Some(Overlay::LibraryMenu));
        let header = page_header(self, "Library", (total > 0).then_some(total), 26., cx)
            .child(div().id("library-search").no_drag().w(px(220.)).mr(px(8.)).child(search_well(&self.citations.table.search, window, cx)))
            .when(can_edit, |d| d
                .child(toolbar_group().mr(px(8.)).child(toolbar_icon("library-details", Icon::Details).no_drag().tip(if self.citations.details.hidden { "Show details" } else { "Hide details" }, None)
                    .on_click(cx.listener(|w, _, window, cx| w.set_details_hidden(!w.citations.details.hidden, window, cx)))))
                .child(toolbar_group().mr(px(8.)).child(toolbar_icon("library-more", Icon::Ellipsis).no_drag().when(menu_open, |d| d.bg(Theme::fill_hover_strong())).tip("More", None)
                    .on_click(cx.listener(move |w, _, window, cx| if menu_open { w.close_overlay(window, cx); } else { w.open_overlay(Overlay::LibraryMenu, window, cx); }))))
                .child(primary_icon_button("library-add", Icon::Plus, "Add").no_drag().on_click(cx.listener(|w, _, window, cx| w.open_lookup(window, cx)))));
        let table = self.render_library_table(&tags, rows, total, window, cx);
        let inspector = self.render_inspector(cx);
        div().flex_1().min_w_0().h_full().flex().flex_col().child(header)
            .child(div().flex_1().min_h_0().flex().child(table).children(inspector))
            .into_any_element()
    }

    /// The fixed columns that fit beside a usable title, at their current widths.
    pub(crate) fn visible_columns(&self, window: &Window) -> Vec<(Col, &'static str, Option<f32>)> {
        let panel = if self.citations.details.panel.is_some() { INSPECTOR_W } else { 0. };
        let sidebar = if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let room = f32::from(window.viewport_size().width) - sidebar - panel - 2. * (TABLE_X + CELL_X) - CITE_W - MIN_TITLE_W;
        let mut columns: Vec<_> = COLS.iter().map(|(col, label, width)| (*col, *label, width.map(|w| self.column_width(label, w)))).collect();
        for col in [Col::Key, Col::Kind, Col::Tags, Col::Year, Col::Authors] {
            if columns.iter().filter_map(|(.., width)| *width).sum::<f32>() <= room { break; }
            columns.retain(|(c, ..)| *c != col);
        }
        columns
    }

    pub(crate) fn render_library_table(&mut self, tags: &[(String, usize)], rows: Vec<Row>, total: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
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
                    .when_some(filter.clone(), |d, tag| d.on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, window, cx| w.open_overlay(Overlay::TagMenu { tag: tag.clone(), position: ev.position }, window, cx))))
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
        let (sort_col, ascending) = self.citations.table.sort;
        let visible = self.visible_columns(window);
        let dragged = self.citations.table.column_drag.map(|drag| drag.name);
        let headers = visible.iter().enumerate().map(|(i, (col, label, width))| {
            let (col, name) = (*col, *label);
            let sorted = sort_col == col;
            let start_width = *width;
            column(*width).id(*label).relative()
                .flex().items_center().gap(px(4.)).text_size(px(12.)).line_height(px(24.)).cursor_pointer()
                .text_color(if sorted { Theme::text() } else { list_header_text() })
                .child(*label)
                .when(sorted, |d| d.child(icon(if ascending { Icon::ChevronUp } else { Icon::ChevronDown }, 10., Theme::text_caption())))
                .on_click(cx.listener(move |w, _, _, cx| {
                    let (current, ascending) = w.citations.table.sort;
                    w.citations.table.sort = (col, current != col || !ascending);
                    cx.notify();
                }))
                // Columns hang from the right edge, so a column's left edge is what moves.
                .when_some(start_width, |d, start_width| d.child(
                    div().id(("library-column-resize", i)).absolute().top_0().bottom_0().left(px(-(CELL_X + 4.))).w(px(9.)).cursor(CursorStyle::ResizeLeftRight)
                        .child(div().absolute().top_0().bottom_0().left(px(4.)).w(px(1.)).when(dragged == Some(name), |d| d.bg(Theme::primary())))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Left, cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            w.citations.table.column_drag = Some(ColumnDrag { name, start_x: ev.position.x, start_width });
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
        let selected = self.citations.table.selected.clone();
        // As on macOS: the accent while the table has the keyboard, gray once it moves on.
        let active = self.citations.table.focus.is_focused(window);
        let papers = self.paper_ids();
        let table = uniform_list("library-rows", count, cx.processor(move |_: &mut Workspace, range: std::ops::Range<usize>, _, cx| {
            range.map(|i| {
                let row = &rows[i];
                let (open, cite, menu) = (row.key.clone(), row.key.clone(), row.key.clone());
                let on = selected.contains(&row.key);
                let accent = on && active;
                let cells = visible.iter().map(|(col, _, width)| {
                    let paper = *col == Col::Title && papers.contains(&paper_file(&row.key));
                    column(*width).flex().items_center().gap(px(6.)).text_size(px(13.))
                        .when(*col == Col::Key, |d| d.font_family(theme::MONO_FONT).text_size(px(12.)))
                        .text_color(if accent { gpui::white() } else if *col == Col::Title { Theme::text() } else { Theme::text_caption() })
                        .child(div().min_w_0().truncate().child(ui_text(cell(row, *col).to_string())))
                        .when(paper, |d| d.child(icon(Icon::Paper, 13., if accent { gpui::white() } else { Theme::text_placeholder() })))
                }).collect::<Vec<_>>();
                div().id(("library-row", i)).group("library-row").w_full().h(px(ROW_H)).px(px(CELL_X)).flex().items_center().cursor_pointer().rounded(px(crate::ui::ROW_RADIUS))
                    .when(i % 2 == 1, |d| d.bg(Theme::fill_group()))
                    .when(on, |d| d.bg(if active { Theme::primary() } else { Theme::sidebar_selected() }))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, window, cx| {
                        window.focus(&w.citations.table.focus);
                        if !w.citations.table.selected.contains(&menu) { w.select_only(&menu); }
                        w.open_overlay(Overlay::EntryMenu { key: menu.clone(), position: ev.position }, window, cx);
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
                        window.focus(&w.citations.table.focus);
                        if ev.click_count() >= 2 && !ev.modifiers().modified() { w.open_entry(Some(open.clone()), window, cx); } else { w.click_row(&open, ev.modifiers(), cx); }
                    }))
            }).collect()
        })).track_scroll(self.citations.table.scroll.clone()).flex_1().min_h_0().px(px(TABLE_X)).pt(px(4.));
        let table = div().id("library-table").flex_1().min_h_0().flex().flex_col().track_focus(&self.citations.table.focus)
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| w.library_key(ev, window, cx)))
            .child(table);
        page = page.child(table_header).child(table);
        page.into_any_element()
    }

    pub(crate) fn can_cite(&self) -> bool {
        self.current.as_deref().is_some_and(is_document)
    }

    pub(crate) fn render_entry_menu(&self, key: &str, position: Point<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let item = |id: &'static str, glyph: Icon, label: &'static str| {
            menu_row(id).child(menu_glyph(glyph, 16.)).child(menu_label(label))
        };
        let key = key.to_string();
        let (edit, cite, copy, paper, attach, notes) = (key.clone(), key.clone(), key.clone(), key.clone(), key.clone(), key);
        let selected = self.selected_in(&self.library_rows(cx));
        let single = selected.len() == 1;
        let (tagged, delete) = (selected.clone(), selected);
        let delete_label = if single { "Delete".to_string() } else { format!("Delete {} Entries", delete.len()) };
        let has_paper = self.has_paper(&paper);
        let menu = menu_panel("entry-menu", 180., cx)
            .when(single, |d| d.child(item("entry-menu-edit", Icon::Edit, "Edit").on_click(menu_action(cx, move |w, window, cx| w.open_entry(Some(edit.clone()), window, cx)))))
            .child(item("entry-menu-tags", Icon::Tag, "Tags…").on_click(menu_action(cx, move |w, window, cx| w.open_tag_editor(tagged.clone(), position, window, cx))))
            .when(single && self.can_cite(), |d| d.child(item("entry-menu-cite", Icon::Quote, "Cite").on_click(menu_action(cx, move |w, window, cx| w.insert_citation(&cite, window, cx)))))
            .when(single, |d| d.child(item("entry-menu-paper", if has_paper { Icon::Paper } else { Icon::Search }, if has_paper { "Open Paper" } else { "Find Paper" }).on_click(menu_action(cx, move |w, _, cx| {
                if has_paper { w.open_paper(&paper_file(&paper), cx); } else { w.fetch_papers(vec![paper.clone()], cx); }
            }))))
            .when(single && !has_paper, |d| d.child(item("entry-menu-attach", Icon::Import, "Attach Paper…").on_click(menu_action(cx, move |w, window, cx| {
                let id = paper_file(&attach);
                w.choose_pdf(window, cx, move |w, path, _, cx| w.attach_paper(&path, &id, cx));
            }))))
            .when(single, |d| d.child(item("entry-menu-notes", Icon::Notes, "Notes").on_click(menu_action(cx, move |w, window, cx| w.open_source_notes(&notes, window, cx)))))
            .when(single, |d| d.child(item("entry-menu-copy", Icon::Copy, "Copy Key").on_click(menu_action(cx, move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))))))
            .child(div().mx(px(13.)).my(px(5.)).h(px(1.)).bg(Theme::line_border()))
            .child(menu_row("entry-menu-delete").text_color(Theme::danger())
                .child(icon(Icon::Delete, 16., Theme::danger()))
                .child(menu_label(delete_label))
                .on_click(cx.listener(move |w, _, window, cx| w.open_overlay(Overlay::Confirm(Confirm::DeleteEntries(delete.clone())), window, cx))));
        anchored_menu(Corner::TopLeft, position, menu)
    }

    /// While resizing a column, a full-window layer follows the mouse, as the sidebar's does.
    pub(crate) fn render_column_drag(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let drag = self.citations.table.column_drag?;
        let end = |w: &mut Workspace, cx: &mut Context<Workspace>| {
            w.citations.table.column_drag = None;
            let widths = w.citations.table.widths.clone();
            w.set_pref(|p| p.library_columns = widths, cx);
            cx.notify();
        };
        Some(deferred(div().id("library-column-drag").absolute().inset_0().occlude().cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_move(cx.listener(move |w, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button != Some(MouseButton::Left) { return end(w, cx); }
                let width = drag.start_width - f32::from(ev.position.x - drag.start_x);
                w.citations.table.widths.insert(drag.name.to_string(), width.clamp(MIN_COLUMN_W, MAX_COLUMN_W));
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(move |w, _, _, cx| end(w, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(move |w, _, _, cx| end(w, cx)))).with_priority(crate::ui::layer::DRAG).into_any_element())
    }

    pub(crate) fn render_library_menu(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let item = |id: &'static str, glyph: Icon, label: &'static str| {
            menu_row(id).child(menu_glyph(glyph, 16.)).child(menu_label(label))
        };
        let menu = menu_panel("library-menu", 220., cx)
            .child(item("library-import", Icon::Import, "Import BibLaTeX…").on_click(menu_action(cx, |w, window, cx| w.import_bib(window, cx))))
            .child(item("library-export", Icon::Export, "Export BibLaTeX…").on_click(menu_action(cx, |w, _, cx| w.export_bib(cx))))
            .child(item("library-history", Icon::Restore, "Version History…").on_click(menu_action(cx, |w, window, cx| w.open_library_history(window, cx))))
            .child(item("library-find-papers", Icon::Search, "Find Missing Papers").on_click(menu_action(cx, |w, _, cx| {
                let keys = w.citations.library.iter().flat_map(Library::rows).map(|row| row.key).collect();
                w.fetch_papers(keys, cx);
            })));
        // Under the "…" button, which sits left of the Add button.
        let right = window.viewport_size().width - px(crate::ui::header_right_inset() + ADD_W);
        anchored_menu(Corner::TopRight, point(right, px(crate::app::HEADER_H - 4.)), menu)
    }
}
