//! The Paper view: the PDF, and a panel with the source's notes and highlights.
//! Highlights live in the PDF (`native_pdf`), notes in `notes/<key>.typ`.

use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use crate::library::{self, Mode, NOTES_DIR, note_file, paper_file};
use crate::native_pdf::Highlight;
use crate::source::{SourceEditor, SourceEvent};
use crate::theme::Theme;
use crate::trash::page_header;
use crate::ui::{Icon, TipExt, TipSide, TitleBarExt, ToastKind, field_well, icon, toolbar_group, toolbar_icon, ui_para};
use gpui::{prelude::FluentBuilder, *};

const PANEL_W: f32 = 340.;
const TAB_X: f32 = 16.;

#[derive(Clone, Copy, Default, PartialEq)]
pub enum PaperTab { #[default] Notes, Highlights }

/// The panel beside a paper.
pub struct PaperPanel {
    hidden: bool,
    tab: PaperTab,
    notes: Option<(Entity<SourceEditor>, Subscription)>,
    /// The highlight whose comment is being written.
    comment: Option<(Highlight, Entity<TextField>, Subscription)>,
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
        let text = self.store.load(&note).unwrap_or_else(|_| format!("= {}\n\n", self.paper_title(id)));
        let editor = cx.new(|cx| {
            let mut editor = SourceEditor::new(text, true, cx);
            // Inside the panel: the viewer would hide what crosses onto it.
            editor.completion_w = PANEL_W - 40.;
            editor
        });
        let subscription = cx.subscribe(&editor, move |w, editor, event: &SourceEvent, cx| match event {
            SourceEvent::Changed => w.save_note(&note, &editor, cx),
            SourceEvent::Citing => w.offer_sources(&editor, cx),
            SourceEvent::Cited => w.link_note_library(&note, &editor, cx),
        });
        self.paper_panel.notes = Some((editor, subscription));
        self.paper_panel.comment = None;
    }

    fn save_note(&mut self, id: &str, editor: &Entity<SourceEditor>, cx: &mut Context<Self>) {
        let text = editor.read(cx).buffer.text().to_string();
        let new = !self.entries.iter().any(|e| e.id == id);
        if let Err(e) = self.ensure_notes_dir().and_then(|()| self.store.save(id, &text)) {
            return self.show_toast_kind(ToastKind::Error, &format!("Could not save the notes: {e}"), cx);
        }
        if new {
            self.entries = self.store.list();
            self.sort();
        }
        // The same file may be open as a document too.
        if self.current.as_deref() == Some(id) && let Some(open) = self.editor.clone() { open.update(cx, |e, cx| e.sync_text(&text, cx)); }
    }

    /// Notes that cite the library read it directly, so they compile on their own.
    fn link_note_library(&mut self, id: &str, editor: &Entity<SourceEditor>, cx: &mut Context<Self>) {
        let text = editor.read(cx).buffer.text().to_string();
        if library::mode(&text, id) != Mode::None { return; }
        let linked = library::link(&text, id);
        editor.update(cx, |e, cx| e.sync_text(&linked, cx));
        self.save_note(id, editor, cx);
    }

    fn ensure_notes_dir(&self) -> anyhow::Result<()> {
        Ok(std::fs::create_dir_all(self.store.root().join(NOTES_DIR))?)
    }

    /// Opens the notes of `key` as a document, starting them if there are none.
    pub(crate) fn open_source_notes(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let id = note_file(&paper_file(key));
        if !self.entries.iter().any(|e| e.id == id) {
            let title = self.paper_title(&paper_file(key));
            if let Err(e) = self.ensure_notes_dir().and_then(|()| self.store.save(&id, &format!("= {title}\n\n"))) {
                return self.show_toast_kind(ToastKind::Error, &format!("Could not create the notes: {e}"), cx);
            }
            self.entries = self.store.list();
            self.sort();
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

    fn render_highlight(&self, index: usize, highlight: Highlight, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.paper_panel.comment.as_ref().filter(|(target, _, _)| *target == highlight).map(|(_, field, _)| field.clone());
        let (open, edit, delete) = (highlight.clone(), highlight.clone(), highlight.clone());
        let page = format!("Page {}", highlight.page + 1);
        let group = SharedString::from(format!("highlight-{index}"));
        div().id(("highlight", index)).group(group.clone()).px(px(10.)).py(px(8.)).flex().flex_col().gap(px(4.)).rounded(px(8.)).cursor_pointer()
            .bg(Theme::fill_group())
            .on_click(cx.listener(move |w, _, _, _| w.pdf.go_to(&open)))
            .child(div().flex().items_center().text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption())
                .child(div().flex_1().child(page))
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
            PaperTab::Notes => div().flex_1().min_h_0().children(self.paper_panel.notes.as_ref().map(|(editor, _)| editor.clone())).into_any_element(),
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
        let (pdf, covered, path) = (self.pdf.clone(), self.paper_covered(window), self.store.path(&id).ok());
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
