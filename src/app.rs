//! Workspace window: two panes — the document list on the left
//! (see `sidebar.rs`) and the editor on the right, with the
//! 64px top bar (breadcrumb title + save status) above the page.

use crate::actions::FocusSearch;
use crate::confirm::Confirm;
use crate::document::Edit;
use crate::overlay::Overlay;
use crate::preview::{Preview, PreviewEvent};
use crate::session::DocumentSession;
use crate::source::{SourceEditor, SourceEvent};
use crate::completion::{Citations, CompletionKind, NoteLinks, Provider};
use crate::input::{TextField, TextFieldEvent};
use crate::store::display_title;
use crate::store::Entry;
use crate::shared_store::SharedStore;
use crate::theme::{self, Theme, craft};
use thesio_core::paths;
use thesio_core::project::{self, Heading, Project};
use thesio_core::references::note_links;
use crate::ui::{Icon, TipExt, TipSide, TitleBarExt, ToastKind, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Asks the active window to close, for a close that arrives outside its element tree.
pub(crate) fn close_active_window(cx: &mut App) {
    let Some(handle) = cx.active_window().and_then(|w| w.downcast::<Workspace>()) else { return };
    // Key dispatch already borrows this window; close after it returns.
    cx.defer(move |cx| {
        if let Err(e) = handle.update(cx, |ws, window, cx| if ws.ready_to_close(window, cx) { window.remove_window(); }) { eprintln!("Could not close document window: {e}"); }
    });
}

/// Quit: the open document is saved first; when that fails its dialog asks
/// (Retry / Discard changes) and quits afterwards.
pub(crate) fn quit(cx: &mut App) {
    let ready = cx
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<Workspace>())
        // A window that is gone has nothing left to save.
        .all(|w| w.update(cx, |ws, window, cx| ws.save_before(window, cx, |_, _, cx| cx.quit())).unwrap_or(true));
    if ready {
        cx.quit();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SaveStatus {
    /// Freshly opened, nothing saved this session: show the modification time.
    Idle,
    Saved,
    Saving,
    Failed(String),
}

/// What the main pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Doc,
    Home,
    Library,
    Sources,
    /// A paper, inside the app: `Workspace::paper`.
    Paper,
    Trash,
    Settings,
}

/// Inline rename of a sidebar row.
pub struct Rename {
    pub id: String,
    pub field: Entity<TextField>,
    _subs: Vec<Subscription>,
}

pub struct Workspace {
    pub settings: crate::settings::Settings,
    native_sidebar: crate::native_header::NativeSidebar,
    pub(crate) papers: crate::papers::Papers,
    pub store: SharedStore,
    pub entries: Vec<Entry>,
    pub current: Option<String>,
    pub editor: Option<Entity<SourceEditor>>,
    /// The open document's compiles, which `preview` shows.
    pub session: Option<Entity<DocumentSession>>,
    pub preview: Option<Entity<Preview>>,
    pub citations: crate::citations::Citations,
    pub search: Entity<TextField>,
    /// Keyboard highlight inside search results.
    pub search_sel: Option<usize>,
    /// Highlights by paper, for search.
    pub paper_index: HashMap<String, crate::search::IndexedPaper>,
    pub indexing_papers: bool,
    pub sidebar_open: bool,
    /// Dragged width (300 by default, 268 to min(800, window − 540)).
    pub sidebar_width: f32,
    /// The resize handle is being dragged.
    pub sidebar_drag: bool,
    pub status: SaveStatus,
    pub(crate) overlay: Option<crate::overlay::Overlay>,
    /// Where focus was when the overlay opened.
    pub(crate) overlay_return: Option<FocusHandle>,
    pub rename: Option<Rename>,
    pub list_scroll: ScrollHandle,
    pub search_scroll: ScrollHandle,
    /// Selection mode: checked documents in My documents, in check order.
    pub batch: Option<Vec<String>>,
    pub view: View,
    pub home: crate::home::Home,
    /// Trash rows, refreshed when the page opens or changes.
    pub trash: Vec<crate::store::TrashEntry>,
    /// The toast and when it appeared.
    pub toast: Option<(ToastKind, SharedString, Instant)>,
    toast_task: Option<Task<()>>,
    /// Folded sidebar sections: Pinned, My documents.
    pub folded: [bool; 4],
    /// Documents that link to the open one: path, title, and where the link is.
    pub backlinks: Vec<(String, String, usize)>,
    /// The open document's headings, through the files it includes.
    pub outline: Vec<Heading>,
    /// The document the outline starts from.
    pub outline_root: Option<String>,
    /// The heading the caret is under.
    pub outline_active: Option<usize>,
    /// Expanded pages of the My documents tree.
    pub expanded: HashSet<String>,
    /// The header "…" button, which the doc menu hangs from.
    pub more_btn_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) focus: FocusHandle,
    pub(crate) save_task: Option<Task<()>>,
    /// The open document has changes not yet on disk (cleared only by a
    /// successful save or by Discard changes).
    pub(crate) unsaved: bool,
    editor_subs: Vec<Subscription>,
    _subs: Vec<Subscription>,
}



impl Workspace {
    pub fn new(document: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::preferences::load();
        if !cx.has_global::<SharedStore>() { cx.set_global(SharedStore::open().expect("open document library")); }
        let store = cx.global::<SharedStore>().clone();
        let entries = store.borrow_mut().list();
        let search = cx.new(|cx| {
            let mut f = TextField::new("Search", cx);
            f.placeholder_color = Theme::text_disabled;
            f
        });
        let subs = vec![
            cx.observe_window_bounds(window, |_, window, cx| crate::window_size::remember(window, cx)),
            // The system switched between light and dark (or the app's own look changed).
            cx.observe_window_appearance(window, |_, _, cx| if crate::appearance::sync_dark(cx) { crate::appearance::refresh_all(cx); }),
            cx.observe_window_activation(window, |this, window, cx| {
                // The accent has no change event in GPUI, and changing it means visiting System Settings.
                if window.is_window_active() { if crate::theme::sync_accent() { crate::appearance::refresh_all(cx); } this.refresh_entries(cx); this.reload_library(); this.table_changed(window, cx); cx.notify(); }
                else { this.save_all(cx); }
            }),
            cx.subscribe_in(&search, window, |this, _, ev: &TextFieldEvent, window, cx| {
                match ev {
                    TextFieldEvent::Changed => {
                        this.search_sel = this.default_search_sel(cx);
                        this.search_scroll.set_offset(point(px(0.), px(0.)));
                    }
                    TextFieldEvent::Up => this.move_search_sel(-1, cx),
                    TextFieldEvent::Down => this.move_search_sel(1, cx),
                    TextFieldEvent::Submit => {
                        if let Some(ix) = this.search_sel {
                            this.pick_search(ix, window, cx)
                        }
                    }
                    _ => {}
                }
                cx.notify();
            }),
            // Last resort (the system logging out): ⌘Q and the close button
            // go through `save_before` and ask when the save fails.
            cx.on_app_quit(|this, cx| {
                crate::window_size::persist(&this.store, cx);
                this.save_all(cx);
                async {}
            }),
        ];
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            // A window that is gone has nothing left to save.
            this.update(cx, |w, cx| w.ready_to_close(window, cx)).unwrap_or(true)
        });
        let settings = store.borrow().settings();
        let sidebar_width = settings.sidebar_width.unwrap_or(theme::SIDEBAR_WIDTH);
        let store_expanded: HashSet<String> = settings.expanded.into_iter().collect();
        let citations = crate::citations::Citations::new(store.borrow().root(), window, cx);
        let mut ws = Workspace {
            settings: crate::settings::Settings::new(cx),
            store,
            entries,
            current: None,
            editor: None,
            session: None,
            preview: None,
            citations,
            search,
            search_sel: None,
            paper_index: HashMap::new(),
            indexing_papers: false,
            sidebar_open: true,
            sidebar_width,
            sidebar_drag: false,
            status: SaveStatus::Idle,
            overlay: None,
            overlay_return: None,
            rename: None,
            list_scroll: ScrollHandle::new(),
            search_scroll: ScrollHandle::new(),
            batch: None,
            view: View::Doc,
            home: Default::default(),
            trash: vec![],
            toast: None,
            toast_task: None,
            more_btn_bounds: Rc::new(Cell::new(None)),
            native_sidebar: Default::default(),
            papers: Default::default(),
            folded: [false; 4],
            backlinks: vec![],
            outline: vec![],
            outline_root: None,
            outline_active: None,
            expanded: store_expanded,
            focus: cx.focus_handle(),
            save_task: None,
            unsaved: false,
            editor_subs: vec![],
            _subs: subs,
        };
        ws.sort();
        match document.or_else(|| ws.first_document()) {
            Some(id) => ws.open(&id, window, cx),
            None => ws.new_doc(window, cx),
        }
        let warnings = ws.store.take_opening_warnings();
        ws.show_warnings(&warnings, cx);
        ws
    }

    /// Most recently modified first.
    pub(crate) fn sort(&mut self) {
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.modified));
    }

    /// Reads the vault's entries again. A paper that went leaves its view.
    pub(crate) fn refresh_entries(&mut self, cx: &mut Context<Self>) {
        self.entries = self.store.borrow_mut().list();
        self.sort();
        if self.view == View::Paper && !self.papers.current.as_ref().is_some_and(|id| self.entries.iter().any(|e| &e.id == id)) { self.set_view(View::Sources, cx); }
    }

    pub fn query(&self, cx: &App) -> String {
        self.search.read(cx).text().trim().to_lowercase()
    }

    /// Shows `id`, once the open document is safely saved.
    pub fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let paper = id.ends_with(".pdf") && crate::native_pdf::supported();
        if self.view != View::Doc && !paper { self.set_view(View::Doc, cx); }
        if self.current.as_deref() == Some(id) {
            return;
        }
        if self.entries.iter().any(|e| e.id == id && e.folder) {
            return self.toggle_folder(id, cx);
        }
        let target = id.to_string();
        self.when_saved(window, cx, move |w, window, cx| w.load_document(&target, window, cx));
    }

    /// Replaces the open document with `id`, which holds nothing unsaved.
    fn load_document(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_overlay(cx);
        if id.ends_with(".pdf") && crate::native_pdf::supported() {
            return self.open_paper(id, window, cx);
        }
        let loaded = self.store.borrow().load(id);
        let text = match loaded {
            Ok(text) => text,
            Err(e) => {
                // Binary files belong to the system's apps; scripted runs must not start any.
                let path = self.store.borrow().path(id);
                match path {
                    Ok(path) if !paths::is_document(id) && std::env::var_os("THESIO_SCRIPT").is_none() => cx.open_with_system(&path),
                    _ => self.show_toast_kind(ToastKind::Error, &format!("Could not open {id}: {e}"), cx),
                }
                return cx.notify();
            }
        };
        let typst = paths::is_document(id);
        let opened_text = text.clone();
        let workspace = cx.weak_entity();
        let providers: Vec<Rc<dyn Provider>> = vec![Rc::new(Citations(workspace.clone())), Rc::new(NoteLinks(workspace))];
        let editor = cx.new(|cx| SourceEditor::new(text.clone(), typst, providers, cx));
        let root = self.store.borrow().root().to_path_buf();
        let main = self.top_document(id, cx);
        let session = typst.then(|| cx.new(|_| DocumentSession::with_pages(root, id.to_string(), main)));
        let preview = session.as_ref().map(|session| cx.new(|cx| Preview::new(session.clone(), cx)));
        if let Some(session) = &session {
            editor.update(cx, |editor, _| editor.set_session(session.clone()));
            session.update(cx, |session, cx| session.set_text(text, cx));
        }
        self.editor_subs = vec![
            cx.subscribe_in(&editor, window, |this, editor, event: &SourceEvent, window, cx| match event {
                SourceEvent::Changed => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(session) = &this.session { session.update(cx, |session, cx| session.set_text(text, cx)); }
                    this.schedule_save(window, cx);
                }
                SourceEvent::Caret => {
                    let caret = editor.read(cx).caret();
                    if let Some(preview) = &this.preview { preview.update(cx, |p, cx| p.follow(caret, cx)); }
                    this.mark_outline(caret, cx);
                }
                SourceEvent::Follow(offset) => {
                    let Some(id) = this.current.clone() else { return };
                    let text = editor.read(cx).text();
                    if let Some(link) = note_links(text, &id).into_iter().find(|link| link.range.contains(offset)) { this.follow_link(&link.target, window, cx); }
                }
                SourceEvent::Completed(CompletionKind::Citation) => if let Some(id) = this.current.clone() { this.citation_added(&id, cx); },
                SourceEvent::Completed(_) => {}
            }),
        ];
        if let Some(preview) = &preview {
            self.editor_subs.push(cx.subscribe_in(preview, window, |this, _, ev: &PreviewEvent, window, cx| {
                let (path, offset) = match ev {
                    PreviewEvent::Note(path) => return this.follow_link(path, window, cx),
                    PreviewEvent::Jump(offset) => (None, *offset),
                    PreviewEvent::Source(path, offset) => (Some(path.clone()), *offset),
                };
                let path = path.or_else(|| this.current.clone());
                if let Some(path) = path { this.go_to_source(&path, offset, window, cx); }
            }));
        }
        editor.read(cx).focus(window);
        let touched = self.store.borrow_mut().touch(id);
        match touched {
            Ok(()) => if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) { e.meta.opened = crate::time::now(); },
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not record opening {id}: {e}"), cx),
        }
        self.editor = Some(editor);
        self.session = session;
        self.preview = preview;
        self.current = Some(id.to_string());
        self.keep_version(id, &opened_text, true, cx);
        self.outline_root = typst.then(|| self.top_document(id, cx));
        self.refresh_outline(cx);
        self.reveal_in_tree(id, cx);
        self.status = SaveStatus::Idle;
        cx.notify();
    }

    /// Follows a rename or move through everything that holds a vault path.
    /// The open document's editor and undo history stay.
    pub(crate) fn path_changed(&mut self, old: &str, new: &str, rewritten: &[String], cx: &mut Context<Self>) {
        if let Some(path) = self.papers.current.as_deref().and_then(|paper| paths::moved(paper, old, new)) { self.papers.current = Some(path); }
        self.refresh_entries(cx);
        let moved = self.current.as_deref().and_then(|c| paths::moved(c, old, new));
        if let Some(path) = moved {
            self.current = Some(path.clone());
            let main = self.top_document(&path, cx);
            self.outline_root = paths::is_document(&path).then(|| main.clone());
            if let Some(session) = &self.session { session.update(cx, |session, _| session.set_path(path, main)); }
        }
        self.papers.panel.path_changed(old, new);
        self.paper_index = std::mem::take(&mut self.paper_index).into_iter().map(|(id, paper)| (paths::moved(&id, old, new).unwrap_or(id), paper)).collect();
        if let Some(batch) = &mut self.batch {
            for id in batch.iter_mut() {
                if let Some(path) = paths::moved(id, old, new) { *id = path; }
            }
        }
        let renamed: Vec<String> = self.expanded.iter().filter(|e| paths::is_within(e, old)).cloned().collect();
        for path in renamed {
            self.expanded.remove(&path);
            self.expanded.insert(format!("{new}{}", &path[old.len()..]));
        }
        self.save_expanded(cx);
        // Their paths changed on disk: open editors take them in.
        for id in rewritten {
            let text = self.store.borrow().load(id);
            if let Ok(text) = text { self.show_text(id, &text, Edit::Undoable, cx); }
        }
        self.refresh_outline(cx);
    }

    /// Goes to a place in a document, opening it if another one is open.
    pub(crate) fn go_to_source(&mut self, path: &str, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.current.as_deref() != Some(path) { self.open(path, window, cx); }
        if self.current.as_deref() != Some(path) { return; }
        self.set_view(View::Doc, cx);
        if let Some(editor) = &self.editor { editor.update(cx, |e, cx| e.go_to(offset, window, cx)); }
    }

    /// Opens a linked file, or says that it is gone.
    pub(crate) fn follow_link(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.entries.iter().any(|e| e.id == path) { self.open(path, window, cx); } else { self.show_toast_kind(ToastKind::Error, &format!("{path} does not exist"), cx); }
    }

    /// Reads the outline again: the open file from its editor, others from memory.
    pub(crate) fn refresh_outline(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.outline_root.clone() else {
            self.outline.clear();
            self.backlinks.clear();
            return;
        };
        let open = self.current.clone().zip(self.editor.as_ref().map(|editor| editor.read(cx).text().to_string()));
        self.backlinks = self.current.as_ref().map_or(vec![], |current| self.entries.iter().filter(|e| e.typst && e.id != *current)
            .filter_map(|e| note_links(&e.body, &e.id).into_iter().find(|link| link.target == *current).map(|link| (e.id.clone(), e.title.clone(), link.range.start))).collect());
        let entries = &self.entries;
        self.outline = Project::open(&root, &mut |path| match &open {
            Some((id, text)) if id == path => Some(text.clone()),
            _ => entries.iter().find(|e| e.id == path).map(|e| e.body.clone()),
        }).outline();
        let caret = self.editor.as_ref().map_or(0, |editor| editor.read(cx).caret());
        self.mark_outline(caret, cx);
    }

    /// Marks the heading the caret is under.
    fn mark_outline(&mut self, caret: usize, cx: &mut Context<Self>) {
        let current = self.current.as_deref();
        let active = self.outline.iter().rposition(|h| Some(h.path.as_str()) == current && h.offset <= caret);
        if active != self.outline_active {
            self.outline_active = active;
            cx.notify();
        }
    }

    fn first_document(&self) -> Option<String> {
        project::newest(self.entries.iter().filter(|e| e.typst).map(|e| (e.id.as_str(), e.modified))).map(str::to_string)
    }

    pub fn new_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.when_saved(window, cx, |w, window, cx| w.create_doc(None, window, cx));
    }

    /// A new document in `parent`, opened.
    pub(crate) fn create_doc(&mut self, parent: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.store.borrow_mut().create(parent);
        match result {
            Ok(id) => {
                self.refresh_entries(cx);
                self.search.update(cx, |s, cx| s.set_text("", cx));
                if let Some(parent) = parent { self.set_expanded(parent, true, cx); }
                self.open(&id, window, cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not create document: {e}"), cx),
        }
        cx.notify();
    }

    /// A new folder, with its name ready to edit.
    pub fn new_folder(&mut self, parent: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.store.borrow_mut().create_folder(parent, "New folder");
        match result {
            Ok(id) => {
                self.refresh_entries(cx);
                self.start_rename(&id, window, cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not create folder: {e}"), cx),
        }
        cx.notify();
    }

    /// Ask before deleting: a confirmation, then the Trash.
    pub fn ask_delete(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_overlay(Overlay::Confirm(Confirm::Delete { ids: vec![id.to_string()], batch: false }), window, cx);
    }

    /// Moves files and folders to the Trash; if the open one goes, opens the next.
    pub fn delete_docs(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        if self.current.as_ref().is_some_and(|c| ids.iter().any(|id| paths::is_within(c, id))) {
            let ids = ids.to_vec();
            self.when_saved(window, cx, move |w, window, cx| w.trash_docs(&ids, window, cx));
        } else {
            self.trash_docs(ids, window, cx);
        }
    }

    fn trash_docs(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        let inside = |doc: &str, id: &String| paths::is_within(doc, id);
        // An item inside another selected folder goes with it.
        let topmost: Vec<&String> = ids.iter().filter(|id| !ids.iter().any(|o| o != *id && paths::is_within(id, o))).collect();
        let mut deleted: Vec<String> = vec![];
        let mut errors = vec![];
        for id in topmost {
            let result = self.store.borrow_mut().delete(id);
            match result {
                Ok(warnings) => {
                    deleted.push(id.clone());
                    errors.extend(warnings);
                }
                Err(e) => errors.push(format!("Could not delete document: {e}")),
            }
        }
        if self.current.as_ref().is_some_and(|c| deleted.iter().any(|id| inside(c, id))) {
            self.save_task = None;
            self.unsaved = false;
            self.current = None;
            self.editor = None;
            self.session = None;
            self.preview = None;
            self.editor_subs.clear();
        }
        self.refresh_entries(cx);
        if let Some(b) = self.batch.as_mut() {
            b.retain(|id| !deleted.iter().any(|d| inside(id, d)));
        }
        if self.current.is_none() {
            // Stay on Home or Trash if that's where the delete came from.
            let view = self.view;
            match self.first_document() {
                Some(next) => self.load_document(&next, window, cx),
                None => self.new_doc(window, cx),
            }
            self.set_view(view, cx);
        }
        if self.view == View::Trash {
            errors.extend(self.store.borrow_mut().purge_expired());
            self.trash = self.store.borrow().list_trash();
        }
        if errors.is_empty() {
            self.show_toast("Deleted successfully", cx);
        } else {
            self.show_toast_kind(ToastKind::Error, &errors.join("; "), cx);
        }
        cx.notify();
    }

    /// Asks where to save, then compiles the document's current text to a PDF.
    pub fn export_pdf(&mut self, id: &str, cx: &mut Context<Self>) {
        let Ok(text) = self.text_of(id, cx) else { return };
        let name = format!("{}.pdf", self.entries.iter().find(|e| e.id == id).map_or(crate::store::UNTITLED, |e| e.title.as_str()));
        let (root, id) = (self.store.borrow().root().to_path_buf(), id.to_string());
        let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
        let prompt = picked.is_none().then(|| cx.prompt_for_new_path(&root, Some(&name)));
        cx.spawn(async move |this, cx| {
            let path = match prompt {
                Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(path) = path else { return };
            let result = cx.background_spawn(async move {
                let mut compiler = thesio_core::compiler::Compiler::new(root);
                compiler.set_buffer(&id, text)?;
                let compilation = compiler.compile(&id)?;
                let Some(document) = compilation.document else {
                    let first = compilation.diagnostics.into_iter().find(|d| d.error).map_or("unknown error".to_string(), |d| d.message);
                    anyhow::bail!("{first}");
                };
                std::fs::write(&path, thesio_core::compiler::export_pdf(&document)?)?;
                anyhow::Ok(())
            }).await;
            let _ = this.update(cx, |w, cx| match result {
                Ok(()) => w.show_toast("Exported PDF", cx),
                Err(e) => w.show_toast_kind(ToastKind::Error, &format!("Could not export PDF: {e}"), cx),
            });
        }).detach();
    }

    /// Copies chosen files into the vault and opens the first document.
    pub fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: None, extensions: None }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt {
                Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(paths) = paths else { return };
            let _ = this.update_in(cx, |w, window, cx| {
                let mut first = None;
                for path in &paths {
                    let result = w.store.borrow_mut().import(path);
                    match result {
                        Ok(id) => if paths::is_document(&id) { first.get_or_insert(id); },
                        Err(e) => w.show_toast_kind(ToastKind::Error, &format!("Could not import {}: {e}", path.display()), cx),
                    }
                }
                w.refresh_entries(cx);
                if let Some(id) = first { w.open(&id, window, cx); }
                cx.notify();
            });
        }).detach();
    }

    pub fn toggle_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return };
        let on = !e.meta.pinned;
        let result = self.store.borrow_mut().set_pinned(id, on);
        if result.is_ok() { e.meta.pinned = on; }
        match result {
            Ok(()) => self.show_toast(if on { "Pinned" } else { "Unpinned" }, cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not pin {id}: {e}"), cx),
        }
        cx.notify();
    }

    /// Favorites / Remove from favorites (listed under Home → Favorites).
    pub fn toggle_star(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return };
        let on = e.meta.starred == 0;
        let result = self.store.borrow_mut().set_starred(id, on);
        if let Ok(time) = result { e.meta.starred = time; }
        match result {
            Ok(_) => self.show_toast(if on { "Added to favorites" } else { "Removed from favorites" }, cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not change favorites: {e}"), cx),
        }
        cx.notify();
    }

    /// Switch the main pane to Home or Trash (or back to the document).
    pub fn show_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        if view == View::Trash {
            self.refresh_trash(cx);
        }
        if matches!(view, View::Library | View::Sources) {
            self.save_all(cx);
            self.reload_library();
            // Rebuilt below, with what documents cite now.
            self.citations.close_inspector();
        }
        self.set_view(view, cx);
        self.table_changed(window, cx);
    }

    /// What every change of the main pane goes through: an overlay belongs to
    /// the page it opened on, and the native viewer only lives on Paper.
    pub(crate) fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.clear_overlay(cx);
        self.view = view;
        if view != View::Paper { self.papers.pdf.close(); }
    }

    pub(crate) fn show_warnings(&mut self, warnings: &[String], cx: &mut Context<Self>) {
        if !warnings.is_empty() {
            self.show_toast_kind(crate::ui::ToastKind::Error, &warnings.join("; "), cx);
        }
    }

    /// A success toast (Deleted successfully, Moved successfully …).
    pub fn show_toast(&mut self, msg: &str, cx: &mut Context<Self>) {
        self.show_toast_kind(ToastKind::Success, msg, cx);
    }

    pub fn show_toast_kind(&mut self, kind: ToastKind, msg: &str, cx: &mut Context<Self>) {
        self.toast = Some((kind, SharedString::from(msg.to_string()), Instant::now()));
        self.toast_task = Some(cx.spawn(async |this, cx| {
            cx.background_executor().timer(Duration::from_millis(2700)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
        cx.notify();
    }

    fn render_toast(&self, window: &mut Window) -> Option<AnyElement> {
        let (kind, msg, t0) = self.toast.as_ref()?;
        crate::ui::toast_el(*kind, msg, *t0, if self.view == View::Paper { TOAST_ON_PAPER } else { 32. }, window)
    }

    /// Rename: the row's title turns into an input; Enter or clicking away saves, Esc cancels.
    pub fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.entries.iter().find(|e| e.id == id).map(|e| e.title.trim().to_string()) else { return };
        let field = cx.new(|cx| {
            let mut f = TextField::new(crate::store::UNTITLED, cx);
            f.set_text(&title, cx);
            f.select_all_text(cx);
            f
        });
        let focus = field.focus_handle(cx);
        window.focus(&focus);
        let subs = vec![
            cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| match ev {
                TextFieldEvent::Submit => w.finish_rename(true, window, cx),
                TextFieldEvent::Cancel => w.finish_rename(false, window, cx),
                _ => {}
            }),
            cx.on_blur(&focus, window, |w, window, cx| w.finish_rename(true, window, cx)),
        ];
        self.rename = Some(Rename { id: id.to_string(), field, _subs: subs });
        // From Home the row may be hidden: show it in the sidebar.
        self.sidebar_open = true;
        self.reveal_in_tree(id, cx);
        cx.notify();
    }

    fn finish_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rename.take() else { return };
        let refocus = r.field.read(cx).is_focused(window);
        let title = r.field.read(cx).text().trim().to_string();
        let old = self.entries.iter().find(|e| e.id == r.id).map(|e| e.title.trim().to_string());
        if commit && old.is_some_and(|o| o != title) {
            self.flush(cx);
            let result = self.store.borrow_mut().rename(&r.id, &title);
            match result {
                Ok(moved) => {
                    self.path_changed(&r.id, &moved.id, &moved.rewritten, cx);
                    if !moved.rewritten.is_empty() { self.show_toast(&updated_paths(moved.rewritten.len()), cx); }
                    self.show_warnings(&moved.warnings, cx);
                }
                Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not rename document: {e}"), cx),
            }
        }
        if refocus
            && let Some(ed) = &self.editor {
                ed.read(cx).focus(window);
            }
        cx.notify();
    }



    /// Home's search box, centred on the pane but never over what sits on
    /// either side of it: the title (and, with the sidebar hidden, the
    /// traffic lights and Show sidebar) on the left, + on the right. Below a
    /// usable width it is None and the caller shows a search icon instead.
    pub(crate) fn header_search(&self, window: &Window, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let main = f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let title = "Home".to_string();
        let title_w = f32::from(window.text_system().shape_line(title.clone().into(), px(15.), &[window.text_style().highlight(HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() }).to_run(title.len())], None).width);
        // page_header's left inset (26 with the sidebar, else the traffic
        // lights plus the 4 + 28 + 12 of Show sidebar), then 16 clear of the title.
        let left = if self.sidebar_open { 26. } else { theme::TRAFFIC_LIGHTS_W + 44. + if cfg!(target_os = "windows") { 32. } else { 0. } } + title_w + 16.;
        // + is 32 wide with an 8pt inset; 16 clear of it.
        let right = HEADER_ACTIONS_H + crate::ui::header_right_inset() + 16.;
        let half = (main / 2. - left).min(main / 2. - right);
        let width = (main - 320.).min(2. * half).min(360.);
        if width < 120. { return None }
        Some(div().id("header-search").absolute().left(px((main - width) / 2.)).top(px(11.))
            .w(px(width)).h(px(crate::ui::BUTTON_H)).no_drag().flex().items_center().gap(px(6.)).px(px(9.))
            .rounded(px(crate::ui::BUTTON_RADIUS)).border_1().border_color(Theme::line_border())
            .cursor_pointer()
            .child(icon(Icon::Search, 13., Theme::text_caption()))
            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(Theme::text_caption()).child("Search documents…"))
            .when_some(crate::shortcuts::hint(&FocusSearch, cx).filter(|_| width >= 200.), |d, keys| d.child(div().flex_none().text_size(px(11.)).text_color(Theme::text_placeholder()).child(keys.replace(" + ", " "))))
            .tip_action("Search documents", &FocusSearch, cx)
            .on_click(cx.listener(|w, _, window, cx| w.open_search(window, cx))))
    }

    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    /// Opens search; the tooltip names the key.
    pub(crate) fn search_button(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let tip = crate::shortcuts::hint(&FocusSearch, cx).map_or("Search".to_string(), |keys| format!("Search ({keys})"));
        crate::ui::header_icon(id, Icon::Search).tip_label(tip, TipSide::Below).on_click(cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(FocusSearch), cx)))
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.current.as_ref().and_then(|id| self.entries.iter().find(|e| &e.id == id)).map(|e| display_title(&e.title)).unwrap_or_default();
        let save_error = match &self.status {
            SaveStatus::Failed(error) => Some(error.clone()),
            _ => None,
        };
        // Independent icon controls, with equal top and right insets.
        let seg = crate::ui::header_icon;
        let divider = || div().flex_none().w(px(1.)).h(px(18.)).bg(Theme::line_divider());
        // Breadcrumbs: the parent pages, each linking to itself, with the
        // middle ones folded into "…" when the chain is long.
        let chain: Vec<(String, String)> = self
            .current
            .as_ref()
            .map(|id| self.ancestors(id).iter().map(|e| (e.id.clone(), display_title(&e.title))).collect())
            .unwrap_or_default();
        let mut crumbs: Vec<AnyElement> = vec![];
        let sep = || div().mx(px(3.)).flex_none().child(icon(Icon::ChevronRight, 12., theme::c(craft::N500))).into_any_element();
        for (i, (id, name)) in chain.iter().enumerate() {
            if chain.len() > 2 && i > 0 && i < chain.len() - 1 {
                if i == 1 {
                    let hidden: Vec<&str> = chain[1..chain.len() - 1].iter().map(|c| c.1.as_str()).collect();
                    crumbs.push(
                        div()
                            .id("crumb-more")
                            .no_drag()
                            .flex_none()
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .text_color(Theme::text_caption())
                            .tip(&hidden.join(" / "), None)
                            .child("…")
                            .into_any_element(),
                    );
                    crumbs.push(sep());
                }
                continue;
            }
            let id = id.clone();
            crumbs.push(
                div()
                    .id(SharedString::from(format!("crumb-{id}")))
                    .no_drag()
                    .min_w(px(28.))
                    .max_w(px(200.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(Theme::text_caption())
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(name.clone())))
                    .on_click(cx.listener(move |w, _, window, cx| w.open(&id, window, cx)))
                    .into_any_element(),
            );
            crumbs.push(sep());
        }
        let record_more = self.more_btn_bounds.clone();
        let doc_menu = matches!(self.overlay, Some(Overlay::DocMenu));
        div()
            .id("top-bar").relative()
            .h(px(HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .pr(px(crate::ui::header_right_inset()))
            // Clear of the traffic lights, which sit over the note when the sidebar is closed.
            .pl(if self.sidebar_open { px(18.) } else { px(theme::TRAFFIC_LIGHTS_W) })
            .title_bar()
            .when(!self.sidebar_open, |d| {
                d.child(self.sidebar_toggle("open-sidebar", Theme::icon(), cx).ml(px(4.)))
                .child(self.home_button("header-home", 18., cx).ml(px(4.)).active(|s| s.bg(Theme::fill_pressed())).tip("Back to Home", None))
                .child(div().mx(px(12.)).child(divider()))
            })
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .h(px(20.))
                            .min_w_0()
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .children(crumbs)
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(ui_text(title)),
                            ),

                    )
                    .when_some(save_error, |d, error| d.child(
                        div().min_w_0().max_w(px(220.)).text_size(px(12.)).line_height(px(18.))
                            .text_color(theme::c(craft::R400)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(error),
                    )),
            )
            .child(div().flex_1().min_w(px(16.)))
            .child(
                crate::ui::toolbar_group()
                    .when(self.current.is_some(), |d| {
                        d.child(
                            // No tooltip while its dropdown is open; the menu hangs
                            // from this button's right edge.
                            seg("doc-more", Icon::Ellipsis)
                                .relative()
                                .when(doc_menu, |d| d.bg(Theme::fill_hover()))
                                .when(!doc_menu, |d| d.tip("More", None))
                                .child(canvas(move |b, _, _| record_more.set(Some(b)), |_, _, _, _| {}).absolute().inset_0())
                                .on_click(cx.listener(move |w, _, window, cx| {
                                    if doc_menu { w.close_overlay(window, cx); } else { w.open_overlay(Overlay::DocMenu, window, cx); }
                                })),
                        )
                    })
                    .child(self.search_button("doc-search", cx))
                    .child(
                        // Straight to a new document: no New menu, as Import is rare.
                        seg("doc-new", Icon::Plus).tip("New document", None).on_click(cx.listener(|w, _, window, cx| w.new_doc(window, cx))),
                    ).into_any_element()
            )
    }
}

/// The toast after a rename or move rewrote other files.
pub(crate) fn updated_paths(files: usize) -> String {
    format!("Updated paths in {files} {}", if files == 1 { "file" } else { "files" })
}

/// Header and action group share an 8px top/right inset.
pub const HEADER_H: f32 = 48.;
/// Toasts stay in the header over a paper: the native viewer would hide them lower down.
const TOAST_ON_PAPER: f32 = 4.;
pub(crate) const HEADER_ACTIONS_H: f32 = 32.;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.native_sidebar.sync(if self.sidebar_open { self.sidebar_w(window) } else { 0. }, window);
        let sidebar = if self.sidebar_open { Some(if matches!(self.view, View::Settings | View::Trash) { self.render_settings_sidebar(window, cx) } else { self.render_sidebar(window, cx) }) } else { None };
        let resize = self.sidebar_open.then(|| self.render_resize_handle(window, cx));
        let resize_overlay = self.render_resize_overlay(cx);
        let toast = self.render_toast(window);
        let main = match self.view {
            View::Doc => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .child(self.top_bar(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .children(self.editor.clone()),
                        )
                        .when_some(self.preview.clone(), |d, preview| d.child(div().relative().flex_1().min_w_0().h_full().child(preview))),
                )
                .into_any_element(),
            View::Home => self.render_home(window, cx),
            View::Library => self.render_library(window, cx),
            View::Sources => self.render_sources(cx),
            View::Paper => self.render_paper(window, cx),
            View::Trash => self.render_trash(cx),
            View::Settings => self.render_settings(window, cx),
        };
        let overlay = self.render_overlay(window, cx);
        let column_drag = self.render_column_drag(cx);
        // Only the note stands out: a white plane flush with the window's top,
        // right and bottom edges. The sidebar sits sunken on the window
        // surface beside it; the change of value is the only line between
        // them (no gutter, radius, border or shadow).
        let hairline = px(1. / window.scale_factor());
        let note = div().id("note").relative().flex_1().min_w_0().h_full().flex().bg(Theme::bg_body()).child(main)
            .child(div().absolute().top(px(HEADER_H)).left_0().right_0().h(hairline).bg(Theme::hairline()))
            .when(self.sidebar_open, |d| d.child(div().absolute().top_0().bottom_0().left_0().w(hairline).bg(Theme::hairline())));
        crate::actions::on_window(div().id("workspace"), cx)
            .key_context("Workspace")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .bg(Theme::window())
            .font_family(theme::UI_FONT)
            .text_color(Theme::text())
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| {
                // Esc leaves Settings.
                if ev.keystroke.key == "escape" && w.view == View::Settings {
                    w.close_settings(window, cx);
                    cx.stop_propagation();
                }
            }))
            // A focused field binds Esc, and bindings run before key listeners.
            .capture_action(cx.listener(|w, _: &crate::input::Cancel, window, cx| w.escape(window, cx)))
            .capture_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| if ev.keystroke.key == "escape" { w.escape(window, cx); }))
            .children(sidebar)
            .child(note)
            .children(resize)
            .children(crate::ui::window_controls(window))
            .children(overlay)
            .children(column_drag)
            .children(resize_overlay)
            .children(toast)
    }
}
