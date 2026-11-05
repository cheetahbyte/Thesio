//! Home: where the vault stands. Recent documents, favorites, a few counts, and
//! the sources no document cites yet.

use crate::app::{View, Workspace};
use crate::library::{NOTES_DIR, paper_file};
use thesio_core::references::cited_keys;
use crate::settings::display_title;
use crate::store::Entry;
use crate::theme::Theme;
use crate::trash::{empty_state, page_header, primary_button, secondary_button};
use crate::ui::{Icon, TipExt, format_visit};
use gpui::{prelude::FluentBuilder, *};
use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hash, Hasher};

const COLUMN_W: f32 = 640.;
const RECENT: usize = 5;
const FAVORITES: usize = 6;
const UNCITED: usize = 6;

/// Counts over the writing in the vault: notes on sources stay out, since a
/// source cited only in its own notes is not cited in anything.
#[derive(Default)]
struct Overview {
    documents: usize,
    words: usize,
    sources: usize,
    papers: usize,
    /// (key, title), by title.
    uncited: Vec<(String, String)>,
}

/// Parsing every document is too slow for a frame, so the overview waits for a change.
#[derive(Default)]
pub struct Home { overview: Option<(u64, Overview)> }

fn visited(e: &Entry) -> u64 {
    if e.meta.opened > 0 { e.meta.opened } else { e.meta.modified }
}

/// The words of a document's prose, roughly: Typst set-up and comments stay out.
fn words(body: &str) -> usize {
    let lines = body.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"));
    lines.map(|l| l.trim_start_matches('=').split_whitespace().count()).sum()
}

fn section(title: &'static str, count: Option<usize>) -> Div {
    div().px(px(8.)).flex().items_center().gap(px(6.)).text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD)
        .child(title)
        .children(count.map(|count| div().font_weight(FontWeight::NORMAL).text_color(Theme::text_caption()).child(count.to_string())))
}

fn row(id: impl Into<ElementId>, title: String, detail: String) -> Stateful<Div> {
    div().id(id).h(px(32.)).px(px(8.)).flex().items_center().gap(px(10.)).rounded(px(crate::ui::ROW_RADIUS)).cursor_pointer()
        .active(|s| s.bg(Theme::fill_pressed()))
        .text_size(px(13.))
        .child(div().flex_1().min_w_0().truncate().child(title))
        .child(div().flex_none().text_color(Theme::text_caption()).child(detail))
}

fn tile(value: usize, label: &'static str) -> Div {
    div().flex_1().min_w_0().px(px(12.)).py(px(10.)).flex().flex_col().rounded(px(8.)).border_1().border_color(Theme::line_border())
        .child(div().text_size(px(22.)).line_height(px(28.)).font_weight(FontWeight::SEMIBOLD).child(value.to_string()))
        .child(div().text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption()).child(label))
}

impl Workspace {
    fn overview_stamp(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        for e in &self.entries { (&e.id, e.meta.modified, e.body.len()).hash(&mut hash); }
        for row in self.citations.library.iter().flat_map(|library| library.rows()) { (row.key, row.title).hash(&mut hash); }
        hash.finish()
    }

    fn overview(&self) -> Overview {
        let writing: Vec<&Entry> = self.entries.iter().filter(|e| e.typst && !e.id.starts_with(&format!("{NOTES_DIR}/"))).collect();
        let cited: BTreeSet<String> = writing.iter().flat_map(|e| cited_keys(&e.body)).collect();
        let rows = self.citations.library.iter().flat_map(|library| library.rows()).collect::<Vec<_>>();
        let papers = rows.iter().filter(|row| self.has_paper(&row.key)).count();
        let sources = rows.len();
        let mut uncited: Vec<_> = rows.into_iter().filter(|row| !cited.contains(&row.key)).map(|row| {
            let title = if row.title.is_empty() { row.key.clone() } else { row.title };
            (row.key, title)
        }).collect();
        uncited.sort_by(|a, b| a.1.cmp(&b.1));
        Overview { documents: writing.len(), words: writing.iter().map(|e| words(&e.body)).sum(), sources, papers, uncited }
    }

    fn document_rows(&self, entries: Vec<&Entry>, key: &'static str, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        entries.into_iter().enumerate().map(|(i, e)| {
            let id = e.id.clone();
            row((key, i), display_title(&e.title).to_string(), format_visit(visited(e)))
                .on_click(cx.listener(move |w, _, window, cx| w.open(&id, window, cx)))
        }).collect()
    }

    pub(crate) fn render_home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let stamp = self.overview_stamp();
        if self.home.overview.as_ref().is_none_or(|(known, _)| *known != stamp) { self.home.overview = Some((stamp, self.overview())); }
        // Too narrow for the search box: a search icon beside +, in the same style.
        let search = self.header_search(window, cx);
        let narrow = search.is_none();
        let header = page_header(self, "Home", None, 26., cx).relative().children(search)
            .child(crate::ui::toolbar_group()
                .when(narrow, |d| d.child(crate::ui::header_icon("home-search", Icon::Search)
                    .tip("Search documents", Some(crate::shortcuts::hint("workspace::FocusSearch", "⌘ + J")))
                    .on_click(cx.listener(|w, _, window, cx| w.open_search(window, cx)))))
                .child(crate::ui::header_icon("home-new", Icon::Plus)
                    .tip("New document", Some(crate::shortcuts::hint("workspace::NewDoc", "⌘ + N")))
                    .on_click(cx.listener(|w, _, window, cx| w.new_doc(window, cx)))));
        let page = div().size_full().flex().flex_col().bg(Theme::bg_body()).text_color(Theme::text()).font_family(crate::theme::UI_FONT).child(header);
        let Some((_, overview)) = &self.home.overview else { return page.into_any_element() };
        if overview.sources == 0 && !self.entries.iter().any(|e| e.typst) {
            let start = div().mt(px(16.)).flex().gap(px(8.))
                .child(secondary_button("home-sources", "Add a Source").on_click(cx.listener(|w, _, _, cx| w.show_view(View::Sources, cx))))
                .child(primary_button("home-start", "New Document").on_click(cx.listener(|w, _, window, cx| w.new_doc(window, cx))));
            return page.child(empty_state(Icon::Home, "Nothing here yet", "Start writing, or add the papers you are reading.").child(start)).into_any_element();
        }
        let (tiles, uncited_count) = (div().flex().gap(px(8.)).child(tile(overview.documents, "Documents")).child(tile(overview.words, "Words")).child(tile(overview.sources, "Sources")).child(tile(overview.papers, "Papers")), overview.uncited.len());
        let shown: Vec<(String, String)> = overview.uncited.iter().take(UNCITED).cloned().collect();
        let mut recent: Vec<&Entry> = self.entries.iter().filter(|e| e.typst).collect();
        recent.sort_by_key(|e| std::cmp::Reverse(visited(e)));
        recent.truncate(RECENT);
        let mut favorites: Vec<&Entry> = self.entries.iter().filter(|e| e.meta.starred > 0).collect();
        favorites.sort_by_key(|e| std::cmp::Reverse(e.meta.starred));
        favorites.truncate(FAVORITES);
        let (recent, favorites) = (self.document_rows(recent, "home-recent", cx), self.document_rows(favorites, "home-favorite", cx));
        let uncited = shown.into_iter().enumerate().map(|(i, (key, title))| {
            let open = key.clone();
            row(("home-uncited", i), title, key).on_click(cx.listener(move |w, _, _, cx| {
                if w.has_paper(&open) { w.open_paper(&paper_file(&open), cx); } else { w.show_view(View::Library, cx); }
            }))
        }).collect::<Vec<_>>();
        let column = div().w_full().max_w(px(COLUMN_W)).flex().flex_col().gap(px(28.))
            .child(tiles)
            .when(!recent.is_empty(), |d| d.child(div().flex().flex_col().gap(px(2.)).child(section("Recent", None)).children(recent)))
            .when(!favorites.is_empty(), |d| d.child(div().flex().flex_col().gap(px(2.)).child(section("Favorites", None)).children(favorites)))
            .when(!uncited.is_empty(), |d| d.child(div().flex().flex_col().gap(px(2.)).child(section("Not Cited Yet", Some(uncited_count))).children(uncited)));
        page.child(div().id("home-content").flex_1().min_h_0().overflow_y_scroll().child(div().w_full().flex().justify_center().px(px(24.)).pt(px(16.)).pb(px(64.)).child(column))).into_any_element()
    }
}
