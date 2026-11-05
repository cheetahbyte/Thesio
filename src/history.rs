//! The Version History dialog: earlier versions of the open document, what
//! changed since each, and restoring one.

use crate::app::Workspace;
use crate::library::Library;
use crate::theme::{self, Theme, craft};
use crate::trash::{dialog, list_line, primary_button, scrim, secondary_button};
use crate::ui::{ToastKind, format_visit, ui_para, ui_text};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::history::{DiffLine, INTERVAL, LineKind, diff};

const WIDTH: f32 = 960.;
const HEIGHT: f32 = 620.;
const LIST_W: f32 = 220.;
const LINE_H: f32 = 19.;
const SIGN_W: f32 = 16.;

pub struct History {
    /// The library rather than the open document.
    library: bool,
    versions: Vec<u64>,
    selected: usize,
    diff: Vec<DiffLine>,
    list: ListState,
}

impl Workspace {
    /// Keeps the open text as a version, at most once per `INTERVAL` unless `now`.
    pub(crate) fn keep_version(&mut self, id: &str, text: &str, now: bool, cx: &mut Context<Self>) {
        let time = crate::time::now();
        if !now && self.last_version.get(id).is_some_and(|last| time.saturating_sub(*last) < INTERVAL) { return; }
        match self.store.snapshot(id, text) {
            Ok(_) => { self.last_version.insert(id.to_string(), time); }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not keep a version of {id}: {e}"), cx),
        }
    }

    pub(crate) fn open_history(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        if !self.flush(cx) { return; }
        let versions = self.store.versions(&id);
        self.history = Some(History { library: false, versions, selected: 0, diff: vec![], list: ListState::new(0, ListAlignment::Top, px(400.)) });
        self.show_version(0, cx);
    }

    pub(crate) fn open_library_history(&mut self, cx: &mut Context<Self>) {
        self.save_inspector(cx);
        let Some(library) = &self.citations.library else { return };
        let versions = library.versions();
        self.history = Some(History { library: true, versions, selected: 0, diff: vec![], list: ListState::new(0, ListAlignment::Top, px(400.)) });
        self.show_version(0, cx);
    }

    /// What the dialog compares against.
    fn history_text(&self, cx: &App) -> Option<String> {
        match self.history.as_ref()?.library {
            true => self.citations.library.as_ref().map(Library::text),
            false => self.editor.as_ref().map(|editor| editor.read(cx).buffer.text().to_string()),
        }
    }

    fn version_text(&self, time: u64) -> anyhow::Result<String> {
        match (self.history.as_ref().is_some_and(|history| history.library), &self.citations.library, &self.current) {
            (true, Some(library), _) => library.version(time),
            (false, _, Some(id)) => self.store.version(id, time),
            _ => anyhow::bail!("Nothing is open"),
        }
    }

    fn show_version(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(current) = self.history_text(cx) else { return };
        let Some(history) = &mut self.history else { return };
        let Some(time) = history.versions.get(index).copied() else { return };
        history.selected = index;
        match self.version_text(time) {
            Ok(text) => {
                // Read as: what restoring this version would change.
                let lines = diff(&current, &text);
                if let Some(history) = &mut self.history {
                    history.list.reset(lines.len());
                    history.diff = lines;
                }
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &e.to_string(), cx),
        }
        cx.notify();
    }

    pub(crate) fn close_history(&mut self, cx: &mut Context<Self>) {
        self.history = None;
        cx.notify();
    }

    /// Brings a version back. What it replaces becomes a version first, and
    /// the editor can undo it.
    fn restore_version(&mut self, cx: &mut Context<Self>) {
        let Some(history) = &self.history else { return };
        let library = history.library;
        let Some(time) = history.versions.get(history.selected).copied() else { return };
        let text = match self.version_text(time) {
            Ok(text) => text,
            Err(e) => return self.show_toast_kind(ToastKind::Error, &e.to_string(), cx),
        };
        if library {
            if let Err(e) = self.citations.library.as_mut().map_or(Ok(()), |library| library.restore(&text)) {
                return self.show_toast_kind(ToastKind::Error, &format!("Could not restore the library: {e}"), cx);
            }
            self.library_restored(&text, cx);
        } else {
            let (Some(id), Some(current), Some(editor)) = (self.current.clone(), self.history_text(cx), self.editor.clone()) else { return };
            self.keep_version(&id, &current, true, cx);
            editor.update(cx, |e, cx| e.reload(&text, cx));
            self.flush(cx);
        }
        self.history = None;
        self.show_toast(&format!("Restored the version from {}", format_visit(time)), cx);
        cx.notify();
    }

    pub(crate) fn render_history(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let history = self.history.as_ref()?;
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
                    LineKind::Added => (Some(theme::ca(craft::old::G500, 0.12)), "+"),
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
        let title = if history.library { "the Library".to_string() } else { self.current.as_deref().map_or("", |id| id.rsplit('/').next().unwrap_or(id)).to_string() };
        let can_restore = !history.diff.is_empty();
        let panel = dialog("history").w(px(WIDTH)).h(px(HEIGHT)).flex().flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex_none().px(px(20.)).py(px(14.)).border_b_1().border_color(list_line()).flex().flex_col().gap(px(2.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(ui_text(format!("Version History of {title}"))))
                .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child("Green is what restoring brings back. Red is what it replaces.")))
            .child(div().flex_1().min_h_0().flex().child(list).child(body))
            .child(div().flex_none().px(px(20.)).py(px(12.)).border_t_1().border_color(list_line()).flex().items_center().justify_end().gap(px(8.))
                .child(secondary_button("history-close", "Close").on_click(cx.listener(|w, _, _, cx| w.close_history(cx))))
                .child(primary_button("history-restore", "Restore This Version").when(!can_restore, |d| d.opacity(0.5))
                    .on_click(cx.listener(move |w, _, _, cx| if can_restore { w.restore_version(cx); }))));
        Some(deferred(div().id("history-scrim").absolute().inset_0().bg(scrim()).flex().items_center().justify_center().occlude().child(panel)).with_priority(3).into_any_element())
    }
}
