//! Add by identifier.

use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use crate::library::Added;
use crate::overlay::Overlay;
use crate::theme::{self, Theme, craft};
use crate::ui::{ModalAt, dialog, field_well, menu_action, modal, plain_button, primary_button, secondary_button};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::identifier::{Identifier, identifiers};
use thesio_core::lookup;

/// The Add by Identifier dialog.
pub struct Lookup {
    field: Entity<TextField>,
    busy: bool,
    /// What could not be added, one line each.
    problems: Vec<String>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub(crate) fn open_lookup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut f = TextField::new("DOI, arXiv ID, PMID, or ISBN", cx);
            f.placeholder_color = Theme::text_disabled;
            f
        });
        let subs = vec![cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| if let TextFieldEvent::Submit = ev { w.run_lookup(window, cx); })];
        let focus = field.focus_handle(cx);
        self.open_overlay(Overlay::Lookup(Lookup { field, busy: false, problems: vec![], _subs: subs }), window, cx);
        window.focus(&focus);
    }

    /// Fetches everything in the field, then adds what came back.
    pub(crate) fn run_lookup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Lookup(dialog)) = &mut self.overlay else { return };
        if dialog.busy { return; }
        let text = dialog.field.read(cx).text().to_string();
        let parsed = identifiers(&text);
        if parsed.is_empty() { return; }
        dialog.busy = true;
        dialog.problems.clear();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let fetched: Vec<(Option<Identifier>, Result<serde_yaml::Mapping, String>)> = cx.background_spawn(async move {
                parsed.into_iter().map(|token| match token {
                    Ok(id) => {
                        let result = lookup::fetch(&id).map_err(|e| format!("{id}: {e}"));
                        (Some(id), result)
                    }
                    Err(text) => (None, Err(format!("{text}: not a DOI, arXiv ID, PMID, or ISBN"))),
                }).collect()
            }).await;
            let _ = this.update_in(cx, |w, window, cx| w.finish_lookup(fetched, window, cx));
        }).detach();
    }

    pub(crate) fn finish_lookup(&mut self, fetched: Vec<(Option<Identifier>, Result<serde_yaml::Mapping, String>)>, window: &mut Window, cx: &mut Context<Self>) {
        self.reload_library();
        let (mut added, mut already) = (0, 0);
        let mut problems = vec![];
        let mut keys = vec![];
        for (id, result) in fetched {
            let entry = match result {
                Ok(entry) => entry,
                Err(problem) => {
                    problems.push(problem);
                    continue;
                }
            };
            let Some(library) = &mut self.citations.library else {
                problems.push("The library could not be read".to_string());
                break;
            };
            match library.add_fetched(entry) {
                Ok(Added::New(key)) => {
                    added += 1;
                    keys.push(key);
                }
                Ok(Added::Duplicate(_)) => already += 1,
                Err(e) => problems.push(format!("{}: {e}", id.map_or(String::new(), |id| id.to_string()))),
            }
        }
        if added > 0 { self.refresh_after_library_change(cx); }
        self.table_changed(window, cx);
        self.fetch_papers(keys, cx);
        let summary = match (added, already) {
            (0, 0) => None,
            (1, 0) => Some("Added 1 entry".to_string()),
            (n, 0) => Some(format!("Added {n} entries")),
            (0, 1) => Some("Already in the library".to_string()),
            (0, k) => Some(format!("All {k} are already in the library")),
            (n, k) => Some(format!("Added {n}, {k} already in the library")),
        };
        if let Some(summary) = summary { self.show_toast(&summary, cx); }
        match &mut self.overlay {
            Some(Overlay::Lookup(dialog)) if !problems.is_empty() => {
                dialog.busy = false;
                dialog.problems = problems;
            }
            Some(Overlay::Lookup(_)) => self.close_overlay(window, cx),
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn render_lookup(&self, lookup: &Lookup, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let problems = lookup.problems.iter().map(|p| div().text_size(px(13.)).line_height(px(18.)).text_color(theme::c(craft::R400)).child(p.clone())).collect::<Vec<_>>();
        let panel = dialog("lookup").w(px(480.)).p(px(20.)).flex().flex_col().gap(px(10.))
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Add by identifier"))
            .child(div().text_size(px(13.)).line_height(px(18.)).text_color(Theme::text_caption()).child("Enter one or more DOIs, arXiv IDs, PMIDs, or ISBNs. Each is sent to doi.org, NCBI, or Open Library, and Thesio looks for an open-access PDF through arXiv and Unpaywall."))
            .child(field_well(&lookup.field, window, cx))
            .children(problems)
            .child(div().mt(px(8.)).flex().items_center().gap(px(8.))
                .child(plain_button("lookup-manual", "Enter Manually…").on_click(menu_action(cx, |w, window, cx| w.open_entry(None, window, cx))))
                .child(div().flex_1())
                .child(secondary_button("lookup-cancel", "Cancel").on_click(cx.listener(|w, _, window, cx| w.close_overlay(window, cx))))
                .when(lookup.busy, |d| d.child(div().px(px(12.)).text_size(px(13.)).text_color(Theme::text_caption()).child("Looking up…")))
                .when(!lookup.busy, |d| d.child(primary_button("lookup-add", "Add").on_click(cx.listener(|w, _, window, cx| w.run_lookup(window, cx))))));
        modal("lookup-scrim", ModalAt::Top, false, panel, cx)
    }
}
