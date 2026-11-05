//! The vault as a tree: folders first, then names in natural order. Also the
//! expand state, new files inside folders, moving, and the breadcrumb chain.

use crate::app::Workspace;
use crate::settings::display_title;
use crate::input::{TextField, TextFieldEvent};
use crate::store::Entry;
use thesio_core::paths::{is_within, natural_cmp};
use crate::theme::{self, Theme, craft};
use crate::trash::{close_button, dialog, primary_button, scrim, secondary_button};
use crate::ui::{Icon, ROW_RADIUS, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::collections::{HashMap, HashSet};

/// Children of every folder, keyed by parent (`None`: the vault root). An
/// entry whose parent is missing is shown at the top level.
pub(crate) struct Tree<'a> {
    roots: Vec<&'a Entry>,
    kids: HashMap<&'a str, Vec<&'a Entry>>,
}

impl<'a> Tree<'a> {
    pub fn new(entries: &'a [Entry]) -> Self {
        let ids: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        let parent = |e: &'a Entry| e.parent.as_deref().filter(|p| ids.contains(p));
        let mut roots: Vec<&Entry> = vec![];
        let mut kids: HashMap<&str, Vec<&Entry>> = HashMap::new();
        for e in entries {
            match parent(e) {
                Some(p) => kids.entry(p).or_default().push(e),
                None => roots.push(e),
            }
        }
        let order = |a: &&Entry, b: &&Entry| b.folder.cmp(&a.folder).then_with(|| natural_cmp(&a.title, &b.title));
        roots.sort_by(order);
        for v in kids.values_mut() {
            v.sort_by(order);
        }
        Tree { roots, kids }
    }

    pub fn children(&self, id: Option<&str>) -> &[&'a Entry] {
        match id {
            None => &self.roots,
            Some(id) => self.kids.get(id).map_or(&[], |v| v.as_slice()),
        }
    }


    /// Pre-order rows with their depth, descending into pages `open` says yes to.
    pub fn rows(&self, open: impl Fn(&str) -> bool) -> Vec<(&'a Entry, usize)> {
        let mut out = vec![];
        let mut stack: Vec<(&Entry, usize)> = self.children(None).iter().rev().map(|e| (*e, 0)).collect();
        while let Some((e, d)) = stack.pop() {
            out.push((e, d));
            if open(&e.id) {
                stack.extend(self.children(Some(&e.id)).iter().rev().map(|c| (*c, d + 1)));
            }
        }
        out
    }

    /// `id` and every page under it.
    pub fn subtree(&self, id: &str) -> Vec<String> {
        let mut out = vec![id.to_string()];
        let mut i = 0;
        while i < out.len() {
            let kids: Vec<String> = self.children(Some(&out[i])).iter().map(|e| e.id.clone()).collect();
            out.extend(kids);
            i += 1;
        }
        out
    }
}

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
    /// Ancestors of `id`, top level first.
    pub(crate) fn ancestors(&self, id: &str) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = vec![];
        let mut cur = self.entries.iter().find(|e| e.id == id);
        while let Some(p) = cur.and_then(|e| e.parent.as_deref()).and_then(|p| self.entries.iter().find(|e| e.id == p)) {
            if p.id == id || out.iter().any(|e| e.id == p.id) {
                break;
            }
            out.push(p);
            cur = Some(p);
        }
        out.reverse();
        out
    }

    /// Expand the sidebar down to `id`.
    pub(crate) fn reveal_in_tree(&mut self, id: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.ancestors(id).iter().map(|e| e.id.clone()).collect();
        for id in ids { self.set_expanded(&id, true, cx); }
    }


    /// A new document in the folder `parent`, or beside it when it is a file.
    pub fn new_child(&mut self, parent: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Only folders hold files: a file's "+" adds a sibling.
        let parent = self.entries.iter().find(|e| e.id == parent).and_then(|e| if e.folder { Some(e.id.clone()) } else { e.parent.clone() });
        self.when_saved(window, cx, move |w, window, cx| w.create_doc(parent.as_deref(), window, cx));
    }

    pub(crate) fn open_move(&mut self, ids: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.row_menu = None;
        let search = cx.new(|cx| {
            let mut f = TextField::new("Search destinations", cx);
            f.placeholder_color = Theme::text_placeholder();
            f
        });
        let subs = vec![cx.subscribe_in(&search, window, |w, _, ev: &TextFieldEvent, window, cx| {
            if let TextFieldEvent::Cancel = ev {
                w.close_move(window, cx);
            }
            cx.notify();
        })];
        window.focus(&search.focus_handle(cx));
        self.move_dlg = Some(MoveDialog { ids, target: None, expanded: HashSet::new(), search, scroll: ScrollHandle::new(), _subs: subs });
        cx.notify();
    }

    fn close_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_dlg = None;
        if let Some(ed) = &self.editor {
            ed.read(cx).focus(window);
        }
        cx.notify();
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

    /// Opens or closes a folder and remembers it for the next launch.
    pub(crate) fn set_expanded(&mut self, id: &str, open: bool, cx: &mut Context<Self>) {
        let changed = if open { self.expanded.insert(id.to_string()) } else { self.expanded.remove(id) };
        if changed { self.save_expanded(cx); }
    }

    pub(crate) fn toggle_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        self.set_expanded(id, !self.expanded.contains(id), cx);
        cx.notify();
    }

    pub(crate) fn save_expanded(&mut self, cx: &mut Context<Self>) {
        let mut expanded: Vec<String> = self.expanded.iter().cloned().collect();
        expanded.sort();
        let result = self.store.borrow().update_settings(|s| s.expanded = expanded);
        if let Err(e) = result { self.show_toast_kind(crate::ui::ToastKind::Error, &format!("Could not remember open folders: {e}"), cx); }
    }

    /// The Move to dialog: My documents on the left, its page tree on the right.
    /// Quiet selection fills, a 30pt search field, the modal buttons.
    pub(crate) fn render_move_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dlg = self.move_dlg.as_ref()?;
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
                        if let Some(d) = w.move_dlg.as_mut() {
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
                                            if let Some(d) = w.move_dlg.as_mut()
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
                            if let Some(d) = w.move_dlg.as_mut() {
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
                            if let Some(d) = w.move_dlg.as_mut() {
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
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().pt(px(24.)).pr(px(56.)).pl(px(24.)).text_size(px(15.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).child("Move from My documents to"))
            .child(close_button("move-close").absolute().top(px(27.)).right(px(25.)).on_click(cx.listener(|w, _, window, cx| w.close_move(window, cx))))
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
                    .child(secondary_button("move-cancel", "Cancel").on_click(cx.listener(|w, _, window, cx| w.close_move(window, cx))))
                    .child(ok.on_click(cx.listener(|w, _, window, cx| {
                        let Some(d) = w.move_dlg.as_ref() else { return };
                        let Some(target) = d.target.clone() else { return };
                        let ids = d.ids.clone();
                        w.batch = None;
                        w.move_docs(&ids, target, cx);
                        w.close_move(window, cx);
                    }))),
            );
        Some(
            deferred(
                div()
                    .id("move-mask")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(scrim())
                    .on_mouse_down(MouseButton::Left, cx.listener(|w, _, window, cx| w.close_move(window, cx)))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}
