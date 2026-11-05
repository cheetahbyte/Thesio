//! The window's root view: the main pane, and the file operations behind it.

use crate::confirm::Confirm;
use crate::document::{Edit, OpenDocument, SaveStatus};
use crate::overlay::Overlay;
use crate::sidebar::{Outline, Sidebar};
use crate::store::display_title;
use crate::store::Entry;
use crate::shared_store::SharedStore;
use crate::theme::{self, Theme, craft};
use thesio_core::paths;
use thesio_core::project;
use crate::ui::{HEADER_H, Icon, TipExt, TitleBarExt, ToastKind, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
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

/// What the main pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Doc,
    Home,
    Library,
    Sources,
    /// A paper, inside the app: `Papers::current`.
    Paper,
    Trash,
    Settings,
}

pub struct Workspace {
    pub settings: crate::settings::Settings,
    native_sidebar: crate::native_sidebar::NativeSidebar,
    pub(crate) papers: crate::papers::Papers,
    pub store: SharedStore,
    pub entries: Vec<Entry>,
    pub(crate) document: Option<OpenDocument>,
    pub citations: crate::citations::Citations,
    pub(crate) search: crate::search::Search,
    pub(crate) sidebar: Sidebar,
    pub(crate) outline: Outline,
    pub(crate) overlay: Option<crate::overlay::Overlay>,
    /// Where focus was when the overlay opened.
    pub(crate) overlay_return: Option<FocusHandle>,
    pub view: View,
    pub home: crate::home::Home,
    /// Trash rows, refreshed when the page opens or changes.
    pub trash: Vec<crate::store::TrashEntry>,
    /// The toast and when it appeared.
    pub toast: Option<(ToastKind, SharedString, Instant)>,
    toast_task: Option<Task<()>>,
    pub(crate) focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(document: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.global::<SharedStore>().clone();
        let entries = store.borrow_mut().list();
        let subs = vec![
            cx.observe_window_bounds(window, |_, window, cx| crate::window_size::remember(window, cx)),
            cx.observe_window_appearance(window, |_, _, cx| if crate::appearance::sync_dark(cx) { cx.refresh_windows(); }),
            cx.observe_window_activation(window, |this, window, cx| {
                // The accent has no change event in GPUI, and changing it means visiting System Settings.
                if window.is_window_active() { if crate::appearance::sync_accent() { cx.refresh_windows(); } this.refresh_entries(cx); this.reload_library(); this.table_changed(window, cx); cx.notify(); }
                else { this.save_all(cx); }
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
        let sidebar = Sidebar::new(store.borrow().settings());
        let citations = crate::citations::Citations::new(store.borrow().root(), window, cx);
        let mut ws = Workspace {
            settings: crate::settings::Settings::new(cx),
            store,
            entries,
            document: None,
            citations,
            search: crate::search::Search::new(window, cx),
            sidebar,
            outline: Outline::default(),
            overlay: None,
            overlay_return: None,
            view: View::Doc,
            home: Default::default(),
            trash: vec![],
            toast: None,
            toast_task: None,
            native_sidebar: Default::default(),
            papers: Default::default(),
            focus: cx.focus_handle(),
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

    /// Shows `id`, once the open document is safely saved.
    pub fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let paper = crate::papers::opens_in_viewer(id);
        if self.view != View::Doc && !paper { self.set_view(View::Doc, cx); }
        if self.current() == Some(id) {
            return;
        }
        if self.entries.iter().any(|e| e.id == id && e.folder) {
            return self.toggle_folder(id, cx);
        }
        let target = id.to_string();
        self.when_saved(window, cx, move |w, window, cx| w.load_document(&target, window, cx));
    }

    /// Follows a rename or move through the open document and its compiles, the
    /// open paper and its notes, the search index, the selected rows, the open
    /// folders, and the editors of rewritten files. The editor and undo history stay.
    pub(crate) fn path_changed(&mut self, old: &str, new: &str, rewritten: &[String], cx: &mut Context<Self>) {
        if let Some(path) = self.papers.current.as_deref().and_then(|paper| paths::moved(paper, old, new)) { self.papers.current = Some(path); }
        self.refresh_entries(cx);
        let moved = self.current().and_then(|c| paths::moved(c, old, new));
        if let Some(path) = moved && let Some(document) = &mut self.document { document.id = path; }
        self.follow_top_document(cx);
        self.papers.panel.path_changed(old, new);
        self.search.path_changed(old, new);
        if let Some(batch) = &mut self.sidebar.batch {
            for id in batch.iter_mut() {
                if let Some(path) = paths::moved(id, old, new) { *id = path; }
            }
        }
        let renamed: Vec<String> = self.sidebar.expanded.iter().filter(|e| paths::is_within(e, old)).cloned().collect();
        for path in renamed {
            self.sidebar.expanded.remove(&path);
            self.sidebar.expanded.insert(format!("{new}{}", &path[old.len()..]));
        }
        self.save_expanded(cx);
        // Their paths changed on disk: open editors take them in.
        for id in rewritten {
            let text = self.store.borrow().load(id);
            match text {
                Ok(text) => self.show_text(id, &text, Edit::Undoable, cx),
                // The editor keeps the old paths, and its next save writes them back.
                Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not reload {id} after its paths changed: {e:#}"), cx),
            }
        }
        self.refresh_outline(cx);
    }

    /// Goes to a place in a document, opening it if another one is open.
    pub(crate) fn go_to_source(&mut self, path: &str, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.current() != Some(path) { self.open(path, window, cx); }
        if self.current() != Some(path) { return; }
        self.set_view(View::Doc, cx);
        if let Some(editor) = self.editor() { editor.update(cx, |e, cx| e.go_to(offset, window, cx)); }
    }

    /// Opens a linked file, or says that it is gone.
    pub(crate) fn follow_link(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.entries.iter().any(|e| e.id == path) { self.open(path, window, cx); } else { self.show_toast_kind(ToastKind::Error, &format!("{path} does not exist"), cx); }
    }

    fn first_document(&self) -> Option<String> {
        project::newest(self.entries.iter().filter(|e| e.typst).map(|e| (e.id.as_str(), e.modified))).map(str::to_string)
    }

    pub fn new_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.when_saved(window, cx, |w, window, cx| w.create_doc(None, window, cx));
    }

    pub(crate) fn create_doc(&mut self, parent: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.store.borrow_mut().create(parent);
        match result {
            Ok(id) => {
                self.refresh_entries(cx);
                self.search.field.update(cx, |s, cx| s.set_text("", cx));
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

    pub fn ask_delete(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_overlay(Overlay::Confirm(Confirm::Delete { ids: vec![id.to_string()], batch: false }), window, cx);
    }

    /// Moves files and folders to the Trash; if the open one goes, opens the next.
    pub fn delete_docs(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        if self.current().is_some_and(|c| ids.iter().any(|id| paths::is_within(c, id))) {
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
        if self.current().is_some_and(|c| deleted.iter().any(|id| inside(c, id))) { self.document = None; }
        self.refresh_entries(cx);
        if let Some(b) = self.sidebar.batch.as_mut() {
            b.retain(|id| !deleted.iter().any(|d| inside(id, d)));
        }
        if self.document.is_none() {
            // Stay on Home or Trash if that's where the delete came from.
            let view = self.view;
            match self.first_document() {
                Some(next) => self.load_document(&next, window, cx),
                None => self.new_doc(window, cx),
            }
            self.set_view(view, cx);
        } else {
            self.follow_top_document(cx);
            self.refresh_outline(cx);
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
        let Some(text) = self.read_document(id, cx) else { return };
        let name = format!("{}.pdf", self.entries.iter().find(|e| e.id == id).map_or(crate::store::UNTITLED, |e| e.title.as_str()));
        let (root, id) = (self.store.borrow().root().to_path_buf(), id.to_string());
        let picked = crate::system::pick_new_path(&root, &name, cx);
        cx.spawn(async move |this, cx| {
            let Some(path) = Self::picked(&this, picked.await, cx) else { return };
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

    /// What a file dialog answered. A dialog that failed says so, unlike one cancelled.
    pub(crate) fn picked<T>(this: &WeakEntity<Self>, answer: anyhow::Result<Option<T>>, cx: &mut AsyncApp) -> Option<T> {
        answer.unwrap_or_else(|e| {
            let _ = this.update(cx, |w, cx| w.show_toast_kind(ToastKind::Error, &format!("Could not open the file dialog: {e:#}"), cx));
            None
        })
    }

    /// Copies chosen files into the vault and opens the first document.
    pub fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::system::pick_files(None, true, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(paths) = Self::picked(&this, picked.await, cx) else { return };
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

    pub fn show_toast(&mut self, msg: &str, cx: &mut Context<Self>) {
        self.show_toast_kind(ToastKind::Success, msg, cx);
    }

    pub fn show_toast_kind(&mut self, kind: ToastKind, msg: &str, cx: &mut Context<Self>) {
        self.toast = Some((kind, SharedString::from(msg.to_string()), Instant::now()));
        self.toast_task = Some(cx.spawn(async |this, cx| {
            cx.background_executor().timer(Duration::from_secs_f32(crate::ui::TOAST_FADE_AT)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
        cx.notify();
    }

    fn render_toast(&self, window: &mut Window) -> Option<AnyElement> {
        let (kind, msg, t0) = self.toast.as_ref()?;
        crate::ui::toast_el(*kind, msg, *t0, if self.view == View::Paper { TOAST_ON_PAPER } else { 32. }, window)
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.current().and_then(|id| self.entries.iter().find(|e| e.id == id)).map(|e| display_title(&e.title)).unwrap_or_default();
        let save_error = self.document.as_ref().and_then(|document| match &document.status {
            SaveStatus::Failed(error) => Some(error.clone()),
            _ => None,
        });
        let divider = || div().flex_none().w(px(1.)).h(px(18.)).bg(Theme::line_divider());
        // Breadcrumbs: the parent pages, each linking to itself, with the
        // middle ones folded into "…" when the chain is long.
        let chain: Vec<(String, String)> = self
            .current()
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
        let record_more = self.document.as_ref().map(|document| document.more_button.clone());
        let doc_menu = matches!(self.overlay, Some(Overlay::DocMenu));
        div()
            .id("top-bar").relative()
            .h(px(HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .pr(px(crate::ui::header_right_inset()))
            // Clear of the traffic lights, which sit over the note when the sidebar is closed.
            .pl(if self.sidebar.open { px(18.) } else { px(theme::TRAFFIC_LIGHTS_W) })
            .title_bar()
            .when(!self.sidebar.open, |d| {
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
                    .when(self.document.is_some(), |d| {
                        d.child(
                            crate::ui::toolbar_icon("doc-more", Icon::Ellipsis)
                                .relative()
                                .when(doc_menu, |d| d.bg(Theme::fill_hover()))
                                .when(!doc_menu, |d| d.tip("More", None))
                                .child(canvas(move |b, _, _| if let Some(cell) = &record_more { cell.set(Some(b)); }, |_, _, _, _| {}).absolute().inset_0())
                                .on_click(cx.listener(move |w, _, window, cx| {
                                    if doc_menu { w.close_overlay(window, cx); } else { w.open_overlay(Overlay::DocMenu, window, cx); }
                                })),
                        )
                    })
                    .child(self.search_button("doc-search", cx))
                    .child(
                        crate::ui::toolbar_icon("doc-new", Icon::Plus).tip("New document", None).on_click(cx.listener(|w, _, window, cx| w.new_doc(window, cx))),
                    ).into_any_element()
            )
    }
}

/// The toast after a rename or move rewrote other files.
pub(crate) fn updated_paths(files: usize) -> String {
    format!("Updated paths in {files} {}", if files == 1 { "file" } else { "files" })
}

/// Toasts stay in the header over a paper: the native viewer would hide them lower down.
const TOAST_ON_PAPER: f32 = 4.;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.native_sidebar.sync(if self.sidebar.open { self.sidebar_w(window) } else { 0. }, window);
        let sidebar = if self.sidebar.open { Some(if matches!(self.view, View::Settings | View::Trash) { self.render_settings_sidebar(window, cx) } else { self.render_sidebar(window, cx) }) } else { None };
        let resize = self.sidebar.open.then(|| self.render_resize_handle(window, cx));
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
                                .children(self.editor().cloned()),
                        )
                        .when_some(self.document.as_ref().and_then(|document| document.preview.clone()), |d, preview| d.child(div().relative().flex_1().min_w_0().h_full().child(preview))),
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
        let hairline = px(1. / window.scale_factor());
        let note = div().id("note").relative().flex_1().min_w_0().h_full().flex().bg(Theme::bg_body()).child(main)
            .child(div().absolute().top(px(HEADER_H)).left_0().right_0().h(hairline).bg(Theme::hairline()))
            .when(self.sidebar.open, |d| d.child(div().absolute().top_0().bottom_0().left_0().w(hairline).bg(Theme::hairline())));
        crate::actions::on_window(div().id("workspace"), cx)
            .key_context("Workspace")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .bg(Theme::window(crate::native_sidebar::supported()))
            .font_family(theme::UI_FONT)
            .text_color(Theme::text())
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| {
                // Both pages show Back with this key.
                if ev.keystroke.key == "escape" && matches!(w.view, View::Settings | View::Trash) {
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
