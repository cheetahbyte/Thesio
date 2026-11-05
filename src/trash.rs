//! Trash page and the confirmation dialogs (Delete, Restore, Delete permanently).
//! Deleted pages wait in the library's `trash/` for 30 days; each row is a
//! deleted page together with the child pages that went with it.

use crate::app::{Confirm, Workspace};
use crate::settings::display_title;
use crate::search::doc_icon;
use crate::store::{TRASH_DAYS, TrashEntry};
pub(crate) use crate::ui::{close_button, dialog, list_header_text, list_line, menu_label, primary_button, scrim, secondary_button};
use crate::theme::{self, Theme};
use crate::ui::{Icon, ROW_RADIUS, TipExt, TitleBarExt, icon, popover, ui_para, ui_text};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::references::cited_keys;

pub const TRASH_MENU_W: f32 = 160.;

/// Time remaining: whole days left before automatic permanent deletion.
fn remaining(deleted: u64) -> String {
    let end = deleted + TRASH_DAYS * 86_400;
    let left = end.saturating_sub(crate::time::now());
    if left < 86_400 { "Deleting soon".into() } else { format!("{} days", left.div_ceil(86_400)) }
}

/// The page header, as tall as the document's so the header pills sit in
/// the same place: a 17pt semibold view title, and the open-sidebar
/// button when the sidebar is hidden.
pub(crate) fn page_header(ws: &Workspace, title: &str, count: Option<usize>, left: f32, cx: &mut Context<Workspace>) -> Stateful<Div> {
    div()
        .id("page-header")
        .h(px(crate::app::HEADER_H))
        .flex_none()
        .flex()
        .items_center()
        // Clear of the traffic lights, like the document header.
        .pl(if ws.sidebar_open { px(left) } else { px(theme::TRAFFIC_LIGHTS_W) })
        .pr(px(crate::ui::header_right_inset()))
        .title_bar()
        .when(!ws.sidebar_open && cfg!(target_os = "windows"), |d| d.child(div().mr(px(12.)).child(ws.header_navigation(cx))))
        .when(!ws.sidebar_open && !cfg!(target_os = "windows"), |d| {
            d.child(
                div()
                    .id("open-sidebar")
                    .no_drag()
                    .flex_none()
                    .ml(px(4.))
                    .mr(px(12.))
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .tip("Show sidebar", Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                    .child(icon(Icon::SidebarToggle, 17., Theme::icon()))
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.sidebar_open = true;
                        cx.notify();
                    })),
            )
        })
        .child(div().text_size(px(15.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).child(crate::settings::label(title)))
        .children(count.map(|n| div().ml(px(8.)).text_size(px(13.)).text_color(Theme::text_caption()).child(n.to_string())))
        .child(div().flex_1())
}

/// Centred empty state: a large glyph, a title, and a caption.
pub(crate) fn empty_state(glyph: Icon, title: &'static str, text: &'static str) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .pb(px(64.))
        .child(icon(glyph, 44., Theme::text_placeholder()))
        .child(div().mt(px(14.)).text_size(px(15.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title))
        .child(div().mt(px(2.)).text_size(px(13.)).line_height(px(18.)).text_color(Theme::text_caption()).child(text))
}

impl Workspace {
    /// Where a trashed page goes back to: its parent if that page is live.
    fn trash_location(&self, t: &TrashEntry) -> String {
        t.parent
            .as_ref()
            .and_then(|p| self.entries.iter().find(|e| &e.id == p))
            .map_or_else(|| crate::settings::label("My documents"), |e| display_title(&e.title))
    }

    pub(crate) fn render_trash(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let header = page_header(self, "Trash", None, 26., cx).child(crate::ui::toolbar_group().child(
            crate::ui::header_icon("trash-search", Icon::Search)
                .tip_label("Search (⌘ + J)".into(), crate::ui::TipSide::Below)
                .on_click(cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(crate::app::FocusSearch), cx))),
        ));
        let mut page = div().flex_1().min_w_0().h_full().flex().flex_col().child(header);
        if self.trash.is_empty() {
            return page.child(empty_state(Icon::Delete, "Trash is empty", "Deleted documents stay here for a while.")).into_any_element();
        }
        // The list: 13pt tertiary headers over a hairline,
        // 15pt titles, 13pt secondary cells.
        let col = |w: Option<f32>, label: &'static str| {
            div()
                .when_some(w, |d, w| d.w(px(w)).flex_none())
                .when(w.is_none(), |d| d.flex_1().min_w_0())
                .pl(px(20.))
                .text_size(px(13.))
                .line_height(px(24.))
                .text_color(list_header_text())
                .child(crate::settings::label(label))
        };
        let table_header = div()
            .h(px(33.))
            .flex_none()
            .mx(px(24.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(list_line())
            .child(col(None, "Title"))
            .child(col(Some(168.), "Location"))
            .child(col(Some(168.), "Time remaining"))
            .child(div().w(px(48.)).flex_none());
        let mut list = div().id("trash-list").flex_1().min_h_0().overflow_y_scroll().px(px(24.)).pb(px(24.));
        for (i, t) in self.trash.iter().enumerate() {
            let (id, id2) = (t.id.clone(), t.id.clone());
            let menu_open = self.trash_menu.as_ref().is_some_and(|(m, _, _)| *m == t.id);
            let title = display_title(&t.title);
            let more = div()
                .id(SharedString::from(format!("trash-more-{}", t.id)))
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .when(menu_open, |d| d.bg(Theme::fill_hover_strong()))
                .child(icon(Icon::Ellipsis, 16., Theme::text_caption()))
                .on_click(cx.listener(move |w, ev: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    let p = ev.position();
                    w.trash_menu = Some((id.clone(), point(p.x + px(12.), p.y + px(14.)), Corner::TopRight));
                    cx.notify();
                }));
            list = list.child(
                div()
                    .id(SharedString::from(format!("trash-row-{}", t.id)))
                    .h(px(56.))
                    .flex()
                    .items_center()
                    .when(i > 0, |d| d.border_t_1().border_color(list_line()))
                    .when(menu_open, |d| d.bg(Theme::fill_hover()).border_color(transparent_black()).rounded(px(ROW_RADIUS)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                            w.trash_menu = Some((id2.clone(), ev.position, Corner::TopLeft));
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pl(px(24.))
                            .flex()
                            .items_center()
                            .child(doc_icon())
                            .child(div().min_w_0().text_size(px(15.)).line_height(px(22.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(title))),
                    )
                    .child(
                        div()
                            .w(px(168.))
                            .flex_none()
                            .pl(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon(Icon::Library, 16., Theme::text_placeholder()))
                            .child(div().min_w_0().text_size(px(13.)).text_color(Theme::text_caption()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(self.trash_location(t)))),
                    )
                    .child(div().w(px(168.)).flex_none().pl(px(20.)).text_size(px(13.)).text_color(Theme::text_caption()).child(remaining(t.deleted)))
                    .child(div().w(px(48.)).flex_none().flex().justify_end().pr(px(12.)).child(more)),
            );
        }
        page = page.child(table_header).child(list);
        page.into_any_element()
    }

    pub(crate) fn render_trash_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (id, pos, corner) = self.trash_menu.clone()?;
        let item = |key: &'static str, ic: Icon, label: &'static str, color: Hsla| {
            crate::ui::menu_row(key)
                .text_color(color)
                .child(icon(ic, 16., if color == Theme::text() { Theme::text_code() } else { color }))
                .child(menu_label(label))
        };
        let (a, b) = (id.clone(), id);
        let menu = popover()
            .id("trash-menu")
            .occlude()
            .w(px(TRASH_MENU_W))
            .py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                w.trash_menu = None;
                cx.notify();
            }))
            .child(item("tm-restore", Icon::Restore, "Restore", Theme::text()).on_click(cx.listener(move |w, _, _, cx| {
                w.trash_menu = None;
                w.confirm = Some(Confirm::Restore(a.clone()));
                cx.notify();
            })))
            .child(item("tm-purge", Icon::Delete, "Delete permanently", Theme::danger()).on_click(cx.listener(move |w, _, _, cx| {
                w.trash_menu = None;
                w.confirm = Some(Confirm::Purge(b.clone()));
                cx.notify();
            })));
        Some(
            deferred(anchored().anchor(corner).position(pos).snap_to_window_with_margin(px(8.)).child(menu))
                .with_priority(2)
                .into_any_element(),
        )
    }

    fn restore_doc(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.store.restore(id) {
            Ok(moved) => {
                self.show_toast(if moved { "Restored. Some documents were moved to My documents because their original location no longer exists." } else { "Restored successfully" }, cx);
            }
            Err(e) => {
                let detail = e.chain().map(|cause| crate::settings::diagnostic(&cause.to_string())).collect::<Vec<_>>().join(": ");
                let msg = format!("Could not restore document: {}", detail);
                self.show_toast_kind(crate::ui::ToastKind::Error, &msg, cx);
                self.status = crate::app::SaveStatus::Failed(msg);
            }
        }
        self.entries = self.store.list();
        self.sort();
        self.trash = self.store.list_trash();
        self.show_store_warnings(cx);
    }

    fn purge_doc(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.store.purge(id) {
            Ok(()) => self.show_toast("Permanently deleted", cx),
            Err(e) => self.status = crate::app::SaveStatus::Failed(format!("Could not delete document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        self.trash = self.store.list_trash();
        self.show_store_warnings(cx);
    }

    fn show_store_warnings(&mut self, cx: &mut Context<Self>) {
        let warnings = self.store.take_warnings();
        if !warnings.is_empty() {
            let msg = warnings.iter().map(|s| crate::settings::diagnostic(s)).collect::<Vec<_>>().join("; ");
            self.show_toast_kind(crate::ui::ToastKind::Error, &msg, cx);
            self.status = crate::app::SaveStatus::Failed(msg);
        }
    }

    /// The confirmation dialog: a modal (white, radius 16), a 15pt
    /// semibold title, one line of 13pt body, Cancel and the action button on
    /// the right.
    pub(crate) fn render_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = self.confirm.as_ref()?;
        let title_of = |id: &str| {
            self.entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| display_title(&e.title))
                .or_else(|| self.trash.iter().find(|t| t.id == id).map(|t| display_title(&t.title)))
                .unwrap_or_default()
        };
        let (title, body, action, danger) = match c {
            Confirm::Delete { ids, batch } => (
                if *batch || ids.len() > 1 { format!("Delete the {} selected items?", ids.len()) } else { format!("Delete {}?", title_of(&ids[0])) },
                "Deleted items will be moved to Trash and permanently deleted after 30 days.".to_string(),
                "Delete",
                true,
            ),
            Confirm::Restore(_) => ("Restore this document?".to_string(), "The document will be restored to its original location.".to_string(), "Restore", false),
            Confirm::Purge(_) => ("Delete permanently?".to_string(), "This page and its child pages cannot be recovered after permanent deletion.".to_string(), "Delete", true),
            Confirm::DeleteEntry(key) => {
                let citing = self.entries.iter().filter(|e| e.typst && cited_keys(&e.body).contains(key)).count();
                let effect = match citing {
                    0 => "No document cites it.".to_string(),
                    1 => "1 document cites it and will show an error.".to_string(),
                    n => format!("{n} documents cite it and will show an error."),
                };
                (format!("Delete {key}?"), format!("This removes it from the library and cannot be undone. {effect}"), "Delete", true)
            }
            Confirm::SaveFailed => (
                "Could not save document".to_string(),
                format!("{}. Retry, or cancel and export a copy using More → Export as PDF. Discarding changes will lose your unsaved work.", self.save_error()),
                "Retry",
                false,
            ),
        };
        let ok = if danger { crate::ui::danger_button("confirm-ok", action) } else { primary_button("confirm-ok", action) };
        let dialog = dialog("confirm-dialog")
            .w(px(400.))
            .px(px(24.))
            .pt(px(20.))
            .pb(px(24.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(15.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).child(ui_para(title)))
            .child(div().mt(px(8.)).text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(ui_para(body)))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1())
                    .child(secondary_button("confirm-cancel", "Cancel").on_click(cx.listener(|w, _, _, cx| {
                        w.confirm = None;
                        w.after_save = None;
                        cx.notify();
                    })))
                    .when(matches!(c, Confirm::SaveFailed), |d| {
                        d.child(secondary_button("confirm-discard", "Discard changes").text_color(Theme::danger()).on_click(cx.listener(|w, _, window, cx| {
                            w.confirm = None;
                            w.resolve_save_failed(false, window, cx);
                        })))
                    })
                    .child(ok.on_click(cx.listener(|w, _, window, cx| {
                        let Some(c) = w.confirm.take() else { return };
                        match c {
                            Confirm::Delete { ids, batch } => {
                                if batch {
                                    w.batch = None;
                                }
                                w.delete_docs(&ids, window, cx);
                            }
                            Confirm::Restore(id) => w.restore_doc(&id, cx),
                            Confirm::Purge(id) => w.purge_doc(&id, cx),
                            Confirm::DeleteEntry(key) => w.delete_entry(&key, cx),
                            Confirm::SaveFailed => w.resolve_save_failed(true, window, cx),
                        }
                        cx.notify();
                    }))),
            );
        Some(
            deferred(
                div()
                    .id("confirm-mask")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(scrim())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}
