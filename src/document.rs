//! The open document's text on its way to disk, and every other write to a
//! document. A failed save keeps the changes pending and asks before anything
//! replaces them. Everything else that fails says so in a toast.

use crate::app::{SaveStatus, View, Workspace};
use crate::confirm::AfterSave;
use crate::ui::ToastKind;
use gpui::*;
use std::time::Duration;

/// How text that did not come from typing enters an editor.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Edit {
    /// One step the user can undo.
    Undoable,
    /// Thesio's own text: outside the undo history, and not announced.
    Generated,
}

impl Workspace {
    /// What `id` holds now: the open editor's copy, or the file.
    pub(crate) fn text_of(&self, id: &str, cx: &App) -> anyhow::Result<String> {
        match (&self.current, &self.editor) {
            (Some(current), Some(editor)) if current == id => Ok(editor.read(cx).buffer.text().to_string()),
            _ => self.store.borrow().load(id),
        }
    }

    /// Gives `text` to the editors showing `id`.
    pub(crate) fn show_text(&mut self, id: &str, text: &str, edit: Edit, cx: &mut Context<Self>) {
        let open = self.editor.clone().filter(|_| self.current.as_deref() == Some(id));
        for editor in open.into_iter().chain(self.papers.panel.notes_editor(id)) {
            editor.update(cx, |e, cx| match edit {
                Edit::Undoable => e.reload(text, cx),
                Edit::Generated => e.sync_text(text, cx),
            });
        }
    }

    /// The one way text reaches a document: the file, its entry, the editors
    /// showing it, and the preview, which may compile it through an include.
    /// When it fails the open document keeps the text and retries on the next save.
    pub(crate) fn write_document(&mut self, id: &str, text: &str, edit: Edit, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let open = self.current.as_deref() == Some(id);
        self.show_text(id, text, edit, cx);
        if open { self.unsaved = true; }
        self.commit(id, text)?;
        if open { self.saved(); }
        if !open || edit == Edit::Generated { self.recompile(cx); }
        cx.notify();
        Ok(())
    }

    fn commit(&mut self, id: &str, text: &str) -> anyhow::Result<()> {
        self.store.borrow_mut().save(id, text)?;
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.body = text.to_string();
            entry.modified = crate::time::now();
        }
        Ok(())
    }

    fn saved(&mut self) {
        self.status = SaveStatus::Saved;
        self.unsaved = false;
    }

    fn recompile(&mut self, cx: &mut Context<Self>) {
        let (Some(editor), Some(session)) = (self.editor.clone(), self.session.clone()) else { return };
        let text = editor.read(cx).buffer.text().to_string();
        session.update(cx, |session, cx| session.set_text(text, cx));
    }

    /// Writes the open document again after text changed without an edit.
    pub(crate) fn mark_unsaved(&mut self, cx: &mut Context<Self>) {
        self.unsaved = true;
        self.flush(cx);
    }

    pub(crate) fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = SaveStatus::Saving;
        self.unsaved = true;
        self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let _ = this.update(cx, |w, cx| w.flush(cx));
        }));
        cx.notify();
    }

    /// Writes the open document to disk if it has unsaved changes. False when
    /// that failed: the changes stay pending (a later save retries them) and
    /// the top bar shows the error, or a toast away from the document.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_task = None;
        if !self.unsaved {
            return true;
        }
        let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone()) else {
            self.unsaved = false;
            return true;
        };
        let text = editor.read(cx).buffer.text().to_string();
        match self.commit(&id, &text) {
            Ok(()) => {
                self.saved();
                self.keep_version(&id, &text, false, cx);
                self.sort();
                self.sync_bibliographies(cx);
                self.refresh_outline(cx);
            }
            Err(e) => {
                let error = format!("Could not save: {e}");
                if self.view != View::Doc { self.show_toast_kind(ToastKind::Error, &error, cx); }
                self.status = SaveStatus::Failed(error);
            }
        }
        cx.notify();
        !self.unsaved
    }

    /// Everything that waits for a save: the Library's panel and the open document.
    pub(crate) fn save_all(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_inspector(cx);
        self.flush(cx)
    }

    /// Saves before something replaces the open document. False when that
    /// failed: nothing is replaced, and the Save failed dialog offers Retry /
    /// Discard changes and runs `then` after either.
    pub(crate) fn save_before(&mut self, window: &mut Window, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> bool {
        let saved = self.save_all(cx);
        if !saved { self.ask_save_failed(Box::new(then), window, cx); }
        saved
    }

    /// Runs `then` now if the open document saves, or from the Save failed dialog.
    pub(crate) fn when_saved(&mut self, window: &mut Window, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static) {
        if self.save_all(cx) { then(self, window, cx); } else { self.ask_save_failed(Box::new(then), window, cx); }
    }

    /// True when this window may close now. Otherwise the Save failed dialog closes it.
    pub(crate) fn ready_to_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        crate::window_size::persist(&self.store, cx);
        self.save_before(window, cx, |_, window, _| window.remove_window())
    }

    /// The Save failed dialog's choices: `retry` saves again, otherwise the
    /// changes are given up. Either way the held-back action runs once the
    /// document no longer needs saving.
    pub(crate) fn resolve_save_failed(&mut self, retry: bool, then: AfterSave, window: &mut Window, cx: &mut Context<Self>) {
        if retry {
            if !self.flush(cx) { return self.ask_save_failed(then, window, cx); }
        } else {
            self.unsaved = false;
            self.status = SaveStatus::Idle;
        }
        then(self, window, cx);
        cx.notify();
    }

    /// The error of the last failed save, for the Save failed dialog.
    pub(crate) fn save_error(&self) -> String {
        match &self.status {
            SaveStatus::Failed(e) => e.clone(),
            _ => "Save failed".to_string(),
        }
    }
}
