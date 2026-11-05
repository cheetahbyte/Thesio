//! The Trash page. Deleted pages wait in the library's `trash/` for 30 days; each row is a
//! deleted page together with the child pages that went with it.

use crate::app::Workspace;
use crate::confirm::Confirm;
use crate::store::display_title;
use crate::search::doc_icon;
use crate::store::{TRASH_DAYS, TrashEntry};
use crate::theme::Theme;
use crate::overlay::Overlay;
use crate::ui::{Icon, ROW_RADIUS, TipExt, anchored_menu, empty_state, icon, list_header_text, list_line, menu_label, menu_panel, page_header, ui_text};
use gpui::{prelude::FluentBuilder, *};

pub const TRASH_MENU_W: f32 = 160.;

/// Time remaining: whole days left before automatic permanent deletion.
fn remaining(deleted: u64) -> String {
    let end = deleted + TRASH_DAYS * 86_400;
    let left = end.saturating_sub(crate::time::now());
    if left < 86_400 { "Deleting soon".into() } else { format!("{} days", left.div_ceil(86_400)) }
}

impl Workspace {
    /// Where a trashed page goes back to: its parent if that page is live.
    fn trash_location(&self, t: &TrashEntry) -> String {
        t.parent
            .as_ref()
            .and_then(|p| self.entries.iter().find(|e| &e.id == p))
            .map_or_else(|| "My documents".to_string(), |e| display_title(&e.title))
    }

    pub(crate) fn render_trash(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let header = page_header(self, "Trash", None, cx).child(crate::ui::toolbar_group().child(
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
                .child(label)
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
            let menu_open = matches!(&self.overlay, Some(Overlay::TrashMenu { id, .. }) if *id == t.id);
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
                .on_click(cx.listener(move |w, ev: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    let p = ev.position();
                    w.open_overlay(Overlay::TrashMenu { id: id.clone(), position: point(p.x + px(12.), p.y + px(14.)), corner: Corner::TopRight }, window, cx);
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
                        cx.listener(move |w, ev: &MouseDownEvent, window, cx| {
                            w.open_overlay(Overlay::TrashMenu { id: id2.clone(), position: ev.position, corner: Corner::TopLeft }, window, cx);
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

    pub(crate) fn render_trash_menu(&self, id: &str, position: Point<Pixels>, corner: Corner, cx: &mut Context<Self>) -> AnyElement {
        let item = |key: &'static str, ic: Icon, label: &'static str, color: Hsla| {
            crate::ui::menu_row(key)
                .text_color(color)
                .child(icon(ic, 16., if color == Theme::text() { Theme::text_code() } else { color }))
                .child(menu_label(label))
        };
        let (a, b) = (id.to_string(), id.to_string());
        let menu = menu_panel("trash-menu", TRASH_MENU_W, cx)
            .child(item("tm-restore", Icon::Restore, "Restore", Theme::text()).on_click(cx.listener(move |w, _, window, cx| w.open_overlay(Overlay::Confirm(Confirm::Restore(a.clone())), window, cx))))
            .child(item("tm-purge", Icon::Delete, "Delete permanently", Theme::danger()).on_click(cx.listener(move |w, _, window, cx| w.open_overlay(Overlay::Confirm(Confirm::Purge(b.clone())), window, cx))));
        anchored_menu(corner, position, menu)
    }

    pub(crate) fn restore_doc(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut warnings = vec![];
        let result = self.store.borrow_mut().restore(id);
        match result {
            Ok(restored) => {
                warnings = restored.warnings;
                self.show_toast(if restored.renamed { "Restored. Some documents were moved to My documents because their original location no longer exists." } else { "Restored successfully" }, cx);
            }
            Err(e) => {
                let detail = e.chain().map(ToString::to_string).collect::<Vec<_>>().join(": ");
                self.show_toast_kind(crate::ui::ToastKind::Error, &format!("Could not restore document: {detail}"), cx);
            }
        }
        self.refresh_entries();
        self.refresh_trash(cx);
        self.show_warnings(&warnings, cx);
    }

    pub(crate) fn purge_doc(&mut self, id: &str, cx: &mut Context<Self>) {
        let result = self.store.borrow_mut().purge(id);
        match result {
            Ok(()) => self.show_toast("Permanently deleted", cx),
            Err(e) => self.show_toast_kind(crate::ui::ToastKind::Error, &format!("Could not delete document: {e}"), cx),
        }
        self.refresh_trash(cx);
    }

    /// Lists the Trash, emptying what is past its time.
    pub(crate) fn refresh_trash(&mut self, cx: &mut Context<Self>) {
        let warnings = self.store.borrow_mut().purge_expired();
        self.trash = self.store.borrow().list_trash();
        self.show_warnings(&warnings, cx);
    }
}
