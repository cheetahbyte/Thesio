//! Home: the local library's home — the New / Upload
//! cards (a new document, or Import as document at once: no menus), the Recently visited and Favorites lists (list or grid view), Display settings for the
//! list columns and the sort dropdown. Dressed as All documents: a
//! hairline list with quiet hover fills, white cards and pills, native menus.

use crate::app::{RowMenu, Workspace};
#[path = "home_scrollbar.rs"]
mod scrollbar;
use crate::settings::display_title;
use crate::search::doc_icon;
use crate::store::Entry;
use crate::theme::Theme;
use crate::trash::{empty_state, list_header_text, list_line, menu_label, page_header, row_hover};
use crate::ui::{Icon, ROW_RADIUS, TipExt, icon, menu_glyph, menu_row, popover, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::RefCell;
use std::collections::{HashMap};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HomeTab {
    #[default]
    Recent,
    Starred,
}

/// The optional list columns (Title is always shown).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Col {
    Modified,
    Created,
    Visited,
    Starred,
}

impl Col {
    fn label(self) -> &'static str {
        match self {
            Col::Modified => "Date modified",
            Col::Created => "Date created",
            Col::Visited => "Recently visited",
            Col::Starred => "Date favorited",
        }
    }
}

/// Recently visited's columns (no Owner); Display settings toggles them.
const RECENT_COLS: [Col; 3] = [Col::Modified, Col::Created, Col::Visited];
const STARRED_COLS: [Col; 4] = [Col::Modified, Col::Created, Col::Visited, Col::Starred];

#[derive(Clone, Copy, PartialEq, Eq)]
enum HomeSort { Visited, Modified, Created, Title }
impl HomeSort {
    fn key(self) -> &'static str { match self { Self::Visited => "visited", Self::Modified => "modified", Self::Created => "created", Self::Title => "title" } }
    fn of(key: &str) -> Self { match key { "modified" => Self::Modified, "created" => Self::Created, "title" => Self::Title, _ => Self::Visited } }
}
impl HomeSort {
    fn label(self) -> &'static str {
        match self {
            Self::Visited => "Last visited",
            Self::Modified => "Last modified",
            Self::Created => "Created (newest first)",
            Self::Title => "Title (A–Z)",
        }
    }
}

pub struct Home {
    pub tab: HomeTab,
    pub grid: bool,
    grid_scroll: UniformListScrollHandle,
    list_scroll: ScrollHandle,
    grid_scrollbar: Rc<scrollbar::Scrollbar>,
    list_scrollbar: Rc<scrollbar::Scrollbar>,
    sort: HomeSort,
    pub sort_menu: bool,
    /// Last-frame bounds of the controls the popovers hang from.
    anchors: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
}

impl Default for Home {
    fn default() -> Self {
        Home {
            tab: HomeTab::Recent,
            grid: true,
            grid_scroll: Default::default(),
            list_scroll: Default::default(),
            grid_scrollbar: Default::default(),
            list_scrollbar: Default::default(),
            sort: HomeSort::Visited,
            sort_menu: false,
            anchors: Default::default(),
        }
    }
}

impl Home {
    fn anchor(&self, key: &str) -> Option<Bounds<Pixels>> {
        self.anchors.borrow().get(key).copied()
    }

    /// Records the element's bounds under `key` each frame.
    fn track(&self, key: &'static str) -> impl IntoElement {
        let rec = self.anchors.clone();
        canvas(move |b, _, _| {
            rec.borrow_mut().insert(key, b);
        }, |_, _, _, _| {})
        .absolute()
        .inset_0()
    }

    fn cols(&self) -> Vec<Col> {
        match self.tab {
            HomeTab::Recent => RECENT_COLS.to_vec(),
            HomeTab::Starred => STARRED_COLS.to_vec(),
        }
    }

    fn sort_col(&self) -> Col {
        match self.sort {
            HomeSort::Visited => Col::Visited,
            HomeSort::Modified => Col::Modified,
            HomeSort::Created => Col::Created,
            HomeSort::Title => Col::Visited,
        }
    }
}

fn visited(e: &Entry) -> u64 {
    if e.meta.opened > 0 { e.meta.opened } else { e.meta.modified }
}

fn cell_text(e: &Entry, c: Col) -> String {
    match c {
        Col::Modified => crate::ui::format_visit(e.meta.modified),
        Col::Created => crate::ui::format_visit(e.meta.created),
        Col::Visited => crate::ui::format_visit(visited(e)),
        Col::Starred => crate::ui::format_visit(e.meta.starred),
    }
}

/// The title column grows three times as fast as the others.
fn weight3(mut d: Div) -> Div {
    d.style().flex_grow = Some(3.);
    d
}

fn entry_icon(_: &Entry) -> AnyElement {
    doc_icon().mr(px(12.)).into_any_element()
}

/// A card's preview: the prose of a document, without its Typst set-up.
fn card_text(body: &str) -> String {
    let lines = body.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"));
    lines.map(|l| l.trim_start_matches('=').trim()).take(12).collect::<Vec<_>>().join("\n")
}

const CARD_RADIUS: f32 = 10.;

impl Workspace {
    fn home_entries(&self) -> Vec<Entry> {
        let mut v: Vec<Entry> = match self.home.tab {
            HomeTab::Recent => self.entries.iter().filter(|e| e.typst).cloned().collect(),
            HomeTab::Starred => self.entries.iter().filter(|e| e.typst && e.meta.starred > 0).cloned().collect(),
        };
        v.sort_by(|a, b| {
            let order = match self.home.sort {
                HomeSort::Visited => visited(b).cmp(&visited(a)),
                HomeSort::Modified => b.meta.modified.cmp(&a.meta.modified),
                HomeSort::Created => b.meta.created.cmp(&a.meta.created),
                HomeSort::Title => a.title.cmp(&b.title),
            };
            order.then_with(|| a.id.cmp(&b.id))
        });
        v
    }


    fn open_row_menu(&mut self, id: &str, key: &str, cx: &mut Context<Self>) {
        let b = self.more_bounds.borrow().get(key).copied();
        let Some(b) = b else { return };
        self.row_menu = Some(RowMenu { id: id.to_string(), pos: point(b.right(), b.bottom() + px(2.)), corner: Corner::TopRight, batch: false });
        cx.notify();
    }

    fn home_more(&self, e: &Entry, key: String, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = e.id.clone();
        let (rec, k2) = (self.more_bounds.clone(), key.clone());
        let open = self.row_menu.as_ref().is_some_and(|m| m.id == e.id && m.pos.x > px(0.) && m.corner == Corner::TopRight);
        div()
            .id(SharedString::from(key.clone()))
            .relative()
            .size(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(Theme::fill_hover_strong()))
            .when(open, |d| d.bg(Theme::fill_hover_strong()))
            .child(icon(Icon::Ellipsis, 16., Theme::text_caption()))
            .child(canvas(move |b, _, _| {
                rec.borrow_mut().insert(key.clone(), b);
            }, |_, _, _, _| {}).absolute().inset_0())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |w, _, _, cx| {
                cx.stop_propagation();
                w.open_row_menu(&id, &k2, cx);
            }))
    }

    /// Home's view and sort order are remembered (`settings.json`).
    pub(crate) fn load_home(&mut self) {
        let s = self.store.settings();
        self.home.grid = !s.home_list;
        self.home.sort = s.home_sort.as_deref().map(HomeSort::of).unwrap_or(HomeSort::Visited);
    }

    fn save_home(&mut self) {
        let mut s = self.store.settings();
        (s.home_list, s.home_sort) = (!self.home.grid, Some(self.home.sort.key().to_string()));
        if let Err(e) = self.store.save_settings(&s) { eprintln!("Could not save home view: {e}"); }
    }

    pub(crate) fn render_home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
        // Tabs as segmented items: the current one on a quiet fill, no accent.
        let tab = |id: &'static str, label: &'static str, t: HomeTab| {
            let on = self.home.tab == t;
            div()
                .id(id)
                .h_full()
                .px(px(12.))
                .flex()
                .items_center()
                .rounded(px(crate::ui::BUTTON_RADIUS))
                .cursor_pointer()
                .text_size(px(13.))
                .line_height(px(20.))
                .when(on, |d| d.bg(Theme::fill_hover_strong()).text_color(Theme::text()))
                .when(!on, |d| d.text_color(Theme::text_caption()).hover(|s| s.text_color(Theme::text())))
                .child(crate::settings::label(label))
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.home.tab = t;
                    w.home.sort_menu = false;
                    cx.notify();
                }))
        };
        let toggle = |id: &'static str, ic: Icon, on: bool, tip: &'static str, grid: bool| {
            div()
                .id(id)
                .w(px(30.))
                .h(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(crate::ui::BUTTON_RADIUS))
                .cursor_pointer()
                .when(on, |d| d.bg(Theme::fill_hover_strong()))
                .when(!on, |d| d.tip(tip, None))
                .child(icon(ic, 14., if on { Theme::text_code() } else { Theme::text_caption() }))
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.home.grid = grid;
                    w.save_home();
                    w.home.sort_menu = false;
                    cx.notify();
                }))
        };
        let toolbar = div()
            .mt(px(12.))
            .h(px(28.))
            .flex_none()
            .pl(px(24.))
            .pr(px(24.))
            .flex()
            .items_center()
            .child(div().flex_none().h(px(crate::ui::BUTTON_H)).flex().gap(px(2.))
                .child(tab("home-tab-recent", "My documents", HomeTab::Recent))
                .child(tab("home-tab-star", "Favorites", HomeTab::Starred)))
            .child(div().flex_1())
            .child(
                div().id("home-sort").relative().h(px(crate::ui::BUTTON_H)).px(px(10.)).flex().items_center().gap(px(6.))
                    .rounded(px(crate::ui::BUTTON_RADIUS)).text_size(px(13.)).text_color(Theme::text_caption()).cursor_pointer()
                    .hover(|s| s.bg(Theme::fill_hover())).when(self.home.sort_menu, |d| d.bg(Theme::fill_hover_strong()))
                    .child(icon(Icon::Sort, 14., Theme::text_caption())).child(crate::settings::label(self.home.sort.label()))
                    .child(self.home.track("home-sort"))
                    .on_click(cx.listener(|w, _, _, cx| { w.home.sort_menu = !w.home.sort_menu; cx.notify(); })),
            )
            .child(
                // The view switcher: one white pill, the current view on a quiet fill.
                div()
                    .ml(px(12.))
                    .flex()
                    .flex_none()
                    .gap(px(2.))
                    .rounded(px(crate::ui::BUTTON_RADIUS))
                    .child(toggle("home-grid", Icon::Grid, self.home.grid, "Switch to grid view", true))
                    .child(toggle("home-list", Icon::List, !self.home.grid, "Switch to list view", false)),
            );

        let entries = self.home_entries();
        let body = if entries.is_empty() && (self.home.grid || self.home.tab == HomeTab::Recent) {
            empty_state(Icon::Document, "Nothing here yet", "New documents appear here.").into_any_element()
        } else if self.home.grid {
            self.render_home_grid(&entries, window, cx)
        } else {
            self.render_home_list(&entries, cx)
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(toolbar)
            .child(body)
            .into_any_element()
    }

    fn render_home_list(&self, entries: &[Entry], cx: &mut Context<Self>) -> AnyElement {
        let cols = self.home.cols();
        let sort = self.home.sort_col();
        // The title is sized at about three times a column.
        let title_cell = || div().flex_grow().flex_shrink().flex_basis(relative(0.)).min_w(px(200.));
        let col_cell = || div().flex_grow().flex_shrink().flex_basis(relative(0.)).max_w(px(240.)).min_w(px(120.));
        // The list: 13pt tertiary headers over a hairline; the
        // sorted column in the text colour, semibold, with its arrow.
        let mut header = div()
            .h(px(33.))
            .mt(px(10.))
            .flex_none()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(list_line())
            .child(
                weight3(title_cell()).pl(px(20.)).child(
                    div().id("home-col-title").px(px(4.)).text_size(px(13.)).line_height(px(24.)).text_color(list_header_text()).child("Title"),
                ),
            );
        for c in &cols {
            let sorted = self.home.sort != HomeSort::Title && *c == sort;
            header = header.child(
                col_cell().pl(px(20.)).child(
                    div()
                        .id(SharedString::from(format!("home-col-{}", c.label())))
                        .relative()
                        .px(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .rounded(px(ROW_RADIUS))
                        .text_size(px(13.))
                        .line_height(px(24.))
                        .text_color(if sorted { Theme::text() } else { list_header_text() })
                        .when(sorted, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .hover(|s| s.bg(Theme::fill_hover()))
                        .when(sorted && self.home.sort_menu, |d| d.bg(Theme::fill_hover_strong()))
                        .child(crate::settings::label(c.label()))
                        .when(sorted, |d| {
                            d.child(icon(Icon::Sort, 12., Theme::text()))
                                .cursor_pointer()
                                .on_click(cx.listener(|w, _, _, cx| {
                                    w.home.sort_menu = !w.home.sort_menu;
                                    cx.notify();
                                }))
                        }),
                ),
            );
        }
        header = header.child(div().w(px(60.)).flex_none());
        let mut rows = div().id("home-rows").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.home.list_scroll).px(px(24.)).pb(px(24.));
        if entries.is_empty() {
            rows = rows.child(div().pt(px(72.)).child(empty_state(Icon::Document, "Nothing here yet", "New documents appear here.")));
        }
        for (i, e) in entries.iter().enumerate() {
            let (id, id2) = (e.id.clone(), e.id.clone());
            let menu_on = self.row_menu.as_ref().is_some_and(|m| m.id == e.id);
            let mut row = div()
                .id(SharedString::from(format!("home-row-{}", e.id)))
                .h(px(56.))
                .flex()
                .items_center()
                .when(i > 0, |d| d.border_t_1().border_color(list_line()))
                .cursor_pointer()
                .hover(row_hover)
                .when(menu_on, |d| d.bg(Theme::fill_hover()).border_color(transparent_black()).rounded(px(ROW_RADIUS)))
                .on_click(cx.listener(move |w, _, window, cx| w.open(&id, window, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                        w.row_menu = Some(RowMenu { id: id2.clone(), pos: ev.position, corner: Corner::TopLeft, batch: false });
                        cx.notify();
                    }),
                )
                .child(
                    weight3(title_cell())
                        .pl(px(24.))
                        .pr(px(16.))
                        .flex()
                        .items_center()
                        .child(entry_icon(e))
                        .child(div().min_w_0().text_size(px(15.)).line_height(px(22.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(display_title(&e.title))))
                        .when(e.meta.pinned, |d| d.child(icon(Icon::PinFilled, 14., Theme::pinned()).ml(px(6.)))),
                );
            for c in &cols {
                row = row.child(col_cell().pl(px(24.)).text_size(px(13.)).text_color(Theme::text_caption()).whitespace_nowrap().overflow_hidden().child(cell_text(e, *c)));
            }
            row = row.child(div().w(px(60.)).flex_none().flex().justify_end().pr(px(12.)).child(self.home_more(e, format!("home-more-{}", e.id), cx)));
            rows = rows.child(row);
        }
        div().flex_1().min_h_0().flex().flex_col().child(header.mx(px(24.)))
            .child(div().relative().flex_1().min_h_0().flex().flex_col().child(rows).child(self.home.list_scrollbar.render(&self.home.list_scroll))).into_any_element()
    }

    fn render_home_grid(&self, entries: &[Entry], window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let main_w = f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let avail = (main_w - 48.).max(1.);
        let wide_cols = (((avail + 16.) / (200. + 16.)).floor() as usize).max(1);
        let n = if wide_cols >= 4 { wide_cols } else { (((avail + 16.) / (160. + 16.)).floor() as usize).clamp(1, 3) };
        let card_h = ((avail - 16. * (n - 1) as f32) / n as f32) * (264. / 236.);
        let entries = entries.to_vec();
        let rows = entries.len().div_ceil(n);
        let grid = uniform_list("home-grid-area", rows, cx.processor(move |workspace, range: std::ops::Range<usize>, _, cx| {
            range.map(|row| {
                let start = row * n;
                let end = (start + n).min(entries.len());
                div().w_full().h(px(card_h + 16.)).px(px(24.)).pb(px(16.))
                    .grid().grid_cols(n as u16).gap(px(16.))
                    .children(entries[start..end].iter().map(|e| workspace.render_home_card(e, card_h, cx)))
            }).collect()
        }))
        // The last row's own 16 plus 16 more: 32 below the cards, like the
        // list view's 24 and clear of the window's rounded corner.
        .flex_1().min_h_0().pb(px(16.)).track_scroll(self.home.grid_scroll.clone());
        let scroll = self.home.grid_scroll.0.borrow().base_handle.clone();
        div().relative().flex_1().min_h_0().mt(px(10.)).flex().flex_col()
            .child(grid).child(self.home.grid_scrollbar.render(&scroll)).into_any_element()
    }

    fn render_home_card(&self, e: &Entry, card_h: f32, cx: &mut Context<Self>) -> AnyElement {
            let id = e.id.clone();
            let id2 = e.id.clone();
            let body = crate::library::without_block(&e.body);
            let preview: String = card_text(&body);
            let page = div().size_full().p(px(18.)).overflow_hidden().flex().flex_col()
                .child(div().flex_none().mb(px(8.)).min_w_0().text_size(px(13.)).line_height(px(18.)).font_weight(FontWeight::SEMIBOLD).child(display_title(&e.title)))
                .child(div().flex_1().min_h_0().overflow_hidden().text_size(px(12.)).line_height(px(17.)).text_color(Theme::text_caption()).child(preview));
            let frame = 1.;
            let menu_on = self.row_menu.as_ref().is_some_and(|m| m.id == e.id);
                div()
                    .id(SharedString::from(format!("home-card-{}", e.id)))
                    .w_full().min_w_0().h(px(card_h)).relative().group("home-card")
                    .flex_none()
                    .bg(Theme::fill_pressed()).p(px(frame))
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|s| s.bg(Theme::fill_active()))
                    .when(menu_on, |d| d.bg(Theme::fill_active()))
                    .on_click(cx.listener(move |w, _, window, cx| w.open(&id, window, cx)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                            w.row_menu = Some(RowMenu { id: id2.clone(), pos: ev.position, corner: Corner::TopLeft, batch: false });
                            cx.notify();
                        }),
                    )
                    .child(crate::ui::continuous_page(CARD_RADIUS - frame).size_full().overflow_hidden().child(page.relative()))
                    .child(crate::ui::continuous_corner_mask(CARD_RADIUS, Theme::bg_body()))
                    .child(
                        div().absolute().top(px(8.)).right(px(8.))
                            .when(!menu_on, |d| d.opacity(0.).group_hover("home-card", |s| s.opacity(1.)))
                            .child(self.home_more(e, format!("home-gmore-{}", e.id), cx).rounded_full().bg(Theme::bg_overlay())),
                    )
            .into_any_element()
    }

    /// Display settings and the sort dropdown: the menu panel; the sort item is a
    /// native menu row.
    pub(crate) fn render_home_menus(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = vec![];
        if self.view != crate::app::View::Home {
            return out;
        }
        let place = |pos: Point<Pixels>, corner: Corner, menu: Stateful<Div>| {
            deferred(anchored().anchor(corner).position(pos).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element()
        };
        if self.home.sort_menu
            && let Some(b) = self.home.anchor("home-sort") {
                let mut menu = popover().id("sort-menu").occlude().w(px(200.)).py(px(crate::ui::MENU_PAD_Y))
                    .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.home.sort_menu = false; cx.notify(); }));
                for (index, order) in [HomeSort::Visited, HomeSort::Modified, HomeSort::Created, HomeSort::Title].into_iter().enumerate() {
                    let g = SharedString::from(format!("home-order-{index}"));
                    menu = menu.child(menu_row(g)
                        .child(menu_label(order.label()).flex_1())
                        .when(self.home.sort == order, |d| d.child(menu_glyph(Icon::Check, 14.)))
                        .on_click(cx.listener(move |w, _, _, cx| {
                            w.home.sort = order;
                            w.save_home();
                            w.home.sort_menu = false;
                            w.home.grid_scroll.scroll_to_item(0, ScrollStrategy::Top);
                            w.home.list_scroll.set_offset(point(px(0.), px(0.)));
                            cx.notify();
                        })));
                }
                out.push(place(point(b.right(), b.bottom() + px(4.)), Corner::TopRight, menu));
            }
        out
    }
}
