//! Moving documents, and the Move to dialog.

use super::Tree;
use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use crate::overlay::Overlay;
use crate::store::{Entry, display_title};
use crate::theme::{self, Theme, craft};
use crate::ui::{Icon, ModalAt, ROW_RADIUS, close_button, dialog, icon, primary_button, secondary_button, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::collections::HashSet;
use thesio_core::paths::is_within;

/// The Move to dialog.
pub struct MoveDialog {
    /// Pages being moved (each with its subtree).
    pub ids: Vec<String>,
    /// Chosen destination: `Some(None)` is the top level of My documents.
    pub target: Option<Option<String>>,
    pub expanded: HashSet<String>,
    pub search: Entity<TextField>,
    pub scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub(crate) fn open_move(&mut self, ids: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| {
            let mut f = TextField::new("Search destinations", cx);
            f.placeholder_color = Theme::text_placeholder();
            f
        });
        let subs = vec![cx.subscribe(&search, |_, _, _: &TextFieldEvent, cx| cx.notify())];
        let focus = search.focus_handle(cx);
        self.open_overlay(Overlay::Move(MoveDialog { ids, target: None, expanded: HashSet::new(), search, scroll: ScrollHandle::new(), _subs: subs }), window, cx);
        window.focus(&focus);
    }

    fn move_dialog(&mut self) -> Option<&mut MoveDialog> {
        match &mut self.overlay {
            Some(Overlay::Move(dialog)) => Some(dialog),
            _ => None,
        }
    }

    /// Moves files and folders into `target` (`None`: the vault root), skipping
    /// what is already there and moves of a folder into itself.
    pub(crate) fn move_docs(&mut self, ids: &[String], target: Option<String>, cx: &mut Context<Self>) {
        self.flush(cx);
        let (mut moved, mut updated, mut problems) = (0, 0, vec![]);
        // An item inside another selected folder goes with it.
        for id in ids.iter().filter(|id| !ids.iter().any(|o| o != *id && is_within(id, o))) {
            let parent = self.entries.iter().find(|e| &e.id == id).and_then(|e| e.parent.clone());
            let inside = target.as_ref().is_some_and(|t| is_within(t, id));
            if inside || parent == target {
                continue;
            }
            let result = self.store.borrow_mut().set_parent(id, target.as_deref());
            match result {
                Ok(outcome) => {
                    self.path_changed(id, &outcome.id, &outcome.rewritten, cx);
                    moved += 1;
                    updated += outcome.rewritten.len();
                    problems.extend(outcome.warnings);
                }
                Err(e) => problems.push(format!("Could not move: {e}")),
            }
        }
        if let Some(t) = &target {
            self.set_expanded(t, true, cx);
            self.reveal_in_tree(t, cx);
        }
        if updated > 0 {
            self.show_toast(&crate::app::updated_paths(updated), cx);
        } else if moved > 0 {
            self.show_toast("Moved successfully", cx);
        }
        self.show_warnings(&problems, cx);
        cx.notify();
    }

    /// The Move to dialog: My documents on the left, its page tree on the right.
    /// Quiet selection fills, a 30pt search field, the modal buttons.
    pub(crate) fn render_move_dialog(&self, dlg: &MoveDialog, cx: &mut Context<Self>) -> AnyElement {
        let tree = Tree::new(&self.entries);
        let hidden: HashSet<String> = dlg.ids.iter().flat_map(|id| tree.subtree(id)).collect();
        let q = dlg.search.read(cx).text.trim().to_lowercase();
        let border = Theme::line_border();

        let search = div()
            .mt(px(12.))
            .mx(px(8.))
            .mb(px(10.))
            .h(px(30.))
            .flex_none()
            .pl(px(10.))
            .pr(px(8.))
            .flex()
            .items_center()
            .rounded(px(9.))
            .border_1()
            .border_color(Theme::line_border())
            .bg(crate::theme::raised(Theme::bg_body()))
            .child(icon(Icon::Search, 15., Theme::text_caption()))
            .child(div().flex_1().min_w_0().ml(px(6.)).flex().child(dlg.search.clone()));
        // The location column reads like a sidebar: a grey surface and
        // a quiet pill for the selected location.
        let left = div()
            .w(px(240.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(Theme::bg_overlay())
            .rounded_l(px(9.))
            .child(search)
            .child(
                div()
                    .mx(px(8.))
                    .h(px(30.))
                    .pl(px(10.))
                    .pr(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(9.))
                    .bg(Theme::fill_hover_strong())
                    .child(icon(Icon::Library, 15., Theme::text_code()))
                    .child(div().ml(px(8.)).text_size(px(13.)).line_height(px(20.)).child("Vault")),
            );

        let row = |key: String, selected: bool| {
            div()
                .id(SharedString::from(key))
                .h(px(30.))
                .mb(px(2.))
                .pr(px(5.))
                .flex()
                .items_center()
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .when(selected, |d| d.bg(Theme::fill_hover_strong()))
        };
        let title = |t: String| div().ml(px(9.)).flex_1().min_w_0().text_size(px(13.)).line_height(px(20.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(t));
        let page_icon = |_: &Entry| icon(Icon::Folder, 16., Theme::text_caption()).into_any_element();
        let mut list = div().id("move-tree").flex_1().min_w_0().px(px(8.)).py(px(12.)).overflow_y_scroll().track_scroll(&dlg.scroll);
        if q.is_empty() {
            list = list.child(
                row("move-root".into(), dlg.target == Some(None))
                    .pl(px(16.))
                    .child(icon(Icon::Library, 16., Theme::text_caption()))
                    .child(title("Vault".into()))
                    .on_click(cx.listener(|w, _, _, cx| {
                        if let Some(d) = w.move_dialog() {
                            d.target = Some(None);
                        }
                        cx.notify();
                    })),
            );
            for (e, depth) in tree.rows(|id| dlg.expanded.contains(id)).into_iter().filter(|(e, _)| e.folder) {
                if hidden.contains(&e.id) {
                    continue;
                }
                let kids = tree.children(Some(&e.id)).iter().any(|c| !hidden.contains(&c.id));
                let open = dlg.expanded.contains(&e.id);
                let (pick, toggle) = (e.id.clone(), e.id.clone());
                list = list.child(
                    row(format!("move-{}", e.id), dlg.target.as_ref().is_some_and(|t| t.as_deref() == Some(e.id.as_str())))
                        .pl(px(16. + 20. * depth as f32))
                        .child(
                            div()
                                .id(SharedString::from(format!("move-arrow-{}", e.id)))
                                .size(px(20.))
                                .p(px(5.))
                                .mr(px(8.))
                                .flex_none()
                                .rounded(px(5.))
                                .when(kids, |d| {
                                    d.child(icon(Icon::ChevronRight, 10., theme::c(craft::N500)).when(open, |s| s.with_transformation(Transformation::rotate(radians(std::f32::consts::FRAC_PI_2)))))
                                        .on_click(cx.listener(move |w, _, _, cx| {
                                            cx.stop_propagation();
                                            if let Some(d) = w.move_dialog()
                                                && !d.expanded.remove(&toggle) {
                                                    d.expanded.insert(toggle.clone());
                                                }
                                            cx.notify();
                                        }))
                                }),
                        )
                        .child(page_icon(e))
                        .child(title(display_title(&e.title)))
                        .on_click(cx.listener(move |w, _, _, cx| {
                            if let Some(d) = w.move_dialog() {
                                d.target = Some(Some(pick.clone()));
                            }
                            cx.notify();
                        })),
                );
            }
        } else {
            let hits: Vec<&Entry> = self.entries.iter().filter(|e| !hidden.contains(&e.id) && display_title(&e.title).to_lowercase().contains(&q)).collect();
            if hits.is_empty() {
                list = list.child(div().pt(px(80.)).flex().justify_center().text_size(px(13.)).text_color(Theme::text_caption()).child("No results"));
            }
            for e in hits {
                let pick = e.id.clone();
                list = list.child(
                    row(format!("move-hit-{}", e.id), dlg.target.as_ref().is_some_and(|t| t.as_deref() == Some(e.id.as_str())))
                        .pl(px(16.))
                        .child(page_icon(e))
                        .child(title(display_title(&e.title)))
                        .on_click(cx.listener(move |w, _, _, cx| {
                            if let Some(d) = w.move_dialog() {
                                d.target = Some(Some(pick.clone()));
                            }
                            cx.notify();
                        })),
                );
            }
        }

        let ready = dlg.target.is_some();
        // OK waits for a target: the accent, faded, until one is picked.
        let ok = primary_button("move-ok", "OK");
        let ok = if ready {
            ok
        } else {
            let faded = theme::ca(theme::accent(), 0.4);
            ok.bg(faded).cursor_default().active(move |s| s.bg(faded))
        };
        let dialog = dialog("move-dialog")
            .relative()
            .w(px(828.))
            .flex()
            .flex_col()
            .child(div().pt(px(24.)).pr(px(56.)).pl(px(24.)).text_size(px(15.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).child("Move from My documents to"))
            .child(close_button("move-close").absolute().top(px(27.)).right(px(25.)).on_click(cx.listener(|w, _, window, cx| w.close_overlay(window, cx))))
            .child(
                div().px(px(24.)).mt(px(16.)).mb(px(24.)).child(
                    div()
                        .h(px(512.))
                        .flex()
                        .border_1()
                        .border_color(border)
                        .rounded(px(10.))
                        .child(left)
                        .child(div().w(px(1.)).h_full().flex_none().bg(border))
                        .child(list),
                ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(24.))
                    .flex()
                    .justify_end()
                    .gap(px(12.))
                    .child(secondary_button("move-cancel", "Cancel").on_click(cx.listener(|w, _, window, cx| w.close_overlay(window, cx))))
                    .child(ok.on_click(cx.listener(|w, _, window, cx| {
                        let Some(d) = w.move_dialog() else { return };
                        let Some(target) = d.target.clone() else { return };
                        let ids = d.ids.clone();
                        w.batch = None;
                        w.move_docs(&ids, target, cx);
                        w.close_overlay(window, cx);
                    }))),
            );
        crate::ui::modal("move-mask", ModalAt::Center, true, dialog, cx)
    }
}
