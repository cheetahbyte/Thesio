//! A paper's notes: an ordinary document at `notes/<key>.typ`, beside the viewer.

use super::viewer::{PANEL_W, PaperPanel};
use crate::app::Workspace;
use crate::document::Edit;
use crate::library::{NOTES_DIR, note_file, paper_file};
use crate::source::{SourceEditor, SourceEvent};
use crate::ui::ToastKind;
use gpui::*;
use thesio_core::bibliography::{self, Mode};
use thesio_core::paths::moved;

/// The paper's notes: `id` follows the file, which appears once something is written.
pub(crate) struct Notes {
    pub(crate) id: String,
    pub(crate) editor: Entity<SourceEditor>,
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
        let note = note_file(id);
        let exists = self.store.borrow().path(&note).is_ok_and(|path| path.exists());
        let loaded = self.store.borrow().load(&note);
        let text = match loaded {
            Ok(text) => text,
            Err(_) if !exists => format!("= {}\n\n", self.paper_title(id)),
            // No editor: the next keystroke would replace a file nobody could read.
            Err(e) => {
                self.papers.panel.notes = None;
                return self.show_toast_kind(ToastKind::Error, &format!("Could not read the notes: {e}"), cx);
            }
        };
        let editor = cx.new(|cx| {
            let mut editor = SourceEditor::new(text, true, cx);
            // Inside the panel: the viewer would hide what crosses onto it.
            editor.completion_w = PANEL_W - 40.;
            editor
        });
        let subscription = cx.subscribe(&editor, |w, editor, event: &SourceEvent, cx| {
            let Some(note) = w.papers.panel.notes.as_ref().map(|notes| notes.id.clone()) else { return };
            match event {
                SourceEvent::Changed => { let text = editor.read(cx).text().to_string(); w.save_note(&note, &text, cx); }
                SourceEvent::Citing => w.offer_sources(&editor, cx),
                SourceEvent::Cited => w.link_note_library(&note, &editor, cx),
                SourceEvent::Caret | SourceEvent::Follow(_) => {}
            }
        });
        self.papers.panel.notes = Some(Notes { id: note, editor, _subscription: subscription });
        self.papers.panel.comment = None;
    }

    /// Writes a notes file, which may be new. The same file may be open as a document too.
    pub(crate) fn save_note(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
        let new = !self.entries.iter().any(|e| e.id == id);
        if let Err(e) = self.ensure_notes_dir().and_then(|()| self.write_document(id, text, Edit::Generated, cx)) {
            return self.show_toast_kind(ToastKind::Error, &format!("Could not save the notes: {e}"), cx);
        }
        if new { self.refresh_entries(cx); }
    }

    /// Notes that cite the library read it directly, so they compile on their own.
    pub(crate) fn link_note_library(&mut self, id: &str, editor: &Entity<SourceEditor>, cx: &mut Context<Self>) {
        let text = editor.read(cx).text().to_string();
        if bibliography::mode(&text, id) != Mode::None { return; }
        self.save_note(id, &bibliography::link(&text, id), cx);
    }

    pub(crate) fn ensure_notes_dir(&self) -> anyhow::Result<()> {
        Ok(std::fs::create_dir_all(self.store.borrow().path(NOTES_DIR)?)?)
    }

    /// Opens the notes of `key` as a document, starting them if there are none.
    pub(crate) fn open_source_notes(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let id = note_file(&paper_file(key));
        if !self.entries.iter().any(|e| e.id == id) {
            let title = self.paper_title(&paper_file(key));
            let text = format!("= {title}\n\n");
            if let Err(e) = self.ensure_notes_dir().and_then(|()| self.write_document(&id, &text, Edit::Generated, cx)) {
                return self.show_toast_kind(ToastKind::Error, &format!("Could not create the notes: {e}"), cx);
            }
            self.refresh_entries(cx);
        }
        self.open(&id, window, cx);
    }
}
