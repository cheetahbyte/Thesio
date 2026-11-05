//! The Version History dialog: earlier versions of the open document or the library, what
//! changed since each, and restoring one.

use crate::app::Workspace;
use crate::document::Edit;
use thesio_core::library::Library;
use crate::theme::{self, Theme, craft};
use crate::overlay::{Overlay, modal};
use crate::ui::{ModalAt, ToastKind, dialog, format_visit, list_line, primary_button, secondary_button, ui_para, ui_text};
use anyhow::Context as _;
use gpui::{prelude::FluentBuilder, *};
use thesio_core::diff::{DiffLine, LineKind, diff};
use thesio_core::history::Versions;
use thesio_core::paths::base_name;

const WIDTH: f32 = 960.;
const HEIGHT: f32 = 620.;
const LIST_W: f32 = 220.;
const LINE_H: f32 = 19.;
const SIGN_W: f32 = 16.;

/// What a History dialog is about.
enum Target {
    Document(String),
    Library,
}

pub struct History {
    target: Target,
    versions: Vec<u64>,
    selected: usize,
    diff: Vec<DiffLine>,
    list: ListState,
}

impl Workspace {
    /// Keeps the open text as a version; while writing, at most every few minutes.
    pub(crate) fn keep_version(&mut self, id: &str, text: &str, now: bool, cx: &mut Context<Self>) {
        let kept = self.store.borrow().versions(id).and_then(|versions| if now { versions.keep(text) } else { versions.keep_paced(text) });
        if let Err(e) = kept { self.show_toast_kind(ToastKind::Error, &format!("Could not keep a version of {id}: {e}"), cx); }
    }

    pub(crate) fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.current().map(str::to_string) else { return };
        if self.flush(cx) { self.open_versions(Target::Document(id), window, cx); }
    }

    pub(crate) fn open_library_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_inspector(cx);
        if self.citations.library.is_some() { self.open_versions(Target::Library, window, cx); }
    }

    fn open_versions(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let versions = self.versions_of(&target).map_or(vec![], |versions| versions.list());
        self.open_overlay(Overlay::History(History { target, versions, selected: 0, diff: vec![], list: ListState::new(0, ListAlignment::Top, px(400.)) }), window, cx);
        self.show_version(0, cx);
    }

    fn versions_of(&self, target: &Target) -> anyhow::Result<Versions> {
        match target {
            Target::Document(id) => self.store.borrow().versions(id),
            Target::Library => self.citations.library.as_ref().map(Library::versions).context("The library could not be read"),
        }
    }

    fn history(&self) -> Option<&History> {
        match &self.overlay {
            Some(Overlay::History(history)) => Some(history),
            _ => None,
        }
    }

    fn history_mut(&mut self) -> Option<&mut History> {
        match &mut self.overlay {
            Some(Overlay::History(history)) => Some(history),
            _ => None,
        }
    }

    /// What the dialog compares against.
    fn history_text(&self, cx: &App) -> Option<String> {
        match &self.history()?.target {
            Target::Document(id) => self.text_of(id, cx).ok(),
            Target::Library => self.citations.library.as_ref().map(Library::text),
        }
    }

    fn version_text(&self, time: u64) -> anyhow::Result<String> {
        self.versions_of(&self.history().context("No history is open")?.target)?.read(time)
    }

    fn show_version(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(current) = self.history_text(cx) else { return };
        let Some(history) = self.history_mut() else { return };
        let Some(time) = history.versions.get(index).copied() else { return };
        history.selected = index;
        match self.version_text(time) {
            Ok(text) => {
                // Read as: what restoring this version would change.
                let lines = diff(&current, &text);
                if let Some(history) = self.history_mut() {
                    history.list.reset(lines.len());
                    history.diff = lines;
                }
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &e.to_string(), cx),
        }
        cx.notify();
    }

    /// Brings a version back. What it replaces becomes a version first, and
    /// the editor can undo it.
    fn restore_version(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(history) = self.history() else { return };
        let Some(time) = history.versions.get(history.selected).copied() else { return };
        let document = match &history.target {
            Target::Document(id) => Some(id.clone()),
            Target::Library => None,
        };
        let text = match self.version_text(time) {
            Ok(text) => text,
            Err(e) => return self.show_toast_kind(ToastKind::Error, &e.to_string(), cx),
        };
        match document {
            None => {
                if let Err(e) = self.citations.library.as_mut().map_or(Ok(()), |library| library.restore(&text)) {
                    return self.show_toast_kind(ToastKind::Error, &format!("Could not restore the library: {e}"), cx);
                }
                self.library_restored(&text, window, cx);
            }
            Some(id) => {
                let Some(current) = self.history_text(cx) else { return };
                self.keep_version(&id, &current, true, cx);
                if let Err(e) = self.write_document(&id, &text, Edit::Undoable, cx) {
                    return self.show_toast_kind(ToastKind::Error, &format!("Could not save {id}: {e}"), cx);
                }
            }
        }
        self.close_overlay(window, cx);
        self.show_toast(&format!("Restored the version from {}", format_visit(time)), cx);
        cx.notify();
    }

    pub(crate) fn render_history(&self, history: &History, cx: &mut Context<Self>) -> AnyElement {
        let rows = history.versions.iter().enumerate().map(|(i, time)| {
            let on = i == history.selected;
            div().id(("version", i)).h(px(30.)).mx(px(8.)).px(px(10.)).flex().items_center().rounded(px(crate::ui::ROW_RADIUS)).cursor_pointer().text_size(px(13.))
                .when(on, |d| d.bg(Theme::sidebar_selected()))
                .child(format_visit(*time))
                .on_click(cx.listener(move |w, _, _, cx| w.show_version(i, cx)))
        }).collect::<Vec<_>>();
        let list = div().id("version-list").w(px(LIST_W)).flex_none().h_full().overflow_y_scroll().py(px(8.)).border_r_1().border_color(list_line()).children(rows);
        let body = if history.versions.is_empty() {
            div().flex_1().min_w_0().p(px(16.)).text_size(px(13.)).text_color(Theme::text_caption()).child("Versions appear here as you write.").into_any_element()
        } else if history.diff.is_empty() {
            div().flex_1().min_w_0().p(px(16.)).text_size(px(13.)).text_color(Theme::text_caption()).child("This version is the same as the document now.").into_any_element()
        } else {
            let lines = history.diff.clone();
            let changes = gpui::list(history.list.clone(), cx.processor(move |_: &mut Workspace, i: usize, _, _| {
                let line = &lines[i];
                if line.kind == LineKind::Gap {
                    return div().h(px(LINE_H)).pl(px(16. + SIGN_W)).font_family(theme::MONO_FONT).text_size(px(12.)).text_color(Theme::text_placeholder()).child("…").into_any_element();
                }
                let (bg, sign) = match line.kind {
                    LineKind::Added => (Some(theme::ca(craft::G500, 0.12)), "+"),
                    LineKind::Removed => (Some(theme::ca(craft::R500, 0.10)), "-"),
                    _ => (None, ""),
                };
                div().w_full().px(px(16.)).flex().items_start().font_family(theme::MONO_FONT).text_size(px(12.)).line_height(px(LINE_H))
                    .when_some(bg, |d, bg| d.bg(bg))
                    .child(div().w(px(SIGN_W)).flex_none().text_color(Theme::text_caption()).child(sign))
                    .child(div().flex_1().min_w_0().min_h(px(LINE_H)).child(ui_para(line.text.clone())))
                    .into_any_element()
            })).flex_1().min_h_0();
            div().flex_1().min_w_0().flex().flex_col().py(px(8.)).child(changes).into_any_element()
        };
        let title = match &history.target {
            Target::Document(id) => base_name(id),
            Target::Library => "the Library",
        };
        let can_restore = !history.diff.is_empty();
        let panel = dialog("history").w(px(WIDTH)).h(px(HEIGHT)).flex().flex_col()
            .child(div().flex_none().px(px(20.)).py(px(14.)).border_b_1().border_color(list_line()).flex().flex_col().gap(px(2.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(ui_text(format!("Version History of {title}"))))
                .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child("Green is what restoring brings back. Red is what it replaces.")))
            .child(div().flex_1().min_h_0().flex().child(list).child(body))
            .child(div().flex_none().px(px(20.)).py(px(12.)).border_t_1().border_color(list_line()).flex().items_center().justify_end().gap(px(8.))
                .child(secondary_button("history-close", "Close").on_click(cx.listener(|w, _, window, cx| w.close_overlay(window, cx))))
                .child(primary_button("history-restore", "Restore This Version").when(!can_restore, |d| d.opacity(0.5))
                    .on_click(cx.listener(move |w, _, window, cx| if can_restore { w.restore_version(window, cx); }))));
        modal("history-scrim", ModalAt::Center, false, panel, cx)
    }
}
