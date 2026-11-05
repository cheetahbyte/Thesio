//! Workspace window: two panes — the document list on the left
//! (see `sidebar.rs`) and the editor on the right, with the
//! 64px top bar (breadcrumb title + save status) above the page.

use crate::preview::{Preview, PreviewEvent};
use crate::source::{SourceEditor, SourceEvent};
use crate::input::{TextField, TextFieldEvent};
use crate::settings::display_title;
use crate::store::Entry;
use crate::shared_store::SharedStore;
use crate::theme::{self, Theme, craft};
use crate::ui::{Icon, TipExt, TipSide, TitleBarExt, ToastKind, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

actions!(workspace, [NewDoc, NewFolder, ImportDoc, InsertCitation, ToggleSidebar, FocusSearch, OpenSettings, OpenAbout, GoHome, CloseWindow, Quit,
    HideApp, HideOthers, ShowAll, Minimize, Zoom, ToggleFullScreen]);

/// The menu bar, in the interface language.
pub fn set_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Thesio".into(),
            items: vec![
                MenuItem::action("About Thesio", OpenAbout),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Thesio", HideApp),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Thesio", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Document", NewDoc),
                MenuItem::action("New Folder", NewFolder),
                MenuItem::action("Insert Citation…", InsertCitation),
                MenuItem::action("Import as Document…", ImportDoc),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindow),
            ],
        },
        // The standard Edit menu: its items act on the focused editor, and
        // macOS adds Dictation and Emoji & Symbols to it.
        Menu {
            name: "Edit".into(),
            items: {
                use crate::source as k;
                vec![
                    MenuItem::os_action("Undo", k::Undo, OsAction::Undo),
                    MenuItem::os_action("Redo", k::Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action("Cut", k::Cut, OsAction::Cut),
                    MenuItem::os_action("Copy", k::Copy, OsAction::Copy),
                    MenuItem::os_action("Paste", k::Paste, OsAction::Paste),
                    MenuItem::os_action("Select All", k::SelectAll, OsAction::SelectAll),
                ]
            },
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Zoom", Zoom),
                MenuItem::separator(),
                MenuItem::action("Enter / Exit Full Screen", ToggleFullScreen),
            ],
        },
    ]);
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewDoc, None),
        KeyBinding::new("cmd-shift-n", NewFolder, None),
        KeyBinding::new("cmd-shift-c", InsertCitation, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-shift-i", ImportDoc, None),
        KeyBinding::new("cmd-alt-home", GoHome, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-]", ToggleSidebar, None),
        KeyBinding::new("cmd-\\", ToggleSidebar, None),
        KeyBinding::new("cmd-j", FocusSearch, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // macOS app and window keys. On Windows the system handles these, and
    // F11 is full screen.
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("f11", ToggleFullScreen, None)]);
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(handle) = cx.active_window().and_then(|w| w.downcast::<Workspace>()) {
            // Key dispatch already borrows this window; close after it returns.
            cx.defer(move |cx| {
                if let Err(e) = handle.update(cx, |ws, window, cx| {
                    crate::window_size::persist(cx);
                    if ws.save_before(cx, |_, window, _| window.remove_window()) { window.remove_window(); }
                }) { eprintln!("Could not close document window: {e}"); }
            });
        }
    });
    cx.on_action(|_: &Quit, cx| quit(cx));
}

/// Quit: the open document is saved first; when that fails its dialog asks
/// (Retry / Discard changes) and quits afterwards. Call it outside any window update
/// (`cx.defer`): a window on the update stack cannot be asked to save.
pub fn quit(cx: &mut App) {
    let ready = cx
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<Workspace>())
        .all(|w| w.update(cx, |ws, _, cx| ws.save_before(cx, |_, _, cx| cx.quit())).unwrap_or(true));
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

/// Right-click / "…" menu on a sidebar row.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub id: String,
    pub pos: Point<Pixels>,
    /// Which corner of the menu sits at `pos`.
    pub corner: Corner,
    /// The selection menu for the checked rows.
    pub batch: bool,
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

/// A confirmation dialog and the pages it is about.
#[derive(Clone, Debug)]
pub enum Confirm {
    /// Delete to Trash; `batch` for the selected rows.
    Delete { ids: Vec<String>, batch: bool },
    /// Trash → Restore.
    Restore(String),
    /// Trash → Delete permanently.
    Purge(String),
    /// Library → delete these entries.
    DeleteEntries(Vec<String>),
    /// Library → change the inspected entry to a type without a parent.
    ChangeSourceType(&'static thesio_core::source_type::SourceType),
    /// Library → remove a tag from every entry.
    DeleteTag(String),
    /// The open document could not be written before it was to be replaced
    /// (another document, New, Quit): Retry / Discard changes, then carry on.
    SaveFailed,
}

/// What to do once the open document is safely on disk (or its changes given up).
pub type AfterSave = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// Inline rename of a sidebar row.
pub struct Rename {
    pub id: String,
    pub field: Entity<TextField>,
    _subs: Vec<Subscription>,
}

pub struct Workspace {
    pub settings_page: crate::settings::Page,
    native_sidebar: crate::native_header::NativeSidebar,
    /// The vault id of the paper the Paper view shows.
    pub(crate) paper: Option<String>,
    /// PDFKit could not read `paper`.
    pub(crate) paper_failed: Rc<Cell<bool>>,
    pub(crate) pdf: Rc<crate::native_pdf::PdfPane>,
    pub(crate) paper_panel: crate::paper::PaperPanel,
    pub store: SharedStore,
    pub entries: Vec<Entry>,
    pub current: Option<String>,
    pub editor: Option<Entity<SourceEditor>>,
    pub preview: Option<Entity<Preview>>,
    pub citations: crate::citations::Citations,
    pub search: Entity<TextField>,
    /// Keyboard highlight inside search results.
    pub search_sel: Option<usize>,
    pub sidebar_open: bool,
    /// Dragged width (300 by default, 268 to min(800, window − 540)).
    pub sidebar_width: f32,
    /// The resize handle is being dragged.
    pub sidebar_drag: bool,
    /// The search dialog (command-center style) is open.
    pub search_open: bool,
    pub status: SaveStatus,
    pub row_menu: Option<RowMenu>,
    pub rename: Option<Rename>,
    /// Last-frame bounds of each row's "…" button, to hang its menu from.
    /// The sidebar list's own menu, and where it opened.
    pub files_menu: Option<Point<Pixels>>,
    /// Header "…" menu.
    pub doc_menu: bool,
    /// The citation style menu.
    pub style_menu: bool,
    pub list_scroll: ScrollHandle,
    pub search_scroll: ScrollHandle,
    /// Selection mode: checked documents in My documents, in check order.
    pub batch: Option<Vec<String>>,
    /// The open confirmation dialog.
    pub confirm: Option<Confirm>,
    pub view: View,
    pub home: crate::home::Home,
    pub settings_scroll: ScrollHandle,
    /// Trash rows, refreshed when the page opens or changes.
    pub trash: Vec<crate::store::TrashEntry>,
    /// A Trash row's menu: the page and where it opens.
    pub trash_menu: Option<(String, Point<Pixels>, Corner)>,
    /// The toast and when it appeared.
    pub toast: Option<(ToastKind, SharedString, Instant)>,
    toast_task: Option<Task<()>>,
    /// Folded sidebar sections: Pinned, My documents.
    pub folded: [bool; 2],
    /// Expanded pages of the My documents tree.
    pub expanded: HashSet<String>,
    pub move_dlg: Option<crate::tree::MoveDialog>,
    /// The header "…" button, which the doc menu hangs from.
    pub more_btn_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) focus: FocusHandle,
    save_task: Option<Task<()>>,
    /// The open document has changes not yet on disk (cleared only by a
    /// successful save or by Discard changes).
    unsaved: bool,
    /// Held back by a failed save: runs after Retry succeeds or Discard changes.
    pub(crate) after_save: Option<AfterSave>,
    editor_subs: Vec<Subscription>,
    _subs: Vec<Subscription>,
}



impl Workspace {
    pub fn new(document: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::settings::load();
        if !cx.has_global::<SharedStore>() { cx.set_global(SharedStore::open().expect("open document library")); }
        let mut store = cx.global::<SharedStore>().clone();
        let entries = store.list();
        let search = cx.new(|cx| {
            let mut f = TextField::new("Search", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![
            cx.observe_window_bounds(window, |_, window, cx| crate::window_size::remember(window, cx)),
            // The system switched between light and dark (or the app's own look changed).
            cx.observe_window_appearance(window, |this, _, cx| if crate::settings::sync_dark(cx) { crate::settings::refresh_all(this, cx); }),
            cx.observe_window_activation(window, |this, window, cx| {
                // The accent has no change event in GPUI, and changing it means visiting System Settings.
                if window.is_window_active() { if crate::theme::sync_accent() { crate::settings::refresh_all(this, cx); } this.entries = this.store.list(); this.sort(); this.reload_library(); cx.notify(); }
                else { this.flush(cx); }
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
                    TextFieldEvent::Cancel => this.close_search(window, cx),
                    _ => {}
                }
                cx.notify();
            }),
            // Last resort (the system logging out): ⌘Q and the close button
            // go through `save_before` and ask when the save fails.
            cx.on_app_quit(|this, cx| {
                crate::window_size::persist(cx);
                this.flush(cx);
                async {}
            }),
        ];
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            crate::window_size::persist(cx);
            this.update(cx, |w, cx| w.save_before(cx, |_, window, _| window.remove_window())).unwrap_or(true)
        });
        let sidebar_width = store.settings().sidebar_width.unwrap_or(theme::SIDEBAR_WIDTH);
        let store_expanded: HashSet<String> = store.settings().expanded.into_iter().collect();
        let citations = crate::citations::Citations::new(&store.root(), cx);
        let mut ws = Workspace {
            settings_page: Default::default(),
            store,
            entries,
            current: None,
            editor: None,
            preview: None,
            citations,
            search,
            search_sel: None,
            sidebar_open: true,
            sidebar_width,
            sidebar_drag: false,
            search_open: false,
            status: SaveStatus::Idle,
            row_menu: None,
            rename: None,
            files_menu: None,
            doc_menu: false,
            style_menu: false,
            list_scroll: ScrollHandle::new(),
            search_scroll: ScrollHandle::new(),
            batch: None,
            confirm: None,
            view: View::Doc,
            home: Default::default(),
            settings_scroll: ScrollHandle::new(),
            trash: vec![],
            trash_menu: None,
            toast: None,
            toast_task: None,
            more_btn_bounds: Rc::new(Cell::new(None)),
            native_sidebar: Default::default(),
            paper: None,
            paper_failed: Default::default(),
            pdf: Default::default(),
            paper_panel: Default::default(),
            folded: [false; 2],
            expanded: store_expanded,
            move_dlg: None,
            focus: cx.focus_handle(),
            save_task: None,
            unsaved: false,
            after_save: None,
            editor_subs: vec![],
            _subs: subs,
        };
        ws.sort();
        match document.or_else(|| ws.first_document()) {
            Some(id) => ws.open(&id, window, cx),
            None => ws.new_doc(window, cx),
        }
        // Kept in the top bar (not just a toast): the user should notice.
        let warnings = ws.store.take_warnings();
        if !warnings.is_empty() {
            let msg = warnings.iter().map(|s| crate::settings::diagnostic(s)).collect::<Vec<_>>().join("; ");
            ws.show_toast_kind(ToastKind::Error, &msg, cx);
            ws.status = SaveStatus::Failed(msg);
        }
        ws
    }

    /// Most recently modified first.
    pub(crate) fn sort(&mut self) {
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.meta.modified));
    }

    pub fn query(&self, cx: &App) -> String {
        self.search.read(cx).text.trim().to_lowercase()
    }

    pub fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.try_open(id, window, cx);
    }

    /// Show `id`; false when it could not be opened (or the open document
    /// could not be saved first, see [`Self::save_before`]).
    pub fn try_open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.view != View::Doc {
            self.view = View::Doc;
            cx.notify();
        }
        if self.current.as_deref() == Some(id) {
            return true;
        }
        if self.entries.iter().any(|e| e.id == id && e.folder) {
            self.toggle_folder(id, cx);
            return false;
        }
        let target = id.to_string();
        if !self.save_before(cx, move |w, window, cx| {
            w.open(&target, window, cx);
        }) {
            return false;
        }
        if id.ends_with(".pdf") && crate::native_pdf::supported() {
            self.open_paper(id, cx);
            return false;
        }
        let text = match self.store.load(id) {
            Ok(text) => text,
            Err(e) => {
                // Binary files belong to the system's apps; scripted runs must not start any.
                if !id.ends_with(".typ") && std::env::var_os("THESIO_SCRIPT").is_none() {
                    cx.open_with_system(&self.store.root().join(id));
                } else {
                    self.show_toast_kind(ToastKind::Error, &format!("Could not open {id}: {}", crate::settings::diagnostic(&e.to_string())), cx);
                }
                cx.notify();
                return false;
            }
        };
        let typst = id.ends_with(".typ");
        let editor = cx.new(|cx| SourceEditor::new(text.clone(), typst, cx));
        let root = self.store.root();
        let main = self.top_document(id);
        let preview = typst.then(|| cx.new(|_| Preview::new(root, id.to_string(), main)));
        if let Some(preview) = &preview { preview.update(cx, |p, cx| p.update(text, cx)); }
        self.editor_subs = vec![
            cx.subscribe_in(&editor, window, |this, editor, event: &SourceEvent, window, cx| match event {
                SourceEvent::Changed => {
                    let text = editor.read(cx).buffer.text().to_string();
                    if let Some(preview) = &this.preview { preview.update(cx, |p, cx| p.update(text, cx)); }
                    this.schedule_save(window, cx);
                }
                SourceEvent::Citing => this.offer_sources(editor, cx),
                SourceEvent::Cited => if let Some(id) = this.current.clone() { this.citation_added(&id, cx); },
            }),
        ];
        if let Some(preview) = &preview {
            self.editor_subs.push(cx.subscribe_in(preview, window, |this, _, ev: &PreviewEvent, window, cx| {
                let PreviewEvent::Jump(offset) = ev;
                if let Some(editor) = &this.editor { editor.update(cx, |e, cx| e.go_to(*offset, window, cx)); }
            }));
        }
        // Opening a document puts the caret in it, unless the user is typing a search.
        if !self.search_open {
            editor.read(cx).focus(window);
        }
        if self.store.touch(id).is_ok()
            && let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                e.meta.opened = crate::time::now();
            }
        self.editor = Some(editor);
        self.preview = preview;
        self.current = Some(id.to_string());
        self.reveal_in_tree(id);
        self.status = SaveStatus::Idle;
        self.row_menu = None;
        self.doc_menu = false;
        cx.notify();
        true
    }

    /// Follows a rename or move: the entry list, and the open document if it
    /// or a folder above it changed path. Its editor and undo history stay.
    pub(crate) fn path_changed(&mut self, old: &str, new: &str, rewritten: &[String], cx: &mut Context<Self>) {
        let moved = self.current.as_deref().and_then(|c| (c == old).then(|| new.to_string()).or_else(|| c.strip_prefix(&format!("{old}/")).map(|rest| format!("{new}/{rest}"))));
        if let Some(path) = moved {
            self.current = Some(path.clone());
            self.entries = self.store.list();
            let main = self.top_document(&path);
            if let Some(preview) = &self.preview { preview.update(cx, |p, _| p.set_path(path, main)); }
        }
        if self.paper.as_deref() == Some(old) { self.paper = Some(new.to_string()); }
        let prefix = format!("{old}/");
        let renamed: Vec<String> = self.expanded.iter().filter(|e| *e == old || e.starts_with(&prefix)).cloned().collect();
        for path in renamed {
            self.expanded.remove(&path);
            self.expanded.insert(format!("{new}{}", &path[old.len()..]));
        }
        self.save_expanded();
        // The open document's paths changed on disk: its editor takes them in.
        if let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone())
            && rewritten.contains(&id)
            && let Ok(text) = self.store.load(&id) {
                editor.update(cx, |e, cx| e.reload(&text, cx));
            }
        self.entries = self.store.list();
        self.sort();
    }

    fn first_document(&self) -> Option<String> {
        self.entries.iter().find(|e| e.typst).map(|e| e.id.clone())
    }

    pub fn new_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.save_before(cx, |w, window, cx| w.new_doc(window, cx)) {
            return;
        }
        match self.store.create(None) {
            Ok(id) => {
                self.search_open = false;
                self.entries = self.store.list();
                self.sort();
                self.search.update(cx, |s, cx| s.set_text("", cx));
                self.open(&id, window, cx);
                if let Some(ed) = &self.editor {
                    ed.read(cx).focus(window);
                }
            }
            Err(e) => self.status = SaveStatus::Failed(format!("Could not create document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        cx.notify();
    }

    /// A new folder, with its name ready to edit.
    pub fn new_folder(&mut self, parent: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.create_folder(parent, "New folder") {
            Ok(id) => {
                self.entries = self.store.list();
                self.sort();
                self.start_rename(&id, window, cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not create folder: {e}"), cx),
        }
        cx.notify();
    }

    /// Ask before deleting: a confirmation, then the Trash.
    pub fn ask_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.row_menu = None;
        self.doc_menu = false;
        self.confirm = Some(Confirm::Delete { ids: vec![id.to_string()], batch: false });
        cx.notify();
    }

    /// Moves files and folders to the Trash; if the open one goes, opens the next.
    pub fn delete_docs(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        let inside = |doc: &str, id: &String| doc == id || doc.starts_with(&format!("{id}/"));
        if self.current.as_ref().is_some_and(|c| ids.iter().any(|id| inside(c, id))) {
            let retry = ids.to_vec();
            if !self.save_before(cx, move |w, window, cx| w.delete_docs(&retry, window, cx)) {
                return;
            }
        }
        // An item inside another selected folder goes with it.
        let topmost: Vec<&String> = ids.iter().filter(|id| !ids.iter().any(|o| o != *id && id.starts_with(&format!("{o}/")))).collect();
        let mut deleted: Vec<String> = vec![];
        let mut errors = vec![];
        for id in topmost {
            match self.store.delete(id) {
                Ok(()) => deleted.push(id.clone()),
                Err(e) => errors.push(format!("Could not delete document: {}", crate::settings::diagnostic(&e.to_string()))),
            }
        }
        if self.current.as_ref().is_some_and(|c| deleted.iter().any(|id| inside(c, id))) {
            self.save_task = None;
            self.unsaved = false;
            self.current = None;
            self.editor = None;
            self.preview = None;
            self.editor_subs.clear();
        }
        self.entries = self.store.list();
        self.sort();
        if let Some(b) = self.batch.as_mut() {
            b.retain(|id| !deleted.iter().any(|d| inside(id, d)));
        }
        self.row_menu = None;
        if self.current.is_none() {
            // Stay on Home or Trash if that's where the delete came from.
            let view = self.view;
            match self.first_document() {
                Some(next) => self.open(&next, window, cx),
                None => self.new_doc(window, cx),
            }
            self.view = view;
        }
        if self.view == View::Trash {
            self.trash = self.store.list_trash();
        }
        errors.extend(self.store.take_warnings().iter().map(|s| crate::settings::diagnostic(s)));
        if errors.is_empty() {
            self.show_toast("Deleted successfully", cx);
        } else {
            let msg = errors.join("; ");
            self.show_toast_kind(ToastKind::Error, &msg, cx);
            self.status = SaveStatus::Failed(msg);
        }
        cx.notify();
    }

    /// Asks where to save, then compiles the document's current text to a PDF.
    pub fn export_pdf(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(text) = self.text_of(id, cx) else { return };
        let name = format!("{}.pdf", self.entries.iter().find(|e| e.id == id).map_or(crate::store::UNTITLED, |e| e.title.as_str()));
        let (root, id) = (self.store.root(), id.to_string());
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
                    match w.store.import(path) {
                        Ok(id) => if id.ends_with(".typ") { first.get_or_insert(id); },
                        Err(e) => w.show_toast_kind(ToastKind::Error, &format!("Could not import {}: {e}", path.display()), cx),
                    }
                }
                w.entries = w.store.list();
                w.sort();
                if let Some(id) = first { w.open(&id, window, cx); }
                cx.notify();
            });
        }).detach();
    }

    pub fn toggle_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut on = false;
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.meta.pinned = !e.meta.pinned;
            on = e.meta.pinned;
            let _ = self.store.set_pinned(id, e.meta.pinned);
        }
        self.sort();
        self.row_menu = None;
        self.show_toast(if on { "Pinned" } else { "Unpinned" }, cx);
        cx.notify();
    }

    /// Favorites / Remove from favorites (listed under Home → Favorites).
    pub fn toggle_star(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return };
        let on = e.meta.starred == 0;
        if let Ok(t) = self.store.set_starred(id, on) {
            e.meta.starred = t;
        }
        self.row_menu = None;
        self.doc_menu = false;
        self.show_toast(if on { "Added to favorites" } else { "Removed from favorites" }, cx);
        cx.notify();
    }

    /// Switch the main pane to Home or Trash (or back to the document).
    pub fn show_view(&mut self, view: View, cx: &mut Context<Self>) {
        if view == View::Trash {
            self.trash = self.store.list_trash();
        }
        if matches!(view, View::Library | View::Sources) {
            self.reload_library();
        }
        self.view = view;
        self.row_menu = None;
        self.trash_menu = None;
        cx.notify();
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
        self.reveal_in_tree(id);
        self.row_menu = None;
        cx.notify();
    }

    fn finish_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rename.take() else { return };
        let refocus = r.field.read(cx).is_focused(window);
        let title = r.field.read(cx).text.trim().to_string();
        let old = self.entries.iter().find(|e| e.id == r.id).map(|e| e.title.trim().to_string());
        if commit && old.is_some_and(|o| o != title) {
            self.flush(cx);
            match self.store.rename(&r.id, &title) {
                Ok((new, rewritten)) => {
                    self.path_changed(&r.id, &new, &rewritten, cx);
                    if !rewritten.is_empty() { self.show_toast(&updated_paths(rewritten.len()), cx); }
                }
                Err(e) => self.status = SaveStatus::Failed(format!("Could not rename document: {}", crate::settings::diagnostic(&e.to_string()))),
            }
        }
        if refocus
            && let Some(ed) = &self.editor {
                ed.read(cx).focus(window);
            }
        cx.notify();
    }



    /// Writes the open document again after text changed without an edit.
    pub(crate) fn mark_unsaved(&mut self, cx: &mut Context<Self>) {
        self.unsaved = true;
        self.flush(cx);
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = SaveStatus::Saving;
        self.unsaved = true;
        self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let _ = this.update(cx, |w, cx| w.flush(cx));
        }));
        cx.notify();
    }


    /// Write the open document to disk if it has unsaved changes. False when
    /// that failed: the changes stay pending (a later save retries them) and
    /// the top bar shows the error.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_inspector(cx);
        self.save_task = None;
        if !self.unsaved {
            return true;
        }
        let (Some(id), Some(ed)) = (self.current.clone(), self.editor.clone()) else {
            self.unsaved = false;
            return true;
        };
        let text = ed.read(cx).buffer.text().to_string();
        match self.store.save(&id, &text) {
            Ok(()) => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                    e.body = text;
                    e.meta.modified = crate::time::now();
                }
                self.status = SaveStatus::Saved;
                self.unsaved = false;
                self.sort();
                self.sync_bibliographies(cx);
            }
            Err(e) => self.status = SaveStatus::Failed(format!("Could not save: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        cx.notify();
        !self.unsaved
    }

    /// Save the open document before something replaces it. When the save
    /// fails nothing is replaced: the Save failed dialog offers Retry / Discard changes
    /// and runs `then` after either.
    pub fn save_before(&mut self, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> bool {
        if self.flush(cx) {
            return true;
        }
        self.after_save = Some(Box::new(then));
        self.confirm = Some(Confirm::SaveFailed);
        cx.notify();
        false
    }

    /// The Save failed dialog's choices: `retry` saves again, otherwise the
    /// changes are given up. Either way the held-back action runs once the
    /// document no longer needs saving.
    pub(crate) fn resolve_save_failed(&mut self, retry: bool, window: &mut Window, cx: &mut Context<Self>) {
        if retry {
            if !self.flush(cx) {
                self.confirm = Some(Confirm::SaveFailed);
                return;
            }
        } else {
            self.unsaved = false;
            self.status = SaveStatus::Idle;
        }
        if let Some(then) = self.after_save.take() {
            then(self, window, cx);
        }
        cx.notify();
    }

    /// The error of the last failed save, for the Save failed dialog.
    pub(crate) fn save_error(&self) -> String {
        match &self.status {
            SaveStatus::Failed(e) => e.clone(),
            _ => "Save failed".to_string(),
        }
    }





    /// Home's search box, centred on the pane but never over what sits on
    /// either side of it: the title (and, with the sidebar hidden, the
    /// traffic lights and Show sidebar) on the left, + on the right. Below a
    /// usable width it is None and the caller shows a search icon instead.
    pub(crate) fn header_search(&self, window: &Window, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let main = f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let title = crate::settings::label("Home");
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
            .when(width >= 200., |d| d.child(div().flex_none().text_size(px(11.)).text_color(Theme::text_placeholder()).child(crate::shortcuts::hint("workspace::FocusSearch", "⌘ + J").replace(" + ", " "))))
            .tip("Search documents", Some(crate::shortcuts::hint("workspace::FocusSearch", "⌘ + J")))
            .on_click(cx.listener(|w, _, window, cx| w.open_search(window, cx))))
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
                d.child(
                    div()
                        .id("open-sidebar")
                        .no_drag()
                        .flex_none()
                        .ml(px(4.))
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .tip("Show sidebar", Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                        .child(icon(Icon::SidebarToggle, 17., Theme::icon()))
                        .on_click(cx.listener(|w, _, _, cx| {
                            w.sidebar_open = true;
                            cx.notify();
                        })),
                )
                .child(
                    div().id("header-home").no_drag().flex_none().ml(px(4.)).size(px(28.))
                        .flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                        .active(|s| s.bg(Theme::fill_pressed()))
                        .tip("Back to Home", None)
                        .child(icon(Icon::Home, 18., Theme::icon()))
                        .on_click(cx.listener(|w, _, _, cx| w.show_view(View::Home, cx))),
                )
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
                                .when(self.doc_menu, |d| d.bg(Theme::fill_hover()))
                                .when(!self.doc_menu, |d| d.tip("More", None))
                                .child(canvas(move |b, _, _| record_more.set(Some(b)), |_, _, _, _| {}).absolute().inset_0())
                                .on_click(cx.listener(|w, _, _, cx| {
                                    w.doc_menu = !w.doc_menu;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        seg("doc-search", Icon::Search)
                            .tip_label("Search (⌘ + J)".into(), TipSide::Below)
                            .on_click(cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(FocusSearch), cx))),
                    )
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
        if self.view == View::Paper && !self.paper.as_ref().is_some_and(|id| self.entries.iter().any(|e| &e.id == id)) { self.view = View::Sources; }
        if self.view != View::Paper { self.pdf.close(); }
        self.native_sidebar.sync(if self.sidebar_open { self.sidebar_w(window) } else { 0. }, window);
        let sidebar = if self.sidebar_open { Some(if matches!(self.view, View::Settings | View::Trash) { self.render_settings_sidebar(window, cx) } else { self.render_sidebar(window, cx) }) } else { None };
        let row_menu = self.render_row_menu(cx);
        let files_menu = self.render_files_menu(cx);
        let resize = self.sidebar_open.then(|| self.render_resize_handle(window, cx));
        let resize_overlay = self.render_resize_overlay(cx);
        let search = self.render_search(window, cx);
        let doc_menu = self.render_doc_menu(window, cx);
        let style_menu = self.render_style_menu(window, cx);
        let confirm = self.render_confirm(cx);
        let trash_menu = self.render_trash_menu(cx);
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
        let move_dlg = self.render_move_dialog(cx);
        let cite_picker = self.render_cite_picker(window, cx);
        let lookup = self.render_lookup(window, cx);
        let quote_dialog = self.render_quote_dialog(window, cx);
        let library_menu = self.render_library_menu(window, cx);
        let library_row_menu = self.render_library_row_menu(cx);
        let tag_editor = self.render_tag_editor(window, cx);
        let type_menu = self.render_inspector_type_menu(cx);
        let tag_menu = self.render_tag_menu(window, cx);
        let column_drag = self.render_column_drag(cx);
        // Only the note stands out: a white plane flush with the window's top,
        // right and bottom edges. The sidebar sits sunken on the window
        // surface beside it; the change of value is the only line between
        // them (no gutter, radius, border or shadow).
        let hairline = px(1. / window.scale_factor());
        let note = div().id("note").relative().flex_1().min_w_0().h_full().flex().bg(Theme::bg_body()).child(main)
            .child(div().absolute().top(px(HEADER_H)).left_0().right_0().h(hairline).bg(Theme::hairline()))
            .when(self.sidebar_open, |d| d.child(div().absolute().top_0().bottom_0().left_0().w(hairline).bg(Theme::hairline())));
        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .bg(Theme::window())
            .font_family(theme::UI_FONT)
            .text_color(Theme::text())
            // Window keys act on the window they were pressed in, not on
            // whichever window macOS considers active.
            .on_action(cx.listener(|w, _: &CloseWindow, window, cx| {
                crate::window_size::persist(cx);
                if w.save_before(cx, |_, window, _| window.remove_window()) { window.remove_window(); }
            }))
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|w, _: &OpenSettings, _, cx| w.show_view(View::Settings, cx)))
            .on_action(cx.listener(|w, _: &OpenAbout, _, cx| w.open_settings(crate::settings::Page::About, cx)))
            .on_action(cx.listener(|w, _: &GoHome, _, cx| w.show_view(View::Home, cx)))
            .on_action(cx.listener(|w, _: &NewDoc, window, cx| w.new_doc(window, cx)))
            .on_action(cx.listener(|w, _: &InsertCitation, window, cx| w.open_cite_picker(window, cx)))
            .on_action(cx.listener(|w, _: &NewFolder, window, cx| w.new_folder(None, window, cx)))
            .on_action(cx.listener(|w, _: &ImportDoc, window, cx| w.open_import(window, cx)))
            .on_action(cx.listener(|w, _: &ToggleSidebar, _, cx| {
                w.sidebar_open = !w.sidebar_open;
                cx.notify();
            }))
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, _, cx| {
                // Esc leaves Settings.
                if ev.keystroke.key == "escape" && w.view == View::Settings {
                    w.close_settings(cx);
                    cx.stop_propagation();
                    return;
                }
                if ev.keystroke.key == "escape" && (w.doc_menu || w.style_menu) {
                    w.doc_menu = false;
                    w.style_menu = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            // Esc leaves Selection mode (closing its menu or confirmation first), even from the editor.
            .capture_key_down(cx.listener(|w, ev: &KeyDownEvent, _, cx| {
                // Esc cancels a confirmation dialog.
                if ev.keystroke.key == "escape" && w.confirm.is_some() {
                    w.confirm = None;
                    w.after_save = None;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if ev.keystroke.key != "escape" || w.batch.is_none() || w.search_open || w.rename.is_some() {
                    return;
                }
                if w.row_menu.is_some() {
                    w.row_menu = None;
                } else {
                    w.batch = None;
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_action(cx.listener(|w, _: &FocusSearch, window, cx| {
                if w.search_open { w.close_search(window, cx); } else { w.open_search(window, cx); }
                cx.stop_propagation();
            }))
            .children(sidebar)
            .child(note)
            .children(resize)
            .children(crate::ui::window_controls(window))
            .children(row_menu)
            .children(files_menu)
            .children(doc_menu)
            .children(style_menu)
            .children(trash_menu)
            .children(confirm)
            .children(move_dlg)
            .children(cite_picker)
            .children(lookup)
            .children(quote_dialog)
            .children(library_menu)
            .children(library_row_menu)
            .children(tag_editor)
            .children(type_menu)
            .children(tag_menu)
            .children(column_drag)
            .children(search)
            .children(resize_overlay)
            .children(toast)
    }
}
