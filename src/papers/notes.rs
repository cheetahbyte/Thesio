//! A paper's notes: an ordinary document at `notes/<key>.typ`, beside the viewer.

use super::viewer::{PANEL_W, PaperPanel};
use crate::app::Workspace;
use crate::citations::CitationKeys;
use crate::completion::{CompletionKind, Provider};
use crate::document::Edit;
use crate::session::DocumentSession;
use thesio_core::library::{NOTES_DIR, note_file, paper_file};
use crate::source::{SourceEditor, SourceEvent};
use crate::typst_code::TypstCode;
use crate::ui::ToastKind;
use gpui::*;
use std::rc::Rc;
use thesio_core::bibliography::{self, Mode};
use thesio_core::paths::moved;

/// The paper's notes: `id` follows the file, which appears once something is written.
pub(crate) struct Notes {
    pub(crate) id: String,
    pub(crate) editor: Entity<SourceEditor>,
    /// Its compiles, for completion and hover: notes have no `Preview`.
    session: Entity<DocumentSession>,
    unsaved: bool,
    /// Why the last save failed, for the Save failed dialog.
    error: Option<String>,
    save_task: Option<Task<()>>,
    _subscription: Subscription,
}

impl PaperPanel {
    pub(crate) fn notes_editor(&self, id: &str) -> Option<Entity<SourceEditor>> {
        self.notes.as_ref().filter(|notes| notes.id == id).map(|notes| notes.editor.clone())
    }

    pub(crate) fn path_changed(&mut self, old: &str, new: &str, cx: &mut App) {
        let Some(notes) = &mut self.notes else { return };
        let Some(path) = moved(&notes.id, old, new) else { return };
        notes.id = path.clone();
        let text = notes.editor.read(cx).text().to_string();
        notes.session.update(cx, |session, cx| {
            session.set_path(path.clone(), path);
            session.set_text(text, cx);
        });
    }
}

impl Workspace {
    /// Loads the notes for paper `id`, or a heading to start them with. The
    /// file only appears once something is written. The panel's last notes
    /// are gone by now: `open_paper` saves them first.
    pub(crate) fn load_paper_notes(&mut self, id: &str, cx: &mut Context<Self>) {
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
        // A note is its own main file. Paper notes have no path of their own
        // here, so they offer no note links.
        let root = self.store.borrow().root().to_path_buf();
        let session = cx.new(|_| DocumentSession::without_pages(root, note.clone(), note.clone()));
        let workspace = cx.weak_entity();
        let providers: Vec<Rc<dyn Provider>> = vec![Rc::new(CitationKeys(workspace)), Rc::new(TypstCode::new(session.downgrade()))];
        let editor = cx.new(|cx| {
            let mut editor = SourceEditor::new(text.clone(), true, providers, cx);
            // Inside the panel: the viewer would hide what crosses onto it.
            editor.completion_w = PANEL_W - 40.;
            editor.set_session(session.clone());
            editor
        });
        session.update(cx, |session, cx| session.set_text(text, cx));
        let subscription = cx.subscribe(&editor, |w, editor, event: &SourceEvent, cx| {
            let Some(note) = w.papers.panel.notes.as_ref().map(|notes| notes.id.clone()) else { return };
            match event {
                SourceEvent::Changed => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(notes) = &w.papers.panel.notes { notes.session.update(cx, |session, cx| session.set_text(text, cx)); }
                    w.schedule_notes_save(cx);
                }
                SourceEvent::Completed(CompletionKind::Citation) => w.link_note_library(&note, &editor, cx),
                SourceEvent::Completed(_) | SourceEvent::Caret | SourceEvent::Follow(_) => {}
            }
        });
        self.papers.panel.notes = Some(Notes { id: note, editor, session, unsaved: false, error: None, save_task: None, _subscription: subscription });
        self.papers.panel.comment = None;
    }

    /// What a paper's notes begin with.
    fn notes_start(&self, paper: &str) -> String {
        format!("= {}\n\n", self.paper_title(paper))
    }

    fn schedule_notes_save(&mut self, cx: &mut Context<Self>) {
        let task = Self::save_later(Self::flush_notes, cx);
        if let Some(notes) = &mut self.papers.panel.notes {
            notes.unsaved = true;
            notes.save_task = Some(task);
        }
    }

    /// Writes the panel's notes if they changed. False when that failed: they
    /// stay pending, and a later save retries them.
    pub(crate) fn flush_notes(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(notes) = &mut self.papers.panel.notes else { return true };
        notes.save_task = None;
        if !notes.unsaved { return true; }
        let (id, text) = (notes.id.clone(), notes.editor.read(cx).text().to_string());
        let saved = self.save_note(&id, &text, cx);
        if saved {
            self.keep_version(&id, &text, false, cx);
            if let Some(notes) = &mut self.papers.panel.notes {
                notes.unsaved = false;
                notes.error = None;
            }
        }
        saved
    }

    /// Gives up the panel's pending notes, for Discard changes.
    pub(crate) fn discard_notes(&mut self) {
        if let Some(notes) = &mut self.papers.panel.notes {
            notes.unsaved = false;
            notes.error = None;
        }
    }

    /// The error of the last failed notes save, for the Save failed dialog.
    pub(crate) fn notes_error(&self) -> Option<&str> { self.papers.panel.notes.as_ref()?.error.as_deref() }

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
        if let Err(e) = &result {
            let error = format!("Could not save the notes: {e}");
            self.show_toast_kind(ToastKind::Error, &error, cx);
            // The editor already shows `text`, so it is pending whoever wrote it.
            if let Some(notes) = self.papers.panel.notes.as_mut().filter(|notes| notes.id == id) {
                notes.unsaved = true;
                notes.error = Some(error);
            }
        }
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
