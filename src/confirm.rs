//! The confirmation dialog, and what each one asks about.

use crate::app::Workspace;
use crate::overlay::Overlay;
use crate::store::display_title;
use crate::theme::Theme;
use crate::ui::{ModalAt, dialog, primary_button, secondary_button, ui_para};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::references::cited_keys;

/// A confirmation dialog and the pages it is about.
pub enum Confirm {
    /// Delete to Trash; `batch` for the selected rows.
    Delete { ids: Vec<String>, batch: bool },
    /// Trash → Restore.
    Restore(String),
    /// Trash → Delete permanently.
    Purge(String),
    /// Library → delete these entries.
    DeleteEntries(Vec<String>),
    /// Library → change an entry to a type without a parent.
    ChangeSourceType { key: String, kind: &'static thesio_core::source_type::SourceType },
    /// Library → remove a tag from every entry.
    DeleteTag(String),
    /// The open document could not be written before it was to be replaced
    /// (another document, New, Quit): Retry / Discard changes, then carry on.
    SaveFailed(AfterSave),
}

/// What to do once the open document is safely on disk (or its changes given up).
pub type AfterSave = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

impl Workspace {
    /// The confirmation dialog: a modal (white, radius 16), a 15pt
    /// semibold title, one line of 13pt body, Cancel and the action button on
    /// the right.
    pub(crate) fn render_confirm(&self, c: &Confirm, cx: &mut Context<Self>) -> AnyElement {
        let title_of = |id: &str| {
            self.entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| display_title(&e.title))
                .or_else(|| self.trash.iter().find(|t| t.id == id).map(|t| display_title(&t.title)))
                .unwrap_or_default()
        };
        let (title, body, action, danger) = match c {
            Confirm::Delete { ids, batch } => (
                if *batch || ids.len() > 1 { format!("Delete the {} selected items?", ids.len()) } else { format!("Delete {}?", title_of(&ids[0])) },
                "Deleted items will be moved to Trash and permanently deleted after 30 days.".to_string(),
                "Delete",
                true,
            ),
            Confirm::Restore(_) => ("Restore this document?".to_string(), "The document will be restored to its original location.".to_string(), "Restore", false),
            Confirm::Purge(_) => ("Delete permanently?".to_string(), "This page and its child pages cannot be recovered after permanent deletion.".to_string(), "Delete", true),
            Confirm::DeleteEntries(keys) => {
                let citing = self.entries.iter().filter(|e| e.typst && { let cited = cited_keys(&e.body); keys.iter().any(|key| cited.contains(key)) }).count();
                let (it, title) = match keys.as_slice() {
                    [key] => ("it", format!("Delete {key}?")),
                    keys => ("them", format!("Delete {} entries?", keys.len())),
                };
                let effect = match citing {
                    0 => format!("No document cites {it}."),
                    1 => format!("1 document cites {it} and will show an error."),
                    n => format!("{n} documents cite {it} and will show an error."),
                };
                (title, format!("This removes {it} from the library and cannot be undone. {effect}"), "Delete", true)
            }
            Confirm::ChangeSourceType { kind, .. } => (format!("Change to {}?", kind.label), "This removes the publication it appeared in, along with that publication's details. It cannot be undone.".to_string(), "Change", true),
            Confirm::DeleteTag(tag) => {
                let count = self.tag_count(tag);
                let entries = if count == 1 { "1 entry".to_string() } else { format!("{count} entries") };
                (format!("Delete the tag {tag}?"), format!("This removes it from {entries}. The entries stay in the library."), "Delete", true)
            }
            Confirm::SaveFailed(_) => (
                "Could not save document".to_string(),
                format!("{}. Retry, or cancel and export a copy using More → Export as PDF. Discarding changes will lose your unsaved work.", self.save_error()),
                "Retry",
                false,
            ),
        };
        let ok = if danger { crate::ui::danger_button("confirm-ok", action) } else { primary_button("confirm-ok", action) };
        let dialog = dialog("confirm-dialog")
            .w(px(400.))
            .px(px(24.))
            .pt(px(20.))
            .pb(px(24.))
            .child(div().text_size(px(15.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).child(ui_para(title)))
            .child(div().mt(px(8.)).text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(ui_para(body)))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1())
                    .child(secondary_button("confirm-cancel", "Cancel").on_click(cx.listener(|w, _, window, cx| w.close_overlay(window, cx))))
                    .when(matches!(c, Confirm::SaveFailed(_)), |d| {
                        d.child(secondary_button("confirm-discard", "Discard changes").text_color(Theme::danger()).on_click(cx.listener(|w, _, window, cx| {
                            if let Some(Overlay::Confirm(Confirm::SaveFailed(then))) = w.take_overlay(window, cx) { w.resolve_save_failed(false, then, window, cx); }
                        })))
                    })
                    .child(ok.on_click(cx.listener(|w, _, window, cx| {
                        let Some(Overlay::Confirm(c)) = w.take_overlay(window, cx) else { return };
                        match c {
                            Confirm::Delete { ids, batch } => {
                                if batch {
                                    w.batch = None;
                                }
                                w.delete_docs(&ids, window, cx);
                            }
                            Confirm::Restore(id) => w.restore_doc(&id, cx),
                            Confirm::Purge(id) => w.purge_doc(&id, cx),
                            Confirm::DeleteEntries(keys) => w.delete_entries(&keys, cx),
                            Confirm::DeleteTag(tag) => w.delete_tag(&tag, cx),
                            Confirm::ChangeSourceType { key, kind } => w.set_source_type(&key, kind, window, cx),
                            Confirm::SaveFailed(then) => w.resolve_save_failed(true, then, window, cx),
                        }
                        cx.notify();
                    }))),
            );
        crate::ui::modal("confirm-mask", ModalAt::Center, false, dialog, cx)
    }
}
