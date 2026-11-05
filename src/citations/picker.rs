//! The source picker: cites, or chooses the source for a paper.

use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use crate::library::{PAPERS_DIR, Row, paper_file};
use crate::overlay::Overlay;
use crate::theme::Theme;
use crate::ui::{ModalAt, dialog, modal, plain_button, search_well, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::path::PathBuf;

const PICKER_ROWS: usize = 8;

pub struct CitePicker {
    field: Entity<TextField>,
    selected: usize,
    purpose: PickFor,
    _subs: Vec<Subscription>,
}

/// What choosing a source in the picker does.
#[derive(Clone)]
pub(crate) enum PickFor {
    Cite,
    /// Copy this PDF in as the source's paper.
    Attach(PathBuf),
    /// Make this unlinked paper the source's.
    Link(String),
}

impl Workspace {
    pub(crate) fn open_cite_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_source_picker(PickFor::Cite, window, cx);
    }

    pub(crate) fn open_source_picker(&mut self, purpose: PickFor, window: &mut Window, cx: &mut Context<Self>) {
        self.reload_library();
        let field = cx.new(|cx| {
            let mut f = TextField::new("Search library", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| {
            let rows = match &w.overlay {
                Some(Overlay::Picker(picker)) => w.picker_rows(picker, cx),
                _ => return,
            };
            let Some(Overlay::Picker(picker)) = &mut w.overlay else { return };
            match ev {
                TextFieldEvent::Changed => picker.selected = 0,
                TextFieldEvent::Up => picker.selected = picker.selected.saturating_sub(1),
                TextFieldEvent::Down => picker.selected = (picker.selected + 1).min(rows.len().saturating_sub(1)),
                TextFieldEvent::Submit => {
                    let selected = picker.selected;
                    if let Some(row) = rows.get(selected) { w.pick_source(&row.key, window, cx); }
                }
                _ => {}
            }
            cx.notify();
        })];
        let focus = field.focus_handle(cx);
        self.open_overlay(Overlay::Picker(CitePicker { field, selected: 0, purpose, _subs: subs }), window, cx);
        window.focus(&focus);
    }

    pub(crate) fn pick_source(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Picker(CitePicker { purpose, .. })) = self.take_overlay(window, cx) else { return };
        match purpose {
            PickFor::Cite => self.insert_citation(key, window, cx),
            PickFor::Attach(path) => self.attach_paper(&path, &paper_file(key), cx),
            PickFor::Link(id) => self.link_paper(&id, key, cx),
        }
    }

    pub(crate) fn picker_rows(&self, picker: &CitePicker, cx: &App) -> Vec<Row> {
        let query = picker.field.read(cx).text.trim().to_lowercase();
        let mut rows = self.visible_rows(&query);
        // A source holds one paper.
        if !matches!(picker.purpose, PickFor::Cite) { rows.retain(|row| !self.has_paper(&row.key)); }
        rows.truncate(PICKER_ROWS);
        rows
    }

    pub(crate) fn render_cite_picker(&self, picker: &CitePicker, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.picker_rows(picker, cx);
        let empty = rows.is_empty();
        let list = rows.into_iter().enumerate().map(|(i, row)| {
            let key = row.key.clone();
            div().id(("cite-row", i)).px(px(12.)).py(px(6.)).rounded(px(8.)).cursor_pointer().flex().flex_col()
                .when(i == picker.selected, |d| d.bg(Theme::fill_hover_strong()))
                .child(div().text_size(px(13.)).line_height(px(18.)).truncate().child(ui_text(if row.title.is_empty() { row.key.clone() } else { row.title.clone() })))
                .child(div().text_size(px(12.)).line_height(px(16.)).truncate().text_color(Theme::text_caption()).child(format!("{} · {} · @{}", row.authors, row.year, row.key)))
                .on_click(cx.listener(move |w, _, window, cx| w.pick_source(&key, window, cx)))
        }).collect::<Vec<_>>();
        let hint = (!matches!(picker.purpose, PickFor::Cite)).then(|| div().px(px(4.)).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Choose the source for this paper"));
        let unlinked = if let PickFor::Attach(path) = &picker.purpose { Some(path.clone()) } else { None };
        let panel = dialog("cite-picker").w(px(520.)).p(px(12.)).flex().flex_col().gap(px(8.))
            .children(hint)
            .child(search_well(&picker.field, window, cx))
            .when(empty, |d| d.child(div().px(px(12.)).py(px(8.)).text_size(px(13.)).text_color(Theme::text_caption()).child("No matching entries")))
            .children(list)
            .when_some(unlinked, |d, path| d.child(div().flex().child(plain_button("picker-unlinked", "Add Without a Source").on_click(cx.listener(move |w, _, window, cx| {
                w.close_overlay(window, cx);
                let name = path.file_stem().map_or("paper".into(), |name| name.to_string_lossy().into_owned());
                w.attach_paper(&path, &format!("{PAPERS_DIR}/{name}.pdf"), cx);
            })))));
        modal("cite-scrim", ModalAt::Top, true, panel, cx)
    }
}
