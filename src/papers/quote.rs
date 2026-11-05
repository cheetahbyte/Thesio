//! Inserting a highlight into a document as a quotation.

use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use crate::library::paper_file;
use crate::native_pdf::Highlight;
use crate::overlay::Overlay;
use crate::source::SourceEditor;
use crate::theme::Theme;
use crate::trash::{dialog, primary_button, secondary_button};
use crate::ui::{ModalAt, ToastKind, field_well, modal};
use gpui::*;
use thesio_core::paths::is_document;
use thesio_core::quote::{clean_quote, page_offset, quote_markup};

/// The review step before a highlight enters a document: PDF text can be wrong.
pub struct QuoteDialog {
    highlight: Highlight,
    key: String,
    /// The page as first shown, to tell a correction from a confirmation.
    page: String,
    text_field: Entity<TextField>,
    page_field: Entity<TextField>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    /// The document a quote goes to: the open one, else this paper's notes.
    pub(crate) fn quote_target(&self) -> Option<(String, Entity<SourceEditor>)> {
        let notes = self.papers.panel.notes.as_ref();
        match (&self.current, &self.editor) {
            (Some(id), Some(editor)) if is_document(id) && Some(id) != notes.map(|notes| &notes.id) => Some((id.clone(), editor.clone())),
            _ => notes.map(|notes| (notes.id.clone(), notes.editor.clone())),
        }
    }

    pub(crate) fn open_quote(&mut self, highlight: Highlight, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.papers.current.clone() else { return };
        let row = self.citations.library.as_ref().and_then(|library| library.rows().into_iter().find(|row| paper_file(&row.key) == id));
        let Some(key) = row.map(|row| row.key) else {
            return self.show_toast_kind(ToastKind::Error, "Link this paper to a source to quote it", cx);
        };
        self.papers.pdf.release_keys();
        let page = self.highlight_page(&highlight);
        let text = clean_quote(&highlight.quote);
        let text_field = cx.new(|cx| {
            let mut f = TextField::new("Quotation", cx);
            f.wrap = true;
            f.multiline = true;
            f.set_text(&text, cx);
            f
        });
        let page_field = cx.new(|cx| {
            let mut f = TextField::new("None", cx);
            f.set_text(&page, cx);
            f
        });
        let on_event = |w: &mut Self, _: &Entity<TextField>, ev: &TextFieldEvent, window: &mut Window, cx: &mut Context<Self>| if let TextFieldEvent::Submit = ev { w.insert_quote(window, cx); };
        let subs = vec![cx.subscribe_in(&text_field, window, on_event), cx.subscribe_in(&page_field, window, on_event)];
        let focus = text_field.focus_handle(cx);
        self.open_overlay(Overlay::Quote(QuoteDialog { highlight, key, page, text_field, page_field, _subs: subs }), window, cx);
        window.focus(&focus);
    }

    pub(crate) fn insert_quote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Quote(dialog)) = &self.overlay else { return };
        let text = dialog.text_field.read(cx).text.clone();
        if text.trim().is_empty() { return; }
        let entered = dialog.page_field.read(cx).text.trim().to_string();
        let offset = (entered != dialog.page).then(|| page_offset(&dialog.highlight.label, dialog.highlight.page, &entered)).flatten();
        let (markup, block) = quote_markup(&text, &dialog.key, &entered);
        let Some((target, editor)) = self.quote_target() else { return };
        self.close_overlay(window, cx);
        if let (Some(offset), Some(id)) = (offset, self.papers.current.clone()) {
            let result = self.store.borrow_mut().set_page_offset(&id, offset);
            if let Err(e) = result {
                self.show_toast_kind(ToastKind::Error, &format!("Could not save the page numbering: {e}"), cx);
            }
            self.refresh_entries();
        }
        editor.update(cx, |e, cx| e.insert_quote(&markup, block, cx));
        if self.papers.panel.notes_editor(&target).is_some() {
            self.link_note_library(&target, &editor, cx);
        } else {
            self.citation_added(&target, cx);
        }
        let name = self.entries.iter().find(|e| e.id == target).map_or(target.clone(), |e| e.title.clone());
        self.show_toast(&format!("Added the quote to {name}"), cx);
        cx.notify();
    }

    pub(crate) fn render_quote_dialog(&self, quote: &QuoteDialog, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let target = self.quote_target().map(|(id, _)| id).and_then(|id| self.entries.iter().find(|e| e.id == id).map(|e| e.title.clone())).unwrap_or_else(|| "the notes".to_string());
        let panel = dialog("quote-dialog").w(px(480.)).p(px(20.)).flex().flex_col().gap(px(10.))
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Insert Quote"))
            .child(div().text_size(px(13.)).line_height(px(18.)).text_color(Theme::text_caption()).child(format!("Text read from a PDF can contain errors, so check it against the paper. It goes into {target}, cited as @{}.", quote.key)))
            .child(field_well(&quote.text_field, window, cx).id("quote-text").h(px(160.)).py(px(3.)).items_start().overflow_y_scroll())
            .child(div().flex().items_center().gap(px(10.))
                .child(div().text_size(px(13.)).child("Page"))
                .child(field_well(&quote.page_field, window, cx).w(px(96.)))
                .child(div().flex_1().text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption()).child("Correct a number once and the rest of this paper follows.")))
            .child(div().mt(px(8.)).flex().justify_end().gap(px(8.))
                .child(secondary_button("quote-cancel", "Cancel").on_click(cx.listener(|w, _, window, cx| w.close_overlay(window, cx))))
                .child(primary_button("quote-insert", "Insert").on_click(cx.listener(|w, _, window, cx| w.insert_quote(window, cx)))));
        modal("quote-scrim", ModalAt::Top, false, panel, cx)
    }
}
