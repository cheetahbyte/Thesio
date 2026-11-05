//! A paper's notes: an ordinary document at `notes/<key>.typ`, beside the viewer.

use super::viewer::{PANEL_W, PaperPanel};
use crate::app::Workspace;
use crate::completion::{Citations, CompletionKind};
use crate::document::Edit;
use crate::library::{NOTES_DIR, note_file, paper_file};
use crate::source::{SourceEditor, SourceEvent};
use crate::ui::ToastKind;
use gpui::*;
use std::rc::Rc;
use std::time::Duration;
use thesio_core::bibliography::{self, Mode};
use thesio_core::paths::moved;

const SAVE_DELAY: Duration = Duration::from_millis(500);

/// The paper's notes: `id` follows the file, which appears once something is written.
pub(crate) struct Notes {
    pub(crate) id: String,
    pub(crate) editor: Entity<SourceEditor>,
    unsaved: bool,
    save_task: Option<Task<()>>,
    _subscription: Subscription,
}

impl PaperPanel {
    pub(crate) fn notes_editor(&self, id: &str) -> Option<Entity<SourceEditor>> {
        self.notes.as_ref().filter(|notes| notes.id == id).map(|notes| notes.editor.clone())
    }

    pub(crate) fn path_changed(&mut self, old: &str, new: &str) {
        if let Some(notes) = &mut self.notes && let Some(path) = moved(&notes.id, old, new) { notes.id = path; }
    }
}

impl Workspace {
    /// Loads the notes for paper `id`, or a heading to start them with. The
    /// file only appears once something is written.
    pub(crate) fn load_paper_notes(&mut self, id: &str, cx: &mut Context<Self>) {
        self.flush(cx);
        self.flush_notes(cx);
        let note = note_file(id);
        let exists = self.store.borrow().path(&note).is_ok_and(|path| path.exists());
        let loaded = self.store.borrow().load(&note);
        let text = match loaded {
            Ok(text) => {
                self.keep_version(&note, &text, true, cx);
                text
            }
            Err(_) if !exists => self.notes_start(id),
            // No editor: the next keystroke would replace a file nobody could read.
            Err(e) => {
                self.papers.panel.notes = None;
                return self.show_toast_kind(ToastKind::Error, &format!("Could not read the notes: {e}"), cx);
            }
        };
        // Paper notes have no path of their own here, so they offer no note links.
        let workspace = cx.weak_entity();
        let editor = cx.new(|cx| {
            let mut editor = SourceEditor::new(text, true, vec![Rc::new(Citations(workspace))], cx);
            // Inside the panel: the viewer would hide what crosses onto it.
            editor.completion_w = PANEL_W - 40.;
            editor
        });
        let subscription = cx.subscribe(&editor, |w, editor, event: &SourceEvent, cx| {
            let Some(note) = w.papers.panel.notes.as_ref().map(|notes| notes.id.clone()) else { return };
            match event {
                SourceEvent::Changed => w.schedule_notes_save(cx),
                SourceEvent::Completed(CompletionKind::Citation) => w.link_note_library(&note, &editor, cx),
                SourceEvent::Completed(_) | SourceEvent::Caret | SourceEvent::Follow(_) => {}
            }
        });
        self.papers.panel.notes = Some(Notes { id: note, editor, unsaved: false, save_task: None, _subscription: subscription });
        self.papers.panel.comment = None;
    }

    /// What a paper's notes begin with.
    fn notes_start(&self, paper: &str) -> String {
        format!("= {}\n\n", self.paper_title(paper))
    }

    fn schedule_notes_save(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |w, cx| w.flush_notes(cx));
        });
        if let Some(notes) = &mut self.papers.panel.notes {
            notes.unsaved = true;
            notes.save_task = Some(task);
        }
    }

    /// Writes the panel's notes if they changed. When that fails they stay
    /// pending, and a later save retries them.
    pub(crate) fn flush_notes(&mut self, cx: &mut Context<Self>) {
        let Some(notes) = &mut self.papers.panel.notes else { return };
        notes.save_task = None;
        if !notes.unsaved { return; }
        let (id, text) = (notes.id.clone(), notes.editor.read(cx).text().to_string());
        if self.save_note(&id, &text, cx) {
            self.keep_version(&id, &text, false, cx);
            if let Some(notes) = &mut self.papers.panel.notes { notes.unsaved = false; }
        }
    }

    /// Writes a notes file, which may be new. The same file may be open as a document too.
    fn write_notes(&mut self, id: &str, text: &str, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let new = !self.entries.iter().any(|e| e.id == id);
        std::fs::create_dir_all(self.store.borrow().path(NOTES_DIR)?)?;
        self.write_document(id, text, Edit::Generated, cx)?;
        if new { self.refresh_entries(cx); }
        Ok(())
    }

    fn save_note(&mut self, id: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let result = self.write_notes(id, text, cx);
        if let Err(e) = &result { self.show_toast_kind(ToastKind::Error, &format!("Could not save the notes: {e}"), cx); }
        result.is_ok()
    }

    /// Notes that cite the library read it directly, so they compile on their own.
    pub(crate) fn link_note_library(&mut self, id: &str, editor: &Entity<SourceEditor>, cx: &mut Context<Self>) {
        let text = editor.read(cx).text().to_string();
        if bibliography::mode(&text, id) != Mode::None { return; }
        self.save_note(id, &bibliography::link(&text, id), cx);
    }

    /// Opens the notes of `key` as a document, starting them if there are none.
    pub(crate) fn open_source_notes(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let id = note_file(&paper_file(key));
        if !self.entries.iter().any(|e| e.id == id) {
            let text = self.notes_start(&paper_file(key));
            if let Err(e) = self.write_notes(&id, &text, cx) {
                return self.show_toast_kind(ToastKind::Error, &format!("Could not create the notes: {e}"), cx);
            }
        }
        self.open(&id, window, cx);
    }
}
