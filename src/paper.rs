//! The Paper view: the PDF, and a panel with the source's notes and highlights.
//! Highlights live in the PDF (`native_pdf`), notes in `notes/<key>.typ`.

use crate::app::Workspace;
use crate::document::Edit;
use crate::input::{TextField, TextFieldEvent};
use crate::library::{NOTES_DIR, note_file, paper_file};
use thesio_core::bibliography::{self, Mode};
use thesio_core::paths::{is_document, moved};
use crate::native_pdf::Highlight;
use crate::source::{SourceEditor, SourceEvent};
use crate::theme::Theme;
use crate::overlay::Overlay;
use crate::trash::{dialog, page_header, primary_button, secondary_button};
use crate::ui::{Icon, ModalAt, TipExt, TipSide, TitleBarExt, ToastKind, field_well, icon, modal, toolbar_group, toolbar_icon, ui_para};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::quote::{clean_quote, page_offset, printed_page, quote_markup};

const PANEL_W: f32 = 340.;
const TAB_X: f32 = 16.;

#[derive(Clone, Copy, Default, PartialEq)]
pub enum PaperTab { #[default] Notes, Highlights }

/// The panel beside a paper.
pub struct PaperPanel {
    hidden: bool,
    tab: PaperTab,
    notes: Option<Notes>,
    /// The highlight whose comment is being written.
    comment: Option<(Highlight, Entity<TextField>, Subscription)>,
}

/// The paper's notes: `id` follows the file, which appears once something is written.
struct Notes {
    id: String,
    editor: Entity<SourceEditor>,
    _subscription: Subscription,
}

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

impl PaperPanel {
    pub(crate) fn notes_editor(&self, id: &str) -> Option<Entity<SourceEditor>> {
        self.notes.as_ref().filter(|notes| notes.id == id).map(|notes| notes.editor.clone())
    }

    pub(crate) fn path_changed(&mut self, old: &str, new: &str) {
        if let Some(notes) = &mut self.notes && let Some(path) = moved(&notes.id, old, new) { notes.id = path; }
    }
}

impl Default for PaperPanel {
    fn default() -> Self { Self { hidden: false, tab: PaperTab::Notes, notes: None, comment: None } }
}

impl Workspace {
    /// Loads the notes for paper `id`, or a heading to start them with. The
    /// file only appears once something is written.
    pub(crate) fn load_paper_notes(&mut self, id: &str, cx: &mut Context<Self>) {
        self.flush(cx);
        let note = note_file(id);
        let exists = self.store.borrow().path(&note).is_ok_and(|path| path.exists());
        let loaded = self.store.borrow().load(&note);
        let text = match loaded {
            Ok(text) => text,
            Err(_) if !exists => format!("= {}\n\n", self.paper_title(id)),
            // No editor: the next keystroke would replace a file nobody could read.
            Err(e) => {
                self.paper_panel.notes = None;
                return self.show_toast_kind(ToastKind::Error, &format!("Could not read the notes: {e}"), cx);
            }
        };
        let editor = cx.new(|cx| {
            let mut editor = SourceEditor::new(text, true, cx);
            // Inside the panel: the viewer would hide what crosses onto it.
            editor.completion_w = PANEL_W - 40.;
            editor
        });
        let subscription = cx.subscribe(&editor, |w, editor, event: &SourceEvent, cx| {
            let Some(note) = w.paper_panel.notes.as_ref().map(|notes| notes.id.clone()) else { return };
            match event {
                SourceEvent::Changed => { let text = editor.read(cx).buffer.text().to_string(); w.save_note(&note, &text, cx); }
                SourceEvent::Citing => w.offer_sources(&editor, cx),
                SourceEvent::Cited => w.link_note_library(&note, &editor, cx),
                SourceEvent::Caret | SourceEvent::Follow(_) => {}
            }
        });
        self.paper_panel.notes = Some(Notes { id: note, editor, _subscription: subscription });
        self.paper_panel.comment = None;
    }

    /// Writes a notes file, which may be new. The same file may be open as a document too.
    fn save_note(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
        let new = !self.entries.iter().any(|e| e.id == id);
        if let Err(e) = self.ensure_notes_dir().and_then(|()| self.write_document(id, text, Edit::Generated, cx)) {
            return self.show_toast_kind(ToastKind::Error, &format!("Could not save the notes: {e}"), cx);
        }
        if new { self.refresh_entries(); }
    }

    /// Notes that cite the library read it directly, so they compile on their own.
    fn link_note_library(&mut self, id: &str, editor: &Entity<SourceEditor>, cx: &mut Context<Self>) {
        let text = editor.read(cx).buffer.text().to_string();
        if bibliography::mode(&text, id) != Mode::None { return; }
        self.save_note(id, &bibliography::link(&text, id), cx);
    }

    fn ensure_notes_dir(&self) -> anyhow::Result<()> {
        Ok(std::fs::create_dir_all(self.store.borrow().path(NOTES_DIR)?)?)
    }

    /// Opens the notes of `key` as a document, starting them if there are none.
    pub(crate) fn open_source_notes(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let id = note_file(&paper_file(key));
        if !self.entries.iter().any(|e| e.id == id) {
            let title = self.paper_title(&paper_file(key));
            let text = format!("= {title}\n\n");
            if let Err(e) = self.ensure_notes_dir().and_then(|()| self.write_document(&id, &text, Edit::Generated, cx)) {
                return self.show_toast_kind(ToastKind::Error, &format!("Could not create the notes: {e}"), cx);
            }
            self.refresh_entries();
        }
        self.open(&id, window, cx);
    }

    fn highlight_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = crate::devtools::take_pdf_select() { self.pdf.select_text(&text); }
        match self.pdf.highlight_selection() {
            Ok(true) => {
                self.paper_panel.tab = PaperTab::Highlights;
                self.paper_panel.hidden = false;
            }
            Ok(false) => self.show_toast_kind(ToastKind::Error, "Select some text to highlight", cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not save the highlight: {e}"), cx),
        }
        cx.notify();
    }

    fn edit_comment(&mut self, highlight: Highlight, window: &mut Window, cx: &mut Context<Self>) {
        self.pdf.release_keys();
        let field = cx.new(|cx| {
            let mut f = TextField::new("Add a comment", cx);
            f.placeholder_color = Theme::text_disabled();
            f.set_text(&highlight.comment, cx);
            f
        });
        let target = highlight.clone();
        let subscription = cx.subscribe_in(&field, window, move |w, field, event: &TextFieldEvent, _, cx| match event {
            TextFieldEvent::Submit => {
                let comment = field.read(cx).text.trim().to_string();
                w.paper_panel.comment = None;
                if let Err(e) = w.pdf.set_comment(&target, &comment) { w.show_toast_kind(ToastKind::Error, &format!("Could not save the comment: {e}"), cx); }
                cx.notify();
            }
            TextFieldEvent::Cancel => {
                w.paper_panel.comment = None;
                cx.notify();
            }
            _ => {}
        });
        window.focus(&field.focus_handle(cx));
        self.paper_panel.comment = Some((highlight, field, subscription));
        cx.notify();
    }

    fn highlight_page(&self, highlight: &Highlight) -> String {
        let offset = self.paper.as_ref().and_then(|id| self.entries.iter().find(|e| e.id == *id)).and_then(|e| e.meta.page_offset);
        printed_page(&highlight.label, highlight.page, offset.unwrap_or(0))
    }

    /// The document a quote goes to: the open one, else this paper's notes.
    fn quote_target(&self) -> Option<(String, Entity<SourceEditor>)> {
        let notes = self.paper_panel.notes.as_ref();
        match (&self.current, &self.editor) {
            (Some(id), Some(editor)) if is_document(id) && Some(id) != notes.map(|notes| &notes.id) => Some((id.clone(), editor.clone())),
            _ => notes.map(|notes| (notes.id.clone(), notes.editor.clone())),
        }
    }

    fn open_quote(&mut self, highlight: Highlight, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.paper.clone() else { return };
        let row = self.citations.library.as_ref().and_then(|library| library.rows().into_iter().find(|row| paper_file(&row.key) == id));
        let Some(key) = row.map(|row| row.key) else {
            return self.show_toast_kind(ToastKind::Error, "Link this paper to a source to quote it", cx);
        };
        self.pdf.release_keys();
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

    fn insert_quote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Quote(dialog)) = &self.overlay else { return };
        let text = dialog.text_field.read(cx).text.clone();
        if text.trim().is_empty() { return; }
        let entered = dialog.page_field.read(cx).text.trim().to_string();
        let offset = (entered != dialog.page).then(|| page_offset(&dialog.highlight.label, dialog.highlight.page, &entered)).flatten();
        let (markup, block) = quote_markup(&text, &dialog.key, &entered);
        let Some((target, editor)) = self.quote_target() else { return };
        self.close_overlay(window, cx);
        if let (Some(offset), Some(id)) = (offset, self.paper.clone()) {
            let result = self.store.borrow_mut().set_page_offset(&id, offset);
            if let Err(e) = result {
                self.show_toast_kind(ToastKind::Error, &format!("Could not save the page numbering: {e}"), cx);
            }
            self.refresh_entries();
        }
        editor.update(cx, |e, cx| e.insert_quote(&markup, block, cx));
        if self.paper_panel.notes_editor(&target).is_some() {
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

    fn render_highlight(&self, index: usize, highlight: Highlight, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.paper_panel.comment.as_ref().filter(|(target, _, _)| *target == highlight).map(|(_, field, _)| field.clone());
        let (open, edit, delete, quote) = (highlight.clone(), highlight.clone(), highlight.clone(), highlight.clone());
        let page = match self.highlight_page(&highlight) {
            page if page.is_empty() => "Before the first page".to_string(),
            page => format!("Page {page}"),
        };
        let group = SharedString::from(format!("highlight-{index}"));
        div().id(("highlight", index)).group(group.clone()).px(px(10.)).py(px(8.)).flex().flex_col().gap(px(4.)).rounded(px(8.)).cursor_pointer()
            .bg(Theme::fill_group())
            .on_click(cx.listener(move |w, _, _, _| w.pdf.go_to(&open)))
            .child(div().flex().items_center().text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption())
                .child(div().flex_1().child(page))
                .child(div().id(("highlight-quote", index)).px(px(4.)).text_color(Theme::text_code()).child("Insert as Quote")
                    .on_click(cx.listener(move |w, _, window, cx| {
                        cx.stop_propagation();
                        w.open_quote(quote.clone(), window, cx);
                    })))
                .child(div().id(("highlight-delete", index)).size(px(20.)).flex().items_center().justify_center().rounded(px(5.))
                    .opacity(0.).group_hover(group, |s| s.opacity(1.))
                    .child(icon(Icon::Delete, 13., Theme::icon()))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        cx.stop_propagation();
                        if let Err(e) = w.pdf.remove(&delete) { w.show_toast_kind(ToastKind::Error, &format!("Could not save the paper: {e}"), cx); }
                        w.paper_panel.comment = None;
                        cx.notify();
                    }))))
            .child(div().text_size(px(13.)).line_height(px(18.)).child(ui_para(highlight.quote.clone())))
            .map(|d| match editing {
                Some(field) => d.child(field_well(&field, window, cx).mt(px(2.))),
                None => d.child(div().id(("highlight-comment", index)).text_size(px(12.)).line_height(px(17.))
                    .text_color(if highlight.comment.is_empty() { Theme::primary() } else { Theme::text_caption() })
                    .child(ui_para(if highlight.comment.is_empty() { "Add a comment".to_string() } else { highlight.comment.clone() }))
                    .on_click(cx.listener(move |w, _, window, cx| {
                        cx.stop_propagation();
                        w.edit_comment(edit.clone(), window, cx);
                    }))),
            })
            .into_any_element()
    }

    fn render_paper_panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.paper_panel.tab;
        let highlights = self.pdf.highlights();
        let label: &'static str = "Highlights";
        let tabs = crate::settings::segmented("paper-tab", &["Notes", label], usize::from(tab == PaperTab::Highlights), |w, i, cx| {
            w.paper_panel.tab = if i == 0 { PaperTab::Notes } else { PaperTab::Highlights };
            cx.notify();
        }, cx);
        let empty = highlights.is_empty();
        let rows = highlights.into_iter().enumerate().map(|(i, highlight)| self.render_highlight(i, highlight, window, cx)).collect::<Vec<_>>();
        let body = match tab {
            PaperTab::Notes => div().flex_1().min_h_0().children(self.paper_panel.notes.as_ref().map(|notes| notes.editor.clone())).into_any_element(),
            PaperTab::Highlights if empty => div().flex_1().flex().flex_col().items_center().justify_center().px(px(32.)).pb(px(64.))
                .child(icon(Icon::Highlight, 32., Theme::text_placeholder()))
                .child(div().mt(px(10.)).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("No highlights yet"))
                .child(div().mt(px(2.)).text_size(px(12.)).line_height(px(17.)).text_color(Theme::text_caption()).child("Select text in the paper, then press the highlighter."))
                .into_any_element(),
            PaperTab::Highlights => div().id("paper-highlights").flex_1().min_h_0().overflow_y_scroll().px(px(TAB_X)).pb(px(16.)).flex().flex_col().gap(px(8.)).children(rows).into_any_element(),
        };
        div().w(px(PANEL_W)).flex_none().h_full().flex().flex_col().border_l_1().border_color(Theme::line_divider()).bg(Theme::bg_body())
            .on_mouse_down(MouseButton::Left, cx.listener(|w, _, _, _| w.pdf.release_keys()))
            .child(div().flex_none().px(px(TAB_X)).pt(px(4.)).pb(px(12.)).flex().justify_center().child(tabs))
            .child(body)
            .into_any_element()
    }

    pub(crate) fn render_paper(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(id) = self.paper.clone() else { return div().into_any_element() };
        let title = self.paper_title(&id);
        let external = id.clone();
        let hidden = self.paper_panel.hidden;
        let header = page_header(self, &title, None, 26., cx)
            .child(toolbar_group()
                .child(toolbar_icon("paper-highlight", Icon::Highlight).no_drag().tip_label("Highlight Selection".to_string(), TipSide::Right)
                    .on_click(cx.listener(|w, _, _, cx| w.highlight_selection(cx))))
                .child(toolbar_icon("paper-panel", Icon::PanelToggle).no_drag().when(!hidden, |d| d.bg(Theme::fill_hover_strong())).tip_label("Notes and Highlights".to_string(), TipSide::Right)
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.paper_panel.hidden = !w.paper_panel.hidden;
                        cx.notify();
                    })))
                .child(toolbar_icon("paper-external", Icon::External).no_drag().tip_label("Open in Default App".to_string(), TipSide::Right)
                    .on_click(cx.listener(move |w, _, _, cx| w.open_paper_externally(&external, cx)))));
        let (pdf, covered, path) = (self.pdf.clone(), self.paper_covered(window), self.store.borrow().path(&id).ok());
        let failed = self.paper_failed.clone();
        let unreadable = failed.get();
        let pane = canvas(|_, _, _| (), move |bounds, _, window, _| {
            let shown = path.as_ref().is_some_and(|path| pdf.show(path, bounds, covered, window));
            let opened = pdf.take_opened();
            if opened || !shown && !failed.replace(true) { window.on_next_frame(|window, _| window.refresh()); }
        }).absolute().size_full();
        let panel = (!hidden).then(|| self.render_paper_panel(window, cx));
        div().flex_1().min_w_0().h_full().flex().flex_col().child(header)
            .child(div().flex_1().min_h_0().flex()
                .child(div().relative().flex_1().min_w_0().h_full().flex().items_center().justify_center().bg(Theme::bg_overlay())
                    .child(pane)
                    .when(unreadable, |d| d.child(div().px(px(24.)).text_size(px(13.)).text_color(Theme::text_caption()).child("This PDF could not be shown here. Try opening it in your default app."))))
                .children(panel))
            .into_any_element()
    }
}
