//! The open document's text on its way to disk, and every other write to a
//! document. A failed save keeps the changes pending and asks before anything
//! replaces them. Everything else that fails says so in a toast.

use crate::app::{View, Workspace};
use crate::citations::CitationKeys;
use crate::completion::{CompletionKind, Found, Item, Provider, Query, Reach};
use crate::confirm::AfterSave;
use crate::preview::{Preview, PreviewEvent};
use crate::session::DocumentSession;
use crate::source::{SourceEditor, SourceEvent};
use crate::typst_code::TypstCode;
use thesio_core::store::Entry;
use crate::ui::ToastKind;
use gpui::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;
use thesio_core::images::Incoming;
use thesio_core::paths;
use thesio_core::references::{note_link, note_link_at, note_links};

#[derive(Clone, Debug, PartialEq)]
pub enum SaveStatus {
    /// Freshly opened, nothing saved this session: show the modification time.
    Idle,
    Saved,
    Saving,
    Failed(String),
}

/// The file in the editor, and everything that lasts as long as it stays open.
pub struct OpenDocument {
    /// Its vault path, which follows renames and moves.
    pub id: String,
    pub editor: Entity<SourceEditor>,
    /// Its compiles, which `preview` shows. Only Typst documents have them.
    pub session: Option<Entity<DocumentSession>>,
    pub preview: Option<Entity<Preview>>,
    pub status: SaveStatus,
    /// It has changes not yet on disk (cleared only by a successful save or by
    /// Discard changes).
    pub(crate) unsaved: bool,
    save_task: Option<Task<()>>,
    /// The header "…" button, which the document menu hangs from.
    pub(crate) more_button: Rc<Cell<Option<Bounds<Pixels>>>>,
    _subs: Vec<Subscription>,
}

const SAVE_DELAY: Duration = Duration::from_millis(500);

/// `[[note` links to the vault's other documents.
pub struct NoteLinks(pub WeakEntity<Workspace>);

impl Provider for NoteLinks {
    fn kind(&self) -> CompletionKind { CompletionKind::NoteLink }

    fn find(&self, text: &str, caret: usize, _: Reach) -> Option<Found> {
        let range = note_link_at(text, caret)?;
        Some(Found { query: text[range.start + 2..caret].to_lowercase(), range })
    }

    fn items(&self, query: Query, cx: &mut App) -> Task<Vec<Item>> {
        let workspace = self.0.clone();
        cx.spawn(async move |cx| workspace.update(cx, |w, _| w.note_link_items(&query.text)).unwrap_or_default())
    }
}

/// How text that did not come from typing enters an editor.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Edit {
    /// One step the user can undo.
    Undoable,
    /// Thesio's own text: outside the undo history, and not announced.
    Generated,
}

impl Workspace {
    /// The open document's id.
    pub(crate) fn current(&self) -> Option<&str> { self.document.as_ref().map(|document| document.id.as_str()) }

    pub(crate) fn editor(&self) -> Option<&Entity<SourceEditor>> { self.document.as_ref().map(|document| &document.editor) }

    /// Replaces the open document with `id`, which holds nothing unsaved.
    pub(crate) fn load_document(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_overlay(cx);
        if crate::papers::opens_in_viewer(id) {
            return self.open_paper(id, window, cx);
        }
        let loaded = self.store.borrow().load(id);
        let text = match loaded {
            Ok(text) => text,
            Err(e) => {
                // Binary files belong to the system's apps.
                let path = self.store.borrow().path(id);
                match path {
                    Ok(path) if !paths::is_document(id) => crate::system::open_with_system(&path, cx),
                    _ => self.show_toast_kind(ToastKind::Error, &format!("Could not open {id}: {e}"), cx),
                }
                return cx.notify();
            }
        };
        let typst = paths::is_document(id);
        let opened_text = text.clone();
        let root = self.store.borrow().root().to_path_buf();
        let main = self.top_document(id, cx);
        let session = typst.then(|| cx.new(|_| DocumentSession::with_pages(root, id.to_string(), main)));
        let workspace = cx.weak_entity();
        let mut providers: Vec<Rc<dyn Provider>> = vec![Rc::new(CitationKeys::new(workspace.clone(), session.as_ref().map(Entity::downgrade))), Rc::new(NoteLinks(workspace))];
        // Last: `@` and `[[` belong to the two above.
        if let Some(session) = &session { providers.push(Rc::new(TypstCode::new(session.downgrade()))); }
        let editor = cx.new(|cx| SourceEditor::new(text.clone(), typst, providers, cx));
        let preview = session.as_ref().map(|session| cx.new(|cx| Preview::new(session.clone(), cx)));
        if let Some(session) = &session {
            editor.update(cx, |editor, cx| editor.set_session(session.clone(), cx));
            session.update(cx, |session, cx| session.set_text(text, cx));
        }
        let mut subs = vec![cx.subscribe_in(&editor, window, Self::on_editor_event)];
        if let Some(preview) = &preview { subs.push(cx.subscribe_in(preview, window, Self::on_preview_event)); }
        editor.read(cx).focus(window);
        let touched = self.store.borrow_mut().touch(id);
        match touched {
            Ok(()) => if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) { e.meta.opened = thesio_core::time::now(); },
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not record opening {id}: {e}"), cx),
        }
        self.document = Some(OpenDocument { id: id.to_string(), editor, session, preview, status: SaveStatus::Idle, unsaved: false, save_task: None, more_button: Default::default(), _subs: subs });
        self.keep_version(id, &opened_text, true, cx);
        self.refresh_outline(cx);
        self.reveal_in_tree(id, cx);
        cx.notify();
    }

    fn on_editor_event(&mut self, editor: &Entity<SourceEditor>, event: &SourceEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SourceEvent::Changed => {
                let text = editor.read(cx).text().to_string();
                if let Some(session) = self.document.as_ref().and_then(|document| document.session.as_ref()) { session.update(cx, |session, cx| session.set_text(text, cx)); }
                self.schedule_save(cx);
            }
            SourceEvent::Caret => {
                let caret = editor.read(cx).caret();
                if let Some(preview) = self.document.as_ref().and_then(|document| document.preview.as_ref()) { preview.update(cx, |p, cx| p.follow(caret, cx)); }
                self.mark_outline(caret, cx);
            }
            SourceEvent::Follow(offset) => {
                let Some(id) = self.current().map(str::to_string) else { return };
                let text = editor.read(cx).text();
                if let Some(link) = note_links(text, &id).into_iter().find(|link| link.range.contains(offset)) { self.follow_link(&link.target, window, cx); }
            }
            SourceEvent::Completed(CompletionKind::Citation) => if let Some(id) = self.current().map(str::to_string) { self.citation_added(&id, cx); },
            SourceEvent::Completed(_) => {}
            SourceEvent::Images(images) => if let Some(id) = self.current().map(str::to_string) { self.add_images(&id, editor, images, cx); },
        }
    }

    /// Copies `images` beside `document` and puts their figures at the caret of
    /// `editor`, which shows it. One that cannot be copied says so and the rest go on.
    pub(crate) fn add_images(&mut self, document: &str, editor: &Entity<SourceEditor>, images: &[Incoming], cx: &mut Context<Self>) {
        let mut paths = vec![];
        for image in images {
            let added = self.store.borrow_mut().add_image(document, image);
            match added {
                Ok(path) => paths.push(path),
                Err(e) => self.show_toast_kind(ToastKind::Error, &format!("{e:#}"), cx),
            }
        }
        if paths.is_empty() { return; }
        self.refresh_entries(cx);
        editor.update(cx, |editor, cx| editor.insert_figures(&paths, cx));
    }

    fn on_preview_event(&mut self, _: &Entity<Preview>, event: &PreviewEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (path, offset) = match event {
            PreviewEvent::Note(path) => return self.follow_link(path, window, cx),
            PreviewEvent::Jump(offset) => (None, *offset),
            PreviewEvent::Source(path, offset) => (Some(path.clone()), *offset),
        };
        if let Some(path) = path.or_else(|| self.current().map(str::to_string)) { self.go_to_source(&path, offset, window, cx); }
    }

    /// What `[[` offers from the open document, matching titles first.
    fn note_link_items(&self, query: &str) -> Vec<Item> {
        let Some(file) = self.current() else { return vec![] };
        let mut notes: Vec<&Entry> = self.entries.iter().filter(|e| e.typst && e.id != file && (e.title.to_lowercase().contains(query) || e.id.to_lowercase().contains(query))).collect();
        notes.sort_by_key(|e| !e.title.to_lowercase().starts_with(query));
        notes.into_iter().map(|e| Item { kind: CompletionKind::NoteLink, title: e.title.clone(), detail: e.id.clone(), info: None, insert: note_link(file, &e.id, &e.title), selection: None, stops: vec![], from: None, tail: 0 }).collect()
    }

    /// What `id` holds now: the open editor's copy, or the file.
    pub(crate) fn text_of(&self, id: &str, cx: &App) -> anyhow::Result<String> {
        match &self.document {
            Some(document) if document.id == id => Ok(document.editor.read(cx).text().to_string()),
            _ => self.store.borrow().load(id),
        }
    }

    /// `text_of`, with a toast when it fails.
    pub(crate) fn read_document(&mut self, id: &str, cx: &mut Context<Self>) -> Option<String> {
        self.text_of(id, cx).map_err(|e| self.show_toast_kind(ToastKind::Error, &format!("{e:#}"), cx)).ok()
    }

    /// Gives `text` to the editors showing `id`.
    pub(crate) fn show_text(&mut self, id: &str, text: &str, edit: Edit, cx: &mut Context<Self>) {
        let open = self.document.as_ref().filter(|document| document.id == id).map(|document| document.editor.clone());
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
        let open = self.current() == Some(id);
        self.show_text(id, text, edit, cx);
        if open { self.set_unsaved(); }
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
            entry.modified = thesio_core::time::now();
        }
        Ok(())
    }

    fn set_unsaved(&mut self) {
        if let Some(document) = &mut self.document { document.unsaved = true; }
    }

    fn saved(&mut self) {
        if let Some(document) = &mut self.document {
            document.status = SaveStatus::Saved;
            document.unsaved = false;
        }
    }

    fn recompile(&mut self, cx: &mut Context<Self>) {
        let Some(document) = &self.document else { return };
        let Some(session) = document.session.clone() else { return };
        let text = document.editor.read(cx).text().to_string();
        session.update(cx, |session, cx| session.set_text(text, cx));
    }

    /// Compiles from where the open file and its top document are now. Either
    /// may have moved, and a top document in the Trash leaves the file on its own.
    pub(crate) fn follow_top_document(&mut self, cx: &mut Context<Self>) {
        let Some(document) = &self.document else { return };
        let Some(session) = document.session.clone() else { return };
        let (path, main) = (document.id.clone(), self.top_document(&document.id, cx));
        if session.update(cx, |session, _| session.set_path(path, main)) { self.recompile(cx); }
    }

    /// Writes the open document again after text changed without an edit.
    pub(crate) fn mark_unsaved(&mut self, cx: &mut Context<Self>) {
        self.set_unsaved();
        self.flush(cx);
    }

    /// Runs `save` once typing has stopped; dropping the task cancels it.
    pub(crate) fn save_later(save: fn(&mut Self, &mut Context<Self>) -> bool, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, save);
        })
    }

    pub(crate) fn schedule_save(&mut self, cx: &mut Context<Self>) {
        let task = Self::save_later(Self::flush, cx);
        if let Some(document) = &mut self.document {
            document.status = SaveStatus::Saving;
            document.unsaved = true;
            document.save_task = Some(task);
        }
        cx.notify();
    }

    /// Writes the open document to disk if it has unsaved changes. False when
    /// that failed: the changes stay pending (a later save retries them) and
    /// the top bar shows the error, or a toast away from the document.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(document) = &mut self.document else { return true };
        document.save_task = None;
        if !document.unsaved {
            return true;
        }
        let (id, text) = (document.id.clone(), document.editor.read(cx).text().to_string());
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
                if let Some(document) = &mut self.document { document.status = SaveStatus::Failed(error); }
            }
        }
        cx.notify();
        !self.document.as_ref().is_some_and(|document| document.unsaved)
    }

    /// Everything that waits for a save: the Library's panel, a paper's notes,
    /// and the open document. False when the notes or the document failed.
    pub(crate) fn save_all(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_inspector(cx);
        let notes = self.flush_notes(cx);
        self.flush(cx) && notes
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
    /// document and the notes no longer need saving.
    pub(crate) fn resolve_save_failed(&mut self, retry: bool, then: AfterSave, window: &mut Window, cx: &mut Context<Self>) {
        if retry {
            if !self.save_all(cx) { return self.ask_save_failed(then, window, cx); }
        } else {
            self.discard_notes();
            if let Some(document) = self.document.as_mut().filter(|document| document.unsaved) {
                document.unsaved = false;
                document.status = SaveStatus::Idle;
            }
        }
        then(self, window, cx);
        cx.notify();
    }

    /// The error of the last failed save, for the Save failed dialog.
    pub(crate) fn save_error(&self) -> String {
        match self.document.as_ref().map(|document| &document.status) {
            Some(SaveStatus::Failed(e)) => e.clone(),
            _ => self.notes_error().unwrap_or("Save failed").to_string(),
        }
    }
}
