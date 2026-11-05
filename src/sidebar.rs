//! Left pane: document list with search. Rows, sections and footer, sunken:
//! no panel, border or shadow, straight on the window surface (only the note
//! floats). 30pt rows on a 31pt pitch, selection pill radius 9.

use crate::app::{View, Workspace};
use crate::confirm::Confirm;
use crate::store::display_title;
use crate::store::Entry;
use crate::theme::{self, Theme, craft};
use thesio_core::project::Heading;
use crate::overlay::{Overlay, menu_action, menu_panel};
use crate::ui::{Icon, TipExt, TitleBarExt, anchored_menu, icon, menu_row, ui_text};
use gpui::{prelude::FluentBuilder, *};

/// Row "…" menu geometry: 32pt rows, 11pt dividers, 6pt padding inside
/// a 1pt border.
pub(crate) const ROW_MENU_W: f32 = 180.;
const ROW_ITEM: f32 = crate::ui::MENU_ROW_H;
const CHEVRON_SLOT: f32 = 16.;
const OUTLINE_ROW_H: f32 = 26.;
const OUTLINE_INDENT: f32 = 12.;
const CHEVRON_R: f32 = 6.;

/// Sidebar geometry (measured from the window edge), shared by the
/// document and Settings sidebars: selection pills start 14pt in, every row is
/// 30pt with radius 9 on a 31pt pitch, glyphs (18pt) centre at 37pt and
/// labels start at 54pt; groups are 16pt apart, a section header is 34pt
/// (6 above its 28pt label).
pub(crate) const SB_X: f32 = 14.;
pub(crate) const ROW_H: f32 = 30.;
const DOC_ROW_H: f32 = ROW_H;
pub(crate) const ROW_R: f32 = 9.;
pub(crate) const ROW_GAP: f32 = 1.;
pub(crate) const ROW_ICON: f32 = 18.;
pub(crate) const GROUP_GAP: f32 = 16.;
pub(crate) const SECTION_H: f32 = 34.;
pub(crate) const SECTION_TOP: f32 = 6.;

/// Selection mode caps at 50 pages.
const BATCH_LIMIT: usize = 50;

/// A row of the nav or the footer: the same pill, glyph and label columns.
pub(crate) fn nav_row(id: impl Into<ElementId>, glyph: Icon, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(ROW_H))
        .pl(px(6.))
        .pr(px(4.))
        .flex()
        .items_center()
        .rounded(px(ROW_R))
        .cursor_pointer()
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(Theme::text())
        .when(on, |d| d.bg(Theme::sidebar_selected()))
        .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().mr(px(9.)).child(icon(glyph, ROW_ICON, Theme::sidebar_glyph())))
        .child(label)
}

/// The footer's divider and padding around `row`.
pub(crate) fn footer_slot(row: impl IntoElement) -> Div {
    div().pt(px(6.)).pb(px(10.)).px(px(SB_X)).child(row)
}

pub(crate) fn footer_line() -> Div { div().flex_none().mx(px(17.)).h(px(1.)).bg(Theme::sidebar_divider()) }

/// Right-click / "…" menu on a sidebar row.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub id: String,
    pub pos: Point<Pixels>,
    /// The selection menu for the checked rows.
    pub batch: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Check {
    Off,
    Partial,
    On,
}

/// 14px checkbox.
fn checkbox(state: Check, disabled: bool) -> Div {
    let white = Theme::bg_body();
    div()
        .size(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .when(state != Check::Off, |d| d.bg(Theme::primary()))
        .when(state == Check::Off, |d| {
            d.border_1()
                .when(disabled, |d| d.border_color(Theme::line_border()).bg(theme::c(craft::N100)))
                .when(!disabled, |d| d.border_color(theme::c(craft::N500)).bg(white))
        })
        .when(state == Check::On, |d| d.child(icon(Icon::Check, 12., white)))
        .when(state == Check::Partial, |d| d.child(div().w(px(8.)).h(px(2.)).rounded(px(1.)).bg(white)))
}

/// A row that goes somewhere in a document.
fn jump_row(id: impl Into<ElementId>, title: String, indent: f32, active: bool) -> Stateful<Div> {
    div().id(id).h(px(OUTLINE_ROW_H)).pl(px(8. + indent)).pr(px(CHEVRON_R)).flex().items_center().rounded(px(ROW_R)).cursor_pointer()
        .text_size(px(13.))
        .text_color(if active { Theme::text() } else { Theme::sidebar_section() })
        .when(active, |d| d.font_weight(FontWeight::MEDIUM))
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(title)))
}

/// A sidebar row being dragged.
#[derive(Clone)]
pub(crate) struct DraggedRow {
    id: String,
    title: SharedString,
}

impl Render for DraggedRow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().px(px(10.)).py(px(4.)).rounded(px(6.)).bg(Theme::bg_body()).border_1().border_color(Theme::line_border())
            .shadow(theme::shadow_s4_down()).text_size(px(13.)).text_color(Theme::text()).child(self.title.clone())
    }
}

impl Workspace {
    /// Sidebar width for the current window: the dragged width, kept between
    /// 268 and min(800, window − 540).
    pub(crate) fn sidebar_w(&self, window: &Window) -> f32 {
        let vw = f32::from(window.viewport_size().width);
        let max = theme::SIDEBAR_MAX.min(vw - theme::SIDEBAR_MIN_CONTENT).max(theme::SIDEBAR_MIN);
        self.sidebar_width.clamp(theme::SIDEBAR_MIN, max)
    }

    /// What the file list shows and "select all" takes: a hidden file must not
    /// be caught by a batch delete or move the user cannot see.
    fn listed_entries(&self) -> impl Iterator<Item = &Entry> {
        use crate::library::PAPERS_DIR;
        use thesio_core::paths::LIBRARY_FILE;
        let hide = crate::preferences::prefs().hide_library_files;
        let inside = format!("{PAPERS_DIR}/");
        // A `sources` folder holding anything else is the user's own.
        let papers_only = !self.entries.iter().any(|e| e.id.starts_with(&inside) && !e.id.ends_with(".pdf"));
        self.entries.iter().filter(move |e| !(hide && (e.id == LIBRARY_FILE || papers_only && (thesio_core::paths::is_within(&e.id, PAPERS_DIR)))))
    }

    pub(crate) fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let entries: Vec<Entry> = self.listed_entries().cloned().collect();

        let header = div()
            .h(px(crate::ui::HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .pl(px(theme::TRAFFIC_LIGHTS_W))
            .pr(px(12.))
            .title_bar()
            .when(cfg!(target_os = "windows"), |d| d.child(self.header_navigation(cx)))
            .when(!cfg!(target_os = "windows"), |d| d.child(self.sidebar_toggle("collapse-sidebar", Theme::sidebar_icon(), cx).ml(px(4.))))
            .child(div().flex_1());

        // The nav is just Home: 36px items, a blue tint when selected. Its
        // icon column (x 17) is shared with Search and the doc rows.
        let page = |id: &'static str, label: &'static str, view: View, glyph: Icon, cx: &mut Context<Self>| {
            let on = self.view == view || view == View::Sources && self.view == View::Paper;
            nav_row(id, glyph, label, on).on_click(cx.listener(move |w, _, window, cx| w.show_view(view, window, cx)))
        };
        let home = if self.view == View::Home { Icon::HomeFilled } else { Icon::Home };
        let nav = div().flex_none().pt(px(8.)).pb(px(4.)).px(px(SB_X))
            .child(page("nav-home", "Home", View::Home, home, cx))
            .child(page("nav-library", "Library", View::Library, Icon::Library, cx))
            .child(page("nav-sources", "Sources", View::Sources, Icon::Paper, cx));

        let mut list = div().flex().flex_col().px(px(SB_X)).pb(px(8.));
        // Pinned docs are shortcuts: they stay in My documents too.
        let pinned: Vec<&Entry> = entries.iter().filter(|e| e.meta.pinned).collect();
        if !pinned.is_empty() {
            list = list.child(self.section(0, "Pinned", cx));
            if !self.folded[0] {
                for e in pinned {
                    list = list.child(self.doc_row(e, "pinned", 0, cx));
                }
            }
            list = list.child(div().h(px(GROUP_GAP - SECTION_TOP)));
        }
        if self.view == View::Doc && !self.outline.is_empty() {
            list = list.child(self.section(2, "Outline", cx));
            if !self.folded[2] {
                let top = self.outline.iter().map(|heading| heading.level).min().unwrap_or(1);
                for (i, heading) in self.outline.iter().enumerate() {
                    list = list.child(self.outline_row(i, heading, top, cx));
                }
            }
            list = list.child(div().h(px(GROUP_GAP - SECTION_TOP)));
        }
        if self.view == View::Doc && !self.backlinks.is_empty() {
            list = list.child(self.section(3, "Linked From", cx));
            if !self.folded[3] {
                for (i, (path, title, offset)) in self.backlinks.iter().enumerate() {
                    let (path, offset) = (path.clone(), *offset);
                    list = list.child(jump_row(("backlink", i), title.clone(), 0., false).on_click(cx.listener(move |w, _, window, cx| w.go_to_source(&path, offset, window, cx))));
                }
            }
            list = list.child(div().h(px(GROUP_GAP - SECTION_TOP)));
        }
        list = list.child(self.section(1, "Files", cx));
        if !self.folded[1] {
            let tree = crate::tree::Tree::new(&entries);
            for (e, depth) in tree.rows(|id| self.expanded.contains(id)) {
                list = list.child(self.doc_row(e, "doc", depth, cx));
            }
        }

        // The footer holds one row: Settings (Storage and Trash live in Settings).
        // The Selection mode toolbar keeps the footer glyphs: bare on the sidebar.
        let footer_btn = |id: &'static str, name: Icon, tip: &'static str, enabled: bool| {
            div()
                .id(id)
                .flex_none()
                .w(px(36.))
                .h(px(32.))
                .when(enabled, |d| d.cursor_pointer())
                .tip(tip, None)
                .child(div().size_full().flex().items_center().justify_center().child(icon(name, 17., if enabled { Theme::text_code() } else { Theme::sidebar_disabled() })))
        };
        let some = self.batch.as_ref().is_some_and(|b| !b.is_empty());
        // Settings; in Settings the same row reads Back (`settings.rs`), so one spot toggles.
        let footer = div()
            .flex_none()
            .flex()
            .flex_col()
            .child(footer_line())
            .when(self.batch.is_none(), |d| {
                d.child(footer_slot(
                    nav_row("ft-settings", Icon::Gear, "Settings", false)
                        .on_click(cx.listener(|w, _, window, cx| w.show_view(View::Settings, window, cx))),
                ))
            })
            // Selection mode toolbar: Duplicate | Move to | Delete.
            .when(self.batch.is_some(), |d| {
                d.child(
                    div().h(px(46.)).mx(px(SB_X + 6. + 10. - 18.)).flex().items_center().gap(px(4.))
                        .child(footer_btn("bt-copy", Icon::Copy, "Duplicate", some).on_click(cx.listener(|w, _, _, cx| w.batch_copy(cx))))
                        .child(footer_btn("bt-move", Icon::Move, "Move to", some).on_click(cx.listener(|w, _, window, cx| w.batch_move(window, cx))))
                        .child(footer_btn("bt-delete", Icon::Delete, "Delete", some).on_click(cx.listener(|w, _, window, cx| w.ask_batch_delete(window, cx)))),
                )
            });

        div()
            .w(px(self.sidebar_w(window)))
            .bg(Theme::sidebar_veil())
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(nav)
            .child(div().id("doc-list").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.list_scroll).child(list)
                // A drop that no row takes lands in the vault root.
                .drag_over::<DraggedRow>(|s, _, _, _| s.bg(Theme::sidebar_hover()))
                .on_drop(cx.listener(|w, row: &DraggedRow, _, cx| w.move_docs(std::slice::from_ref(&row.id), None, cx)))
                .on_mouse_down(MouseButton::Right, cx.listener(|w, ev: &MouseDownEvent, window, cx| w.open_overlay(Overlay::FilesMenu(ev.position), window, cx))))
            .child(footer)
            .into_any_element()
    }

    /// Width handle: a 15px hit strip straddling the sidebar edge (6px inside,
    /// 9px over the note's margin). Borderless like the sidebar: only the
    /// cursor changes, and a faint line shows while dragging.
    pub(crate) fn render_resize_handle(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let w = self.sidebar_w(window);
        let dragging = self.sidebar_drag;
        div()
            .id("sidebar-resize")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(w - 6.))
            .w(px(15.))
            .cursor(CursorStyle::ResizeLeftRight)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(5.))
                    .w(px(2.))
                    .rounded(px(1.))
                    .when(dragging, |d| d.bg(Theme::primary())),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, _, window, cx| {
                    cx.stop_propagation();
                    ws.sidebar_width = ws.sidebar_w(window);
                    ws.sidebar_drag = true;
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// While dragging, a transparent full-window layer keeps the resize cursor
    /// and follows the mouse.
    pub(crate) fn render_resize_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.sidebar_drag {
            return None;
        }
        let end = |ws: &mut Workspace, cx: &mut Context<Workspace>| {
            ws.sidebar_drag = false;
            let width = ws.sidebar_width;
            let result = ws.store.borrow().update_settings(|s| s.sidebar_width = Some(width));
            if let Err(e) = result { ws.show_toast_kind(crate::ui::ToastKind::Error, &format!("Could not remember the sidebar width: {e}"), cx); }
            cx.notify();
        };
        Some(
            deferred(
                div()
                    .id("sidebar-resize-overlay")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_move(cx.listener(move |ws, ev: &MouseMoveEvent, window, cx| {
                        // The release can land before this layer exists.
                        if ev.pressed_button != Some(MouseButton::Left) {
                            return end(ws, cx);
                        }
                        ws.sidebar_width = f32::from(ev.position.x);
                        ws.sidebar_width = ws.sidebar_w(window);
                        cx.notify();
                    }))
                    .on_mouse_up(MouseButton::Left, cx.listener(move |ws, _, _, cx| end(ws, cx)))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(move |ws, _, _, cx| end(ws, cx))),
            )
            .with_priority(crate::ui::layer::DRAG)
            .into_any_element(),
        )
    }

    /// Section header, as in Finder: the title on the left and a chevron at the
    /// end of the line, lined up with the folders' chevrons. Clicking it folds.
    fn section(&self, idx: usize, title: &'static str, cx: &mut Context<Self>) -> Div {
        let folded = self.folded[idx];
        let row = div().h(px(SECTION_H)).pl(px(4.)).pt(px(SECTION_TOP)).flex().items_center();
        if let (1, Some(sel)) = (idx, self.batch.as_ref()) {
            let n = sel.len();
            let all = n > 0 && n >= self.listed_entries().count().min(BATCH_LIMIT);
            return row
                .child(
                    div()
                        .id("batch-all")
                        .ml(px(6.))
                        .size(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(checkbox(if all { Check::On } else if n > 0 { Check::Partial } else { Check::Off }, false))
                        .on_click(cx.listener(move |w, _, _, cx| {
                            let ids = if all { vec![] } else { w.listed_entries().take(BATCH_LIMIT).map(|e| e.id.clone()).collect() };
                            w.batch = Some(ids);
                            cx.notify();
                        })),
                )
                .child(div().flex_1().min_w_0().ml(px(8.)).text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(format!("Selected: {n}")))
                .child(
                    div()
                        .id("batch-exit")
                        .flex_none()
                        .ml(px(8.))
                        .h(px(22.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(7.))
                        .border_1()
                        .border_color(Theme::button_border())
                        .bg(crate::theme::raised(Theme::bg_body()))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .child("Exit")
                        .tip("Exit selection mode", None)
                        .on_click(cx.listener(|w, _, _, cx| {
                            w.batch = None;
                            cx.notify();
                        })),
                );
        }
        row.child(
            div()
                .id(("section-title", idx))
                .flex_1()
                .min_w_0()
                .pl(px(4.))
                .pr(px(CHEVRON_R))
                .h(px(22.))
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .child(div().text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::sidebar_section()).child(title))
                .child(div().size(px(CHEVRON_SLOT)).flex_none().flex().items_center().justify_center().child(icon(if folded { Icon::ChevronRight } else { Icon::ChevronDown }, 11., Theme::sidebar_icon())))
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.folded[idx] = !w.folded[idx];
                    cx.notify();
                })),
        )
    }

    /// Check or uncheck a My documents row (entering Selection mode if needed); false
    /// when the 50-page limit blocks it.
    pub(crate) fn toggle_check(&mut self, id: &str) -> bool {
        self.folded[1] = false;
        let sel = self.batch.get_or_insert_with(Vec::new);
        if let Some(i) = sel.iter().position(|x| x == id) {
            sel.remove(i);
        } else if sel.len() < BATCH_LIMIT {
            sel.push(id.to_string());
        } else {
            return false;
        }
        true
    }

    /// The checked docs, in list order.
    fn checked(&self) -> Vec<String> {
        let Some(sel) = &self.batch else { return vec![] };
        self.entries.iter().filter(|e| sel.contains(&e.id)).map(|e| e.id.clone()).collect()
    }

    /// Batch duplicate: a "… Copy" of each checked doc; leaves Selection mode.
    pub(crate) fn batch_copy(&mut self, cx: &mut Context<Self>) {
        let ids = self.checked();
        if ids.is_empty() {
            return;
        }
        for id in ids.iter().filter(|id| thesio_core::paths::is_document(id)) {
            self.copy_doc(id, cx);
        }
        self.refresh_entries(cx);
        self.batch = None;
        cx.notify();
    }

    pub(crate) fn batch_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.checked();
        if !ids.is_empty() {
            self.open_move(ids, window, cx);
        }
    }

    pub(crate) fn ask_batch_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.checked();
        if !ids.is_empty() { self.open_overlay(Overlay::Confirm(Confirm::Delete { ids, batch: true }), window, cx); }
    }

    fn outline_row(&self, i: usize, heading: &Heading, top: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let (path, offset) = (heading.path.clone(), heading.offset);
        jump_row(("outline", i), heading.title.clone(), OUTLINE_INDENT * (heading.level - top) as f32, self.outline_active == Some(i))
            .on_click(cx.listener(move |w, _, window, cx| w.go_to_source(&path, offset, window, cx)))
    }

    fn row_base(&self, e: &Entry, key: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = e.id.clone();
        let id2 = e.id.clone();
        let in_lib = key == "doc";
        div()
            .id(SharedString::from(format!("{key}-{}", e.id)))
            .group("doc-row")
            .mb(px(ROW_GAP))
            .rounded(px(ROW_R))
            .cursor_pointer()
            .on_click(cx.listener(move |w, ev: &ClickEvent, window, cx| {
                if w.rename.as_ref().is_some_and(|r| r.id == id) {
                    return;
                }
                // In selection mode a click checks the row; Shift-click enters it.
                if in_lib && (w.batch.is_some() || ev.modifiers().shift) {
                    w.toggle_check(&id);
                    cx.notify();
                    return;
                }
                w.open(&id, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |w, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let batch = in_lib && w.batch.is_some();
                    if batch && !w.batch.as_ref().is_some_and(|b| b.contains(&id2)) && !w.toggle_check(&id2) {
                        return;
                    }
                    w.open_overlay(Overlay::RowMenu(RowMenu { id: id2.clone(), pos: ev.position, batch }), window, cx);
                }),
            )
    }

    fn doc_row(&self, e: &Entry, key: &str, depth: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.view == View::Doc && self.current.as_deref() == Some(e.id.as_str());
        let renaming = self.rename.as_ref().filter(|r| r.id == e.id).map(|r| r.field.clone());
        let title = match renaming {
            Some(field) => div()
                .ml(px(6.))
                .mr(px(6.))
                .flex_1()
                .min_w_0()
                .h(px(24.))
                .px(px(6.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .bg(crate::theme::raised(Theme::bg_body()))
                .border_1()
                .border_color(Theme::primary())
                .text_size(px(13.))
                .child(field)
                .into_any_element(),
            // Without a glyph, file names start where a folder's glyph does.
            None => div()
                .pl(px(if e.folder { 9. } else { 2. }))
                .flex_1()
                .min_w_0()
                .text_size(px(13.))
                .line_height(px(DOC_ROW_H))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(Theme::text())
                .child(ui_text(display_title(&e.title)))
                .into_any_element(),
        };
        let renaming = self.rename.as_ref().is_some_and(|r| r.id == e.id);
        // Selection mode: My documents rows get a checkbox and lose "…"; checked rows are tinted.
        let batch = self.batch.as_ref().filter(|_| key == "doc");
        let checked = batch.is_some_and(|b| b.contains(&e.id));
        let blocked = batch.is_some_and(|b| !checked && b.len() >= BATCH_LIMIT);
        self.row_base(e, key, cx)
            .h(px(DOC_ROW_H))
            .flex()
            .items_center()
            .when(selected || checked, |d| d.bg(Theme::sidebar_selected()))
            .pl(px(6. + 16. * depth as f32))
            .relative()
            // One guide per ancestor, under its chevron; the extra point bridges ROW_GAP.
            .children((0..if batch.is_none() { depth } else { 0 }).map(|level| div().absolute().top(px(-ROW_GAP)).bottom_0().left(px(16. + 16. * level as f32)).w(px(1.)).bg(Theme::sidebar_guide())))
            .when(key == "doc" && !renaming && batch.is_none(), |d| {
                // Dropping on a file means the folder it sits in.
                let target = if e.folder { Some(e.id.clone()) } else { e.parent.clone() };
                let dragged = DraggedRow { id: e.id.clone(), title: display_title(&e.title).into() };
                d.on_drag(dragged, |row, _, _, cx| cx.new(|_| row.clone()))
                    .drag_over::<DraggedRow>(|s, _, _, _| s.bg(Theme::sidebar_selected()))
                    .on_drop(cx.listener(move |w, row: &DraggedRow, _, cx| w.move_docs(std::slice::from_ref(&row.id), target.clone(), cx)))
            })
            .when(batch.is_some(), |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("check-{}", e.id)))
                        .size(px(20.))
                        .mr(px(4.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(blocked, |d| d.tip("Selection limit reached", None))
                        .child(checkbox(if checked { Check::On } else { Check::Off }, blocked)),
                )
            })
            .when(e.folder, |d| d.child(div().size(px(20.)).flex_none().flex().items_center().justify_center().child(icon(Icon::Folder, ROW_ICON, Theme::sidebar_glyph()))))
            .child(title)
            .when(key == "doc" && e.folder, |d| {
                let toggle = e.id.clone();
                let open = self.expanded.contains(&e.id);
                d.child(div().id(SharedString::from(format!("arrow-{}", e.id))).size(px(CHEVRON_SLOT)).mr(px(CHEVRON_R)).flex_none().flex().items_center().justify_center()
                    .child(icon(Icon::ChevronRight, 11., Theme::sidebar_icon()).when(open, |s| s.with_transformation(Transformation::rotate(radians(std::f32::consts::FRAC_PI_2)))))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |w, _, _, cx| {
                        cx.stop_propagation();
                        w.toggle_folder(&toggle, cx);
                    })))
            })
    }

    /// The menu for the list itself: everything that adds to it lives here.
    pub(crate) fn render_files_menu(&self, position: Point<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let item = |id: &'static str, glyph: Icon, label: &'static str| {
            menu_row(id).child(crate::ui::menu_glyph(glyph, 16.)).child(crate::ui::menu_label(label))
        };
        let menu = menu_panel("files-menu", ROW_MENU_W, cx)
            .child(item("files-new", Icon::Plus, "New document").on_click(menu_action(cx, |w, window, cx| w.new_doc(window, cx))))
            .child(item("files-new-folder", Icon::FolderNew, "New folder").on_click(menu_action(cx, |w, window, cx| w.new_folder(None, window, cx))))
            .child(item("files-select", Icon::Select, "Select documents").on_click(menu_action(cx, |w, _, cx| {
                w.batch = Some(vec![]);
                w.folded[1] = false;
                cx.notify();
            })));
        anchored_menu(Corner::TopLeft, position, menu)
    }

    pub(crate) fn render_row_menu(&self, m: &RowMenu, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (pinned, starred, folder, typst) = self.entries.iter().find(|e| e.id == m.id).map(|e| (e.meta.pinned, e.meta.starred > 0, e.folder, e.typst))?;
        let item_c = |id: &'static str, ic: Icon, label: &'static str, color: Hsla, icon_color: Hsla| {
            menu_row(id)
                .mx(px(0.))
                .text_color(color)
                .child(icon(ic, 16., icon_color))
                .child(div().flex_1().min_w_0().child(crate::ui::menu_label(label)))
        };
        let item = |id, ic, label| item_c(id, ic, label, Theme::text(), Theme::text_code());
        let divider = || div().mx(px(8.)).my(px(5.)).h(px(1.)).bg(Theme::line_border());
        if m.batch {
            // Right-click in Selection mode: the batch menu.
            let n = self.batch.as_ref().map_or(0, |b| b.len());
            let menu = menu_panel("batch-menu", ROW_MENU_W, cx)
                .px(px(5.))
                .child(div().h(px(ROW_ITEM)).px(px(8.)).flex().items_center().text_size(px(12.)).text_color(Theme::text_placeholder()).child(format!("Selected: {n}")))
                .child(item("bm-copy", Icon::Copy, "Duplicate").on_click(menu_action(cx, |w, _, cx| w.batch_copy(cx))))
                .child(item("bm-move", Icon::Move, "Move to").on_click(menu_action(cx, |w, window, cx| w.batch_move(window, cx))))
                .child(item("bm-delete", Icon::Delete, "Delete").on_click(menu_action(cx, |w, window, cx| w.ask_batch_delete(window, cx))));
            return Some(anchored_menu(Corner::TopLeft, m.pos, menu));
        }
        let id = m.id.clone();
        let mut menu = menu_panel("row-menu", ROW_MENU_W, cx).px(px(5.));
        let reveal = self.store.borrow().path(&id).ok();
        let reveal_item = item("rm-reveal", Icon::Move, crate::system::REVEAL_LABEL).on_click(menu_action(cx, move |_, _, cx| {
            if let Some(path) = &reveal { crate::system::reveal(path, cx); }
        }));
        if folder {
            let (doc, sub, mv, ren, del) = (id.clone(), id.clone(), id.clone(), id.clone(), id);
            menu = menu
                .child(item("rm-new", Icon::Plus, "New document").on_click(menu_action(cx, move |w, window, cx| w.new_child(&doc, window, cx))))
                .child(item("rm-new-folder", Icon::FolderNew, "New folder").on_click(menu_action(cx, move |w, window, cx| w.new_folder(Some(&sub), window, cx))))
                .child(divider())
                .child(item("rm-rename", Icon::Edit, "Rename").on_click(menu_action(cx, move |w, window, cx| w.start_rename(&ren, window, cx))))
                .child(item("rm-move", Icon::Move, "Move to").on_click(menu_action(cx, move |w, window, cx| w.open_move(vec![mv.clone()], window, cx))))
                .child(reveal_item)
                .child(divider())
                .child(item_c("rm-del", Icon::Delete, "Delete", Theme::danger(), Theme::danger()).on_click(menu_action(cx, move |w, window, cx| w.ask_delete(&del, window, cx))));
        } else {
            let (dup, mv, pin, star, pdf, ren, del, new_window) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id);
            if typst {
                menu = menu
                    .child(item("rm-window", Icon::Window, "Open in new window").on_click(menu_action(cx, move |_, _, cx| {
                        let id = new_window.clone();
                        cx.defer(move |cx| crate::open_main_window(false, Some(id), cx));
                    })))
                    .child(item("rm-copy", Icon::Copy, "Duplicate").on_click(menu_action(cx, move |w, window, cx| w.duplicate_doc(&dup, window, cx))));
            }
            menu = menu
                .child(item("rm-move", Icon::Move, "Move to").on_click(menu_action(cx, move |w, window, cx| w.open_move(vec![mv.clone()], window, cx))))
                .child(item("rm-pin", if pinned { Icon::Unpin } else { Icon::Pin }, if pinned { "Unpin" } else { "Pin" }).on_click(menu_action(cx, move |w, _, cx| w.toggle_pin(&pin, cx))))
                .child(item("rm-star", if starred { Icon::Unstar } else { Icon::Star }, if starred { "Remove from favorites" } else { "Add to favorites" }).on_click(menu_action(cx, move |w, _, cx| w.toggle_star(&star, cx))))
                .child(divider());
            if typst {
                menu = menu.child(item("rm-pdf", Icon::Export, "Export as PDF").on_click(menu_action(cx, move |w, _, cx| w.export_pdf(&pdf, cx))));
            }
            menu = menu
                .child(item("rm-rename", Icon::Edit, "Rename").on_click(menu_action(cx, move |w, window, cx| w.start_rename(&ren, window, cx))))
                .child(reveal_item)
                .child(divider())
                .child(item_c("rm-del", Icon::Delete, "Delete", Theme::danger(), Theme::danger()).on_click(menu_action(cx, move |w, window, cx| w.ask_delete(&del, window, cx))));
        }
        Some(anchored_menu(Corner::TopLeft, m.pos, menu))
    }
}
