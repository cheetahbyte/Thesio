//! Keeping each document's bibliography in step with the library.

use crate::app::{View, Workspace};
use crate::document::Edit;
use crate::ui::ToastKind;
use gpui::*;
use std::collections::BTreeSet;
use thesio_core::bibliography::{self, Mode};
use thesio_core::paths::is_document;
use thesio_core::project::{Documents, Project};
use thesio_core::references::{cited_keys, rename_citation};

impl Workspace {
    /// Every key cited in a document and the files it includes.
    pub(crate) fn cited_in(&self, id: &str, cx: &App) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for (_, text) in Project::open(id, &mut |file| self.text_of(file, cx).ok()).files() {
            keys.extend(cited_keys(text));
        }
        keys
    }

    /// Every document, for asking which includes which.
    pub(crate) fn documents(&self, cx: &App) -> Documents {
        Documents::load(self.entries.iter().filter(|e| e.typst).map(|e| e.id.as_str()), &mut |file| self.text_of(file, cx).ok())
    }

    /// Rewrites each generated block to hold what its document cites.
    pub(crate) fn sync_bibliographies(&mut self, cx: &mut Context<Self>) {
        let Some(library) = &self.citations.library else { return };
        let documents: Vec<String> = self.entries.iter().filter(|e| e.typst && bibliography::block_range(&e.body).is_some()).map(|e| e.id.clone()).collect();
        let mut updates = vec![];
        for id in documents {
            let keys: BTreeSet<String> = self.cited_in(&id, cx).into_iter().filter(|k| library.contains(k)).collect();
            let (Ok(yaml), Ok(text)) = (library.subset(&keys), self.text_of(&id, cx)) else { continue };
            if let Some(updated) = bibliography::sync_block(&text, &yaml) {
                updates.push((id, updated));
            }
        }
        for (id, text) in updates {
            if let Err(e) = self.write_document(&id, &text, Edit::Generated, cx) {
                self.show_toast_kind(ToastKind::Error, &format!("Could not update the bibliography in {id}: {e}"), cx);
            }
        }
    }

    pub(crate) fn bibliography_mode(&self, cx: &App) -> Mode {
        self.current.as_ref().and_then(|id| Some(bibliography::mode(&self.text_of(id, cx).ok()?, id))).unwrap_or(Mode::None)
    }

    /// Switches the open document's bibliography as one undoable edit.
    pub(crate) fn set_bibliography(&mut self, target: Mode, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        let Ok(text) = self.text_of(&id, cx) else { return };
        let text = bibliography::remove(&text, &id);
        let text = match target {
            Mode::Embedded => bibliography::embed(&text, ""),
            Mode::Linked => bibliography::link(&text, &id),
            _ => text,
        };
        if let Err(e) = self.write_document(&id, &text, Edit::Undoable, cx) {
            self.show_toast_kind(ToastKind::Error, &format!("Could not save {id}: {e}"), cx);
        }
    }

    /// The document holding the open file's bibliography, and its style. A
    /// chapter's bibliography is usually in the document that includes it.
    pub(crate) fn style_owner(&self, cx: &App) -> Option<(String, String)> {
        let id = self.current.clone()?;
        [id.clone(), self.top_document(&id, cx)].into_iter().find_map(|doc| {
            let text = self.text_of(&doc, cx).ok()?;
            let mode = bibliography::mode(&text, &doc);
            (mode != Mode::None).then(|| {
                let style = bibliography::style(&text).unwrap_or_else(|| bibliography::DEFAULT_STYLE.to_string());
                (doc, style)
            })
        })
    }

    /// Gives the open file's bibliography a citation style.
    pub(crate) fn set_citation_style(&mut self, style: &str, cx: &mut Context<Self>) {
        let Some((doc, _)) = self.style_owner(cx) else { return };
        let Some(updated) = self.text_of(&doc, cx).ok().and_then(|text| bibliography::set_style(&text, style)) else { return };
        if let Err(e) = self.write_document(&doc, &updated, Edit::Undoable, cx) {
            self.show_toast_kind(ToastKind::Error, &format!("Could not change the style in {doc}: {e}"), cx);
        }
    }

    /// The top-level document `id` belongs to: a document has one bibliography,
    /// so it goes there and not in an included chapter.
    pub(crate) fn top_document(&self, id: &str, cx: &App) -> String {
        self.documents(cx).top_document(id)
    }

    /// Embeds a bibliography in `id`'s top-level document unless the document
    /// or any file above it already has one.
    pub(crate) fn ensure_bibliography(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.bibliography_mode(cx) != Mode::None { return; }
        let documents = self.documents(cx);
        if documents.includers(id).iter().any(|doc| self.text_of(doc, cx).is_ok_and(|text| bibliography::mode(&text, doc) != Mode::None)) { return; }
        let top = documents.top_document(id);
        let Ok(text) = self.text_of(&top, cx) else { return };
        if let Err(e) = self.write_document(&top, &bibliography::embed(&text, ""), Edit::Generated, cx) {
            self.show_toast_kind(ToastKind::Error, &format!("Could not add a bibliography to {top}: {e}"), cx);
        }
    }

    /// Cites `key` at the caret, embedding a bibliography if nothing covers this
    /// document yet.
    pub(crate) fn insert_citation(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone()) else {
            return self.show_toast_kind(ToastKind::Error, "Open a document to insert a citation", cx);
        };
        if !is_document(&id) {
            return self.show_toast_kind(ToastKind::Error, "Citations go in Typst documents", cx);
        }
        self.show_view(View::Doc, cx);
        editor.update(cx, |e, cx| e.insert_citation(key, cx));
        self.citation_added(&id, cx);
        editor.read(cx).focus(window);
        cx.notify();
    }

    /// Saves a new citation and makes sure its document has a bibliography.
    pub(crate) fn citation_added(&mut self, id: &str, cx: &mut Context<Self>) {
        self.ensure_bibliography(id, cx);
        self.mark_unsaved(cx);
    }

    /// Follows a key change through every document that cites it.
    pub(crate) fn rename_citations(&mut self, old: &str, new: &str, cx: &mut Context<Self>) {
        let mut changed = 0;
        for id in self.entries.iter().filter(|e| e.typst).map(|e| e.id.clone()).collect::<Vec<_>>() {
            let Ok(text) = self.text_of(&id, cx) else { continue };
            let Some(updated) = rename_citation(&text, old, new) else { continue };
            if let Err(e) = self.write_document(&id, &updated, Edit::Undoable, cx) {
                self.show_toast_kind(ToastKind::Error, &format!("Could not update the citation in {id}: {e}"), cx);
                continue;
            }
            changed += 1;
        }
        match changed {
            0 => {}
            1 => self.show_toast("Updated the citation in 1 document", cx),
            n => self.show_toast(&format!("Updated citations in {n} documents"), cx),
        }
    }
}
