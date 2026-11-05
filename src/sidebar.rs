//! Left pane: document list with search. Rows, sections and footer, sunken:
//! no panel, border or shadow, straight on the window surface (only the note
//! floats). 30pt rows on a 31pt pitch, selection pill radius 9.

use crate::app::{Confirm, RowMenu, View, Workspace};
use crate::settings::display_title;
use crate::store::Entry;
use crate::theme::{self, Theme, craft};
use thesio_core::project::Heading;
use crate::ui::{Icon, TipExt, TitleBarExt, icon, popover, ui_text};
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

/// The footer row: like Home (same pill, glyph and label columns) under
/// the footer line, so the bottom corner mirrors the top.
pub(crate) fn footer_row(id: &'static str, glyph: Icon, label: &'static str) -> Stateful<Div> {
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
        .text_color(Theme::sidebar_text())
        .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().mr(px(9.)).child(icon(glyph, ROW_ICON, Theme::sidebar_glyph())))
        .child(label)
}

/// The footer's divider and padding around `row`.
pub(crate) fn footer_slot(row: impl IntoElement) -> Div {
    div().pt(px(6.)).pb(px(10.)).px(px(SB_X)).child(row)
}

pub(crate) fn footer_line() -> Div { div().flex_none().mx(px(17.)).h(px(1.)).bg(Theme::sidebar_divider()) }

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
        use thesio_core::bibliography::LIBRARY_FILE;
        let hide = crate::settings::prefs().hide_library_files;
        let inside = format!("{PAPERS_DIR}/");
        // A `sources` folder holding anything else is the user's own.
        let papers_only = !self.entries.iter().any(|e| e.id.starts_with(&inside) && !e.id.ends_with(".pdf"));
        self.entries.iter().filter(move |e| !(hide && (e.id == LIBRARY_FILE || papers_only && (e.id == PAPERS_DIR || e.id.starts_with(&inside)))))
    }

    pub(crate) fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let entries: Vec<Entry> = self.listed_entries().cloned().collect();

        let header = div()
            .h(px(crate::app::HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .pl(px(theme::TRAFFIC_LIGHTS_W))
            .pr(px(12.))
            .title_bar()
            .when(cfg!(target_os = "windows"), |d| d.child(self.header_navigation(cx)))
            .when(!cfg!(target_os = "windows"), |d| d.child(
                div()
                    .id("collapse-sidebar")
                    .no_drag()
                    .flex_none()
                    .ml(px(4.))
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .tip("Hide sidebar", Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                    .child(icon(Icon::SidebarToggle, 17., Theme::sidebar_icon()))
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.sidebar_open = false;
                        cx.notify();
                    })),
            ))
            .child(div().flex_1());

        // The nav is just Home: 36px items, a blue tint when selected. Its
        // icon column (x 17) is shared with Search and the doc rows.
        let nav_row = |id: &'static str, label: &'static str, view: View, glyph: Icon, on_glyph: Icon, cx: &mut Context<Self>| {
            let on = self.view == view || view == View::Sources && self.view == View::Paper;
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
                .when(on, |d| d.bg(Theme::sidebar_selected()).text_color(Theme::text()))
                .when(!on, |d| d.text_color(Theme::sidebar_text()))
                .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().mr(px(9.)).child(icon(
                    if on { on_glyph } else { glyph },
                    ROW_ICON,
                    Theme::sidebar_glyph(),
                )))
                .child(label)
                .on_click(cx.listener(move |w, _, _, cx| w.show_view(view, cx)))
        };
        let nav = div().flex_none().pt(px(8.)).pb(px(4.)).px(px(SB_X))
            .child(nav_row("nav-home", "Home", View::Home, Icon::Home, Icon::HomeFilled, cx))
            .child(nav_row("nav-library", "Library", View::Library, Icon::Library, Icon::Library, cx))
            .child(nav_row("nav-sources", "Sources", View::Sources, Icon::Paper, Icon::Paper, cx));

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
                    list = list.child(self.jump_row(("backlink", i), title.clone(), 0., false, cx).on_click(cx.listener(move |w, _, window, cx| w.go_to_source(&path, offset, window, cx))));
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
        let footer_btn = |id: &'static str, name: Icon, tip: &'static str, enabled: bool, on: bool| {
            div()
                .id(id)
                .flex_none()
                .w(px(36.))
                .h(px(32.))
                .when(enabled, |d| d.cursor_pointer())
                .tip(tip, None)
                .child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .when(on, |d| d.bg(Theme::sidebar_selected()))
                        .child(match (on, enabled) {
                            (true, _) => icon(name, 17., Theme::text()),
                            (false, true) => icon(name, 17., Theme::text_code()),
                            (false, false) => icon(name, 17., Theme::sidebar_disabled()),
                        }),
                )
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
                    footer_row("ft-settings", Icon::Gear, "Settings")
                        .on_click(cx.listener(|w, _, _, cx| w.show_view(View::Settings, cx))),
                ))
            })
            // Selection mode toolbar: Duplicate | Move to | Delete, minus the cloud ones.
            .when(self.batch.is_some(), |d| {
                d.child(
                    div().h(px(46.)).mx(px(SB_X + 6. + 10. - 18.)).flex().items_center().gap(px(4.))
                        .child(footer_btn("bt-copy", Icon::Copy, "Duplicate", some, false).on_click(cx.listener(|w, _, _, cx| w.batch_copy(cx))))
                        .child(footer_btn("bt-move", Icon::Move, "Move to", some, false).on_click(cx.listener(|w, _, window, cx| w.batch_move(window, cx))))
                        .child(footer_btn("bt-delete", Icon::Delete, "Delete", some, false).on_click(cx.listener(|w, _, _, cx| w.ask_batch_delete(cx)))),
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
                .on_mouse_down(MouseButton::Right, cx.listener(|w, ev: &MouseDownEvent, _, cx| {
                    w.row_menu = None;
                    w.files_menu = Some(ev.position);
                    cx.notify();
                })))
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
            let mut s = ws.store.settings();
            s.sidebar_width = Some(ws.sidebar_width);
            let _ = ws.store.save_settings(&s);
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
            .with_priority(4)
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
                            w.row_menu = None;
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
                .child(div().text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::sidebar_section()).child(crate::settings::label(title)))
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
        for id in ids.iter().filter(|id| id.ends_with(".typ")) {
            self.copy_doc(id, cx);
        }
        self.entries = self.store.list();
        self.sort();
        self.batch = None;
        self.row_menu = None;
        cx.notify();
    }

    pub(crate) fn batch_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.checked();
        if !ids.is_empty() {
            self.open_move(ids, window, cx);
        }
    }

    pub(crate) fn ask_batch_delete(&mut self, cx: &mut Context<Self>) {
        let ids = self.checked();
        if !ids.is_empty() {
            self.confirm = Some(Confirm::Delete { ids, batch: true });
            self.row_menu = None;
            cx.notify();
        }
    }

    fn outline_row(&self, i: usize, heading: &Heading, top: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let (path, offset) = (heading.path.clone(), heading.offset);
        self.jump_row(("outline", i), heading.title.clone(), OUTLINE_INDENT * (heading.level - top) as f32, self.outline_active == Some(i), cx)
            .on_click(cx.listener(move |w, _, window, cx| w.go_to_source(&path, offset, window, cx)))
    }

    /// A row that goes somewhere in a document.
    fn jump_row(&self, id: impl Into<ElementId>, title: String, indent: f32, active: bool, _: &mut Context<Self>) -> Stateful<Div> {
        div().id(id).h(px(OUTLINE_ROW_H)).pl(px(8. + indent)).pr(px(CHEVRON_R)).flex().items_center().rounded(px(ROW_R)).cursor_pointer()
            .text_size(px(13.))
            .text_color(if active { Theme::text() } else { Theme::sidebar_section() })
            .when(active, |d| d.font_weight(FontWeight::MEDIUM))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(title)))
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
                w.row_menu = None;
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
                cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    w.files_menu = None;
                    let batch = in_lib && w.batch.is_some();
                    if batch && !w.batch.as_ref().is_some_and(|b| b.contains(&id2)) && !w.toggle_check(&id2) {
                        return;
                    }
                    w.row_menu = Some(RowMenu { id: id2.clone(), pos: ev.position, corner: Corner::TopLeft, batch });
                    cx.notify();
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
                .border_color(Theme::primary_500())
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
                .text_color(if selected { Theme::text() } else { Theme::sidebar_text() })
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
                let target = if e.folder { Some(e.id.clone()) } else { e.meta.parent.clone() };
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
    pub(crate) fn render_files_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = self.files_menu?;
        let item = |id: &'static str, glyph: Icon, label: &'static str| {
            crate::ui::menu_row(id).child(crate::ui::menu_glyph(glyph, 16.)).child(crate::ui::menu_label(label))
        };
        let menu = popover().id("files-menu").occlude().w(px(ROW_MENU_W)).py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                w.files_menu = None;
                cx.notify();
            }))
            .child(item("files-new", Icon::Plus, "New document").on_click(cx.listener(|w, _, window, cx| {
                w.files_menu = None;
                w.new_doc(window, cx);
            })))
            .child(item("files-new-folder", Icon::FolderNew, "New folder").on_click(cx.listener(|w, _, window, cx| {
                w.files_menu = None;
                w.new_folder(None, window, cx);
            })))
            .child(item("files-select", Icon::Select, "Select documents").on_click(cx.listener(|w, _, _, cx| {
                w.files_menu = None;
                w.batch = Some(vec![]);
                w.folded[1] = false;
                cx.notify();
            })));
        Some(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }

    pub(crate) fn render_row_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(m) = self.row_menu.clone() else { return vec![] };
        let Some((pinned, starred, folder, typst)) = self.entries.iter().find(|e| e.id == m.id).map(|e| (e.meta.pinned, e.meta.starred > 0, e.folder, e.typst)) else { return vec![] };
        let item_c = |id: &'static str, ic: Icon, label: &'static str, color: Hsla, icon_color: Hsla| {
            crate::ui::menu_row(id)
                .mx(px(0.))
                .text_color(color)
                .child(icon(ic, 16., icon_color))
                .child(div().flex_1().min_w_0().child(crate::ui::menu_label(label)))
        };
        let item = |id, ic, label| item_c(id, ic, label, Theme::text(), Theme::text_code());
        let divider = || div().mx(px(8.)).my(px(5.)).h(px(1.)).bg(Theme::line_border());
        if m.batch {
            // Right-click in Selection mode: the batch menu minus Add shortcut to, Transfer ownership.
            let n = self.batch.as_ref().map_or(0, |b| b.len());
            let menu = popover()
                .id("batch-menu")
                .occlude()
                .w(px(ROW_MENU_W))
                .px(px(5.))
                .py(px(crate::ui::MENU_PAD_Y))
                .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                    w.row_menu = None;
                    cx.notify();
                }))
                .child(div().h(px(ROW_ITEM)).px(px(8.)).flex().items_center().text_size(px(12.)).text_color(Theme::text_placeholder()).child(format!("Selected: {n}")))
                .child(item("bm-copy", Icon::Copy, "Duplicate").on_click(cx.listener(|w, _, _, cx| w.batch_copy(cx))))
                .child(item("bm-move", Icon::Move, "Move to").on_click(cx.listener(|w, _, window, cx| w.batch_move(window, cx))))
                .child(item("bm-delete", Icon::Delete, "Delete").on_click(cx.listener(|w, _, _, cx| w.ask_batch_delete(cx))));
            return vec![
                deferred(anchored().anchor(m.corner).position(m.pos).snap_to_window_with_margin(px(8.)).child(menu))
                    .with_priority(2)
                    .into_any_element(),
            ];
        }
        let id = m.id.clone();
        let close = cx.listener(|w: &mut Workspace, _: &MouseDownEvent, _, cx| {
            w.row_menu = None;
            cx.notify();
        });
        let mut menu = popover().id("row-menu").occlude().w(px(ROW_MENU_W)).px(px(5.)).py(px(crate::ui::MENU_PAD_Y)).on_mouse_down_out(close);
        let root = self.store.root();
        let reveal = id.clone();
        let reveal_item = item("rm-reveal", Icon::Move, if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in file manager" }).on_click(cx.listener(move |w, _, _, cx| {
            w.row_menu = None;
            cx.reveal_path(&root.join(&reveal));
            cx.notify();
        }));
        if folder {
            let (doc, sub, mv, ren, del) = (id.clone(), id.clone(), id.clone(), id.clone(), id);
            menu = menu
                .child(item("rm-new", Icon::Plus, "New document").on_click(cx.listener(move |w, _, window, cx| {
                    w.row_menu = None;
                    w.new_child(&doc, window, cx);
                })))
                .child(item("rm-new-folder", Icon::FolderNew, "New folder").on_click(cx.listener(move |w, _, window, cx| {
                    w.row_menu = None;
                    w.new_folder(Some(&sub), window, cx);
                })))
                .child(divider())
                .child(item("rm-rename", Icon::Edit, "Rename").on_click(cx.listener(move |w, _, window, cx| w.start_rename(&ren, window, cx))))
                .child(item("rm-move", Icon::Move, "Move to").on_click(cx.listener(move |w, _, window, cx| w.open_move(vec![mv.clone()], window, cx))))
                .child(reveal_item)
                .child(divider())
                .child(item_c("rm-del", Icon::Delete, "Delete", Theme::danger(), Theme::danger()).on_click(cx.listener(move |w, _, _, cx| w.ask_delete(&del, cx))));
        } else {
            let (dup, mv, pin, star, pdf, ren, del, new_window) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id);
            if typst {
                menu = menu
                    .child(item("rm-window", Icon::Window, "Open in new window").on_click(cx.listener(move |w, _, _, cx| {
                        w.row_menu = None;
                        let id = new_window.clone();
                        cx.defer(move |cx| crate::open_main_window(false, Some(id), cx));
                        cx.notify();
                    })))
                    .child(item("rm-copy", Icon::Copy, "Duplicate").on_click(cx.listener(move |w, _, window, cx| w.duplicate_doc(&dup, window, cx))));
            }
            menu = menu
                .child(item("rm-move", Icon::Move, "Move to").on_click(cx.listener(move |w, _, window, cx| w.open_move(vec![mv.clone()], window, cx))))
                .child(item("rm-pin", if pinned { Icon::Unpin } else { Icon::Pin }, if pinned { "Unpin" } else { "Pin" }).on_click(cx.listener(move |w, _, _, cx| w.toggle_pin(&pin, cx))))
                .child(item("rm-star", if starred { Icon::Unstar } else { Icon::Star }, if starred { "Remove from favorites" } else { "Add to favorites" }).on_click(cx.listener(move |w, _, _, cx| w.toggle_star(&star, cx))))
                .child(divider());
            if typst {
                menu = menu.child(item("rm-pdf", Icon::Export, "Export as PDF").on_click(cx.listener(move |w, _, _, cx| {
                    w.row_menu = None;
                    w.export_pdf(&pdf, cx);
                })));
            }
            menu = menu
                .child(item("rm-rename", Icon::Edit, "Rename").on_click(cx.listener(move |w, _, window, cx| w.start_rename(&ren, window, cx))))
                .child(reveal_item)
                .child(divider())
                .child(item_c("rm-del", Icon::Delete, "Delete", Theme::danger(), Theme::danger()).on_click(cx.listener(move |w, _, _, cx| w.ask_delete(&del, cx))));
        }
        vec![
            deferred(anchored().anchor(m.corner).position(m.pos).snap_to_window_with_margin(px(8.)).child(menu))
                .with_priority(2)
                .into_any_element(),
        ]
    }
}
