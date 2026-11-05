//! The Paper view: the PDF, and a panel with the source's notes and highlights.
//! Highlights live in the PDF (`native_pdf`), notes in `notes/<key>.typ`.

use super::notes::Notes;
use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use crate::native_pdf::Highlight;
use crate::theme::Theme;
use crate::ui::{Icon, TipExt, TipSide, TitleBarExt, ToastKind, field_well, icon, page_header, toolbar_group, toolbar_icon, ui_para};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::quote::printed_page;

pub(crate) const PANEL_W: f32 = 340.;
const TAB_X: f32 = 16.;

#[derive(Clone, Copy, Default, PartialEq)]
pub enum PaperTab { #[default] Notes, Highlights }

/// The panel beside a paper.
pub struct PaperPanel {
    hidden: bool,
    tab: PaperTab,
    pub(crate) notes: Option<Notes>,
    /// The highlight whose comment is being written.
    pub(crate) comment: Option<(Highlight, Entity<TextField>, Subscription)>,
}

impl Default for PaperPanel {
    fn default() -> Self { Self { hidden: false, tab: PaperTab::Notes, notes: None, comment: None } }
}

impl Workspace {
    pub(crate) fn highlight_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = crate::devtools::take_pdf_select() { self.papers.pdf.select_text(&text); }
        match self.papers.pdf.highlight_selection() {
            Ok(true) => {
                self.papers.panel.tab = PaperTab::Highlights;
                self.papers.panel.hidden = false;
            }
            Ok(false) => self.show_toast_kind(ToastKind::Error, "Select some text to highlight", cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not save the highlight: {e}"), cx),
        }
        cx.notify();
    }

    pub(crate) fn edit_comment(&mut self, highlight: Highlight, window: &mut Window, cx: &mut Context<Self>) {
        self.papers.pdf.release_keys();
        let field = cx.new(|cx| {
            let mut f = TextField::new("Add a comment", cx);
            f.placeholder_color = Theme::text_disabled;
            f.set_text(&highlight.comment, cx);
            f
        });
        let target = highlight.clone();
        let subscription = cx.subscribe_in(&field, window, move |w, field, event: &TextFieldEvent, _, cx| match event {
            TextFieldEvent::Submit => {
                let comment = field.read(cx).text().trim().to_string();
                w.papers.panel.comment = None;
                if let Err(e) = w.papers.pdf.set_comment(&target, &comment) { w.show_toast_kind(ToastKind::Error, &format!("Could not save the comment: {e}"), cx); }
                cx.notify();
            }
            TextFieldEvent::Cancel => {
                w.papers.panel.comment = None;
                cx.notify();
            }
            _ => {}
        });
        window.focus(&field.focus_handle(cx));
        self.papers.panel.comment = Some((highlight, field, subscription));
        cx.notify();
    }

    pub(crate) fn highlight_page(&self, highlight: &Highlight) -> String {
        let offset = self.papers.current.as_ref().and_then(|id| self.entries.iter().find(|e| e.id == *id)).and_then(|e| e.meta.page_offset);
        printed_page(&highlight.label, highlight.page, offset.unwrap_or(0))
    }

    pub(crate) fn render_highlight(&self, index: usize, highlight: Highlight, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.papers.panel.comment.as_ref().filter(|(target, _, _)| *target == highlight).map(|(_, field, _)| field.clone());
        let (open, edit, delete, quote) = (highlight.clone(), highlight.clone(), highlight.clone(), highlight.clone());
        let page = match self.highlight_page(&highlight) {
            page if page.is_empty() => "Before the first page".to_string(),
            page => format!("Page {page}"),
        };
        let group = SharedString::from(format!("highlight-{index}"));
        div().id(("highlight", index)).group(group.clone()).px(px(10.)).py(px(8.)).flex().flex_col().gap(px(4.)).rounded(px(8.)).cursor_pointer()
            .bg(Theme::fill_group())
            .on_click(cx.listener(move |w, _, _, _| w.papers.pdf.go_to(&open)))
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
                        if let Err(e) = w.papers.pdf.remove(&delete) { w.show_toast_kind(ToastKind::Error, &format!("Could not save the paper: {e}"), cx); }
                        w.papers.panel.comment = None;
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

    pub(crate) fn render_paper_panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.papers.panel.tab;
        let highlights = self.papers.pdf.highlights();
        let label: &'static str = "Highlights";
        let tabs = crate::ui::segmented("paper-tab", &["Notes", label], usize::from(tab == PaperTab::Highlights), |w, i, cx| {
            w.papers.panel.tab = if i == 0 { PaperTab::Notes } else { PaperTab::Highlights };
            cx.notify();
        }, cx);
        let empty = highlights.is_empty();
        let rows = highlights.into_iter().enumerate().map(|(i, highlight)| self.render_highlight(i, highlight, window, cx)).collect::<Vec<_>>();
        let body = match tab {
            PaperTab::Notes => div().flex_1().min_h_0().children(self.papers.panel.notes.as_ref().map(|notes| notes.editor.clone())).into_any_element(),
            PaperTab::Highlights if empty => div().flex_1().flex().flex_col().items_center().justify_center().px(px(32.)).pb(px(64.))
                .child(icon(Icon::Highlight, 32., Theme::text_placeholder()))
                .child(div().mt(px(10.)).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("No highlights yet"))
                .child(div().mt(px(2.)).text_size(px(12.)).line_height(px(17.)).text_color(Theme::text_caption()).child("Select text in the paper, then press the highlighter."))
                .into_any_element(),
            PaperTab::Highlights => div().id("paper-highlights").flex_1().min_h_0().overflow_y_scroll().px(px(TAB_X)).pb(px(16.)).flex().flex_col().gap(px(8.)).children(rows).into_any_element(),
        };
        div().w(px(PANEL_W)).flex_none().h_full().flex().flex_col().border_l_1().border_color(Theme::line_divider()).bg(Theme::bg_body())
            .on_mouse_down(MouseButton::Left, cx.listener(|w, _, _, _| w.papers.pdf.release_keys()))
            .child(div().flex_none().px(px(TAB_X)).pt(px(4.)).pb(px(12.)).flex().justify_center().child(tabs))
            .child(body)
            .into_any_element()
    }

    pub(crate) fn render_paper(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(id) = self.papers.current.clone() else { return div().into_any_element() };
        let title = self.paper_title(&id);
        let external = id.clone();
        let hidden = self.papers.panel.hidden;
        let header = page_header(self, &title, None, cx)
            .child(toolbar_group()
                .child(toolbar_icon("paper-highlight", Icon::Highlight).no_drag().tip_label("Highlight Selection".to_string(), TipSide::Right)
                    .on_click(cx.listener(|w, _, _, cx| w.highlight_selection(cx))))
                .child(toolbar_icon("paper-panel", Icon::PanelToggle).no_drag().when(!hidden, |d| d.bg(Theme::fill_hover_strong())).tip_label("Notes and Highlights".to_string(), TipSide::Right)
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.papers.panel.hidden = !w.papers.panel.hidden;
                        cx.notify();
                    })))
                .child(toolbar_icon("paper-external", Icon::External).no_drag().tip_label("Open in Default App".to_string(), TipSide::Right)
                    .on_click(cx.listener(move |w, _, _, cx| w.open_paper_externally(&external, cx)))));
        let (pdf, covered, path) = (self.papers.pdf.clone(), self.paper_covered(window), self.store.borrow().path(&id).ok());
        let failed = self.papers.failed.clone();
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
