//! The header "…" menu: move, pin, star, duplicate, export, and delete.

use crate::app::Workspace;
use thesio_core::bibliography::{CITATION_STYLES, Mode};
use crate::theme::{self, Theme};
use crate::ui::{Icon, popover};
use gpui::{prelude::FluentBuilder, *};

const MENU_W: f32 = 200.;
/// Gap between the "…" button and the menu.
const MENU_GAP: f32 = 6.;
const PAD: f32 = crate::ui::MENU_PAD_Y;

fn menu_item(id: &'static str, ic: Icon, label: impl Into<SharedString>) -> Stateful<Div> {
    crate::ui::menu_row(id)
        .child(crate::ui::menu_glyph(ic, 16.))
        .child(div().flex_1().min_w_0().overflow_hidden().child(crate::ui::menu_label(label)))
}

/// Native-menu separator: 1px, 5px either side, inset 13px.
fn divider() -> Div {
    div().flex_none().mx(px(13.)).my(px(5.)).h(px(1.)).bg(Theme::line_border())
}

impl Workspace {
    fn close_doc_menu(&mut self, cx: &mut Context<Self>) {
        self.doc_menu = false;
        cx.notify();
    }

    /// Duplicates a document beside the original and opens the copy.
    pub fn duplicate_doc(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_doc_menu(cx);
        self.row_menu = None;
        if let Some(new) = self.copy_doc(id, cx) {
            self.refresh_entries();
            self.open(&new, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn copy_doc(&mut self, id: &str, cx: &mut Context<Self>) -> Option<String> {
        let text = self.text_of(id, cx).ok()?;
        let entry = self.entries.iter().find(|e| e.id == id)?;
        let (title, parent) = (format!("{} copy", entry.title), entry.parent.clone());
        let result = self.store.borrow_mut().create_from(parent.as_deref(), &title, &text);
        match result {
            Ok(new) => Some(new),
            Err(e) => {
                self.show_toast_kind(crate::ui::ToastKind::Error, &format!("Could not duplicate document: {e}"), cx);
                None
            }
        }
    }

    /// Words by file, with the open file marked.
    pub(crate) fn render_word_count(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.words_open { return None; }
        let words = self.preview.as_ref()?.read(cx).words.clone();
        let total: usize = words.iter().map(|(_, words)| words).sum();
        let row = |name: String, count: usize, strong: bool| {
            div().h(px(28.)).flex().items_center().gap(px(16.)).text_size(px(13.))
                .when(strong, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(div().flex_1().min_w_0().truncate().child(crate::ui::ui_text(name)))
                .child(div().flex_none().text_color(Theme::text_caption()).child(crate::preview::format_count(count)))
        };
        let rows = words.iter().map(|(path, count)| row(path.clone(), *count, self.current.as_ref() == Some(path))).collect::<Vec<_>>();
        let panel = crate::trash::dialog("word-count").w(px(440.)).p(px(20.)).flex().flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().mb(px(10.)).text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Word Count"))
            .child(div().id("word-count-files").max_h(px(320.)).overflow_y_scroll().children(rows))
            .child(div().mt(px(6.)).pt(px(6.)).border_t_1().border_color(Theme::line_border()).child(row("Total".to_string(), total, true)))
            .child(div().mt(px(8.)).text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption()).child("Counts the text the document draws. The bibliography and page numbers are left out."))
            .child(div().mt(px(14.)).flex().justify_end().child(crate::trash::secondary_button("word-count-close", "Close").on_click(cx.listener(|w, _, _, cx| {
                w.words_open = false;
                cx.notify();
            }))));
        Some(deferred(div().id("word-count-scrim").absolute().inset_0().bg(crate::trash::scrim()).flex().items_center().justify_center().occlude().child(panel)).with_priority(3).into_any_element())
    }

    pub(crate) fn render_style_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.style_menu { return None; }
        let (_, current) = self.style_owner(cx)?;
        let rows = CITATION_STYLES.iter().enumerate().map(|(i, (label, name))| {
            crate::ui::menu_row(("style-row", i))
                .child(div().w(px(16.)).flex_none().when(*name == current, |d| d.child(crate::ui::menu_glyph(Icon::Check, 14.))))
                .child(crate::ui::menu_label(*label))
                .on_click(cx.listener(move |w, _, _, cx| w.set_citation_style(name, cx)))
        }).collect::<Vec<_>>();
        let menu = popover().id("style-menu").occlude().w(px(MENU_W + 40.)).py(px(PAD))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                w.style_menu = false;
                cx.notify();
            }))
            .children(rows);
        let btn = self.more_btn_bounds.get();
        let right = btn.map_or(window.viewport_size().width - px(120.), |b| b.right());
        let top = btn.map_or(px(60.), |b| b.bottom() + px(MENU_GAP));
        Some(deferred(anchored().anchor(Corner::TopRight).position(point(right, top)).child(menu)).with_priority(2).into_any_element())
    }

    pub(crate) fn render_doc_menu(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.doc_menu {
            return vec![];
        }
        let Some(id) = self.current.clone() else { return vec![] };
        let entry = self.entries.iter().find(|e| e.id == id);
        let pinned = entry.is_some_and(|e| e.meta.pinned);
        let starred = entry.is_some_and(|e| e.meta.starred > 0);
        let typst = entry.is_some_and(|e| e.typst);
        let mode = self.bibliography_mode(cx);
        let block_shown = self.editor.as_ref().is_some_and(|e| e.read(cx).block_shown);
        let owner = self.style_owner(cx);
        // A document has one bibliography, and a chapter's may be in its main file.
        let elsewhere = owner.as_ref().is_some_and(|(doc, _)| Some(doc) != self.current.as_ref());
        let style = owner.map(|(_, style)| style);
        let bibliography = |id: &'static str, ic: Icon, label: &'static str, target: Mode| {
            menu_item(id, ic, label).on_click(cx.listener(move |w, _, _, cx| {
                w.close_doc_menu(cx);
                w.set_bibliography(target, cx);
            }))
        };
        let (pin_id, star_id, mv_id, del_id, dup_id, pdf_id) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id);
        let menu = popover()
            .id("doc-menu")
            .occlude()
            .w(px(MENU_W))
            .py(px(PAD))
            .flex()
            .flex_col()
            .shadow(theme::shadow_s4_down())
            .on_mouse_down_out(cx.listener(|w, _: &MouseDownEvent, _, cx| w.close_doc_menu(cx)))
            .child(menu_item("dm-move", Icon::Move, "Move to").on_click(cx.listener(move |w, _, window, cx| {
                w.close_doc_menu(cx);
                w.open_move(vec![mv_id.clone()], window, cx);
            })))
            .child(menu_item("dm-pin", if pinned { Icon::Unpin } else { Icon::Pin }, if pinned { "Unpin" } else { "Pin" }).on_click(cx.listener(move |w, _, _, cx| {
                w.toggle_pin(&pin_id, cx);
                w.close_doc_menu(cx);
            })))
            .child(menu_item("dm-star", if starred { Icon::Unstar } else { Icon::Star }, if starred { "Remove from favorites" } else { "Add to favorites" })
                .on_click(cx.listener(move |w, _, _, cx| w.toggle_star(&star_id, cx))))
            .when(typst, |d| d.child(divider())
                .child(menu_item("dm-copy", Icon::Copy, "Duplicate").on_click(cx.listener(move |w, _, window, cx| w.duplicate_doc(&dup_id, window, cx))))
                .child(menu_item("dm-words", Icon::Notes, "Word Count").on_click(cx.listener(|w, _, _, cx| {
                    w.close_doc_menu(cx);
                    w.words_open = true;
                    cx.notify();
                })))
                .child(menu_item("dm-history", Icon::Restore, "Version History…").on_click(cx.listener(|w, _, _, cx| {
                    w.close_doc_menu(cx);
                    w.open_history(cx);
                })))
                .child(menu_item("dm-pdf", Icon::Export, "Export as PDF").on_click(cx.listener(move |w, _, _, cx| {
                    w.close_doc_menu(cx);
                    w.export_pdf(&pdf_id, cx);
                })))
                .when_some(style, |d, style| {
                    let label = CITATION_STYLES.iter().find(|(_, name)| *name == style).map_or(style.as_str(), |(label, _)| label);
                    d.child(divider()).child(menu_item("dm-style", Icon::Quote, format!("Citation Style: {label}")).on_click(cx.listener(|w, _, _, cx| {
                        w.doc_menu = false;
                        w.style_menu = true;
                        cx.notify();
                    })))
                })
                .when(mode != Mode::Custom, |d| d.child(divider())
                    .child(menu_item("dm-cite", Icon::Quote, "Insert citation").on_click(cx.listener(|w, _, window, cx| {
                        w.close_doc_menu(cx);
                        w.open_cite_picker(window, cx);
                    })))
                    .when(mode == Mode::Embedded, |d| d.child(menu_item("dm-block", if block_shown { Icon::Hide } else { Icon::Show }, if block_shown { "Hide bibliography data" } else { "Show bibliography data" }).on_click(cx.listener(|w, _, _, cx| {
                        w.close_doc_menu(cx);
                        if let Some(editor) = &w.editor { editor.update(cx, |e, cx| e.toggle_block(cx)); }
                    }))))
                    .when(mode != Mode::Embedded && !elsewhere, |d| d.child(bibliography("dm-embed", Icon::Library, "Embed bibliography", Mode::Embedded)))
                    .when(mode != Mode::Linked && !elsewhere, |d| d.child(bibliography("dm-link", Icon::Library, "Link library file", Mode::Linked)))
                    .when(mode != Mode::None, |d| d.child(bibliography("dm-unlink", Icon::Close, "Remove bibliography", Mode::None)))))
            .child(divider())
            .child(menu_item("dm-delete", Icon::Delete, "Delete").on_click(cx.listener(move |w, _, _, cx| w.ask_delete(&del_id, cx))));
        let vw = window.viewport_size().width;
        let btn = self.more_btn_bounds.get();
        let right = btn.map_or(vw - px(120.), |b| b.right());
        let top = btn.map_or(px(60.), |b| b.bottom() + px(MENU_GAP));
        vec![deferred(anchored().anchor(Corner::TopRight).position(point(right, top)).child(menu)).with_priority(2).into_any_element()]
    }
}
