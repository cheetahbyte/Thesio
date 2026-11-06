//! Papers: PDFs in `sources/`, each following its entry in the Library.

pub(crate) mod notes;
pub(crate) mod quote;
mod sources;
pub(crate) mod viewer;

use crate::app::{View, Workspace};
use thesio_core::library::{self, PAPERS_DIR, Row, note_file, paper_file};
use crate::ui::ToastKind;
use gpui::*;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use thesio_core::lookup;
use thesio_core::paths::{PDF_HEADER, base_name, is_pdf};

/// Whether `id` opens in the app's own viewer. Any PDF in the vault does.
pub(crate) fn opens_in_viewer(id: &str) -> bool { is_pdf(id) && crate::native_pdf::supported() }

/// The open paper and the viewer, and the downloads under way.
#[derive(Default)]
pub struct Papers {
    /// The vault id of the paper the Paper view shows.
    pub(crate) current: Option<String>,
    /// PDFKit could not read `current`.
    pub(crate) failed: Rc<Cell<bool>>,
    pub(crate) pdf: Rc<crate::native_pdf::PdfPane>,
    pub(crate) panel: viewer::PaperPanel,
    /// Papers still downloading.
    pub(crate) fetching: usize,
}

impl Workspace {
    /// The ids of the PDFs in `sources/`.
    pub(crate) fn paper_ids(&self) -> BTreeSet<String> {
        self.entries.iter().filter(|e| !e.folder && e.parent.as_deref() == Some(PAPERS_DIR) && is_pdf(&e.id)).map(|e| e.id.clone()).collect()
    }

    pub(crate) fn has_paper(&self, key: &str) -> bool {
        let file = paper_file(key);
        self.entries.iter().any(|e| e.id == file)
    }

    /// Shows a paper in the app, once the open notes are safely saved, or where
    /// there is no viewer, in the system's app.
    pub(crate) fn open_paper(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !crate::native_pdf::supported() { return self.open_paper_externally(id, cx); }
        let id = id.to_string();
        self.when_saved(window, cx, move |w, window, cx| {
            w.papers.current = Some(id.clone());
            w.papers.failed.set(false);
            w.load_paper_notes(&id, cx);
            w.show_view(View::Paper, window, cx);
        });
    }

    pub(crate) fn open_paper_externally(&self, id: &str, cx: &App) {
        if let Ok(path) = self.store.borrow().path(id) { crate::system::open_with_system(&path, cx); }
    }

    /// The source a paper belongs to.
    pub(crate) fn paper_row(&self, id: &str) -> Option<Row> {
        let rows = self.citations.library.as_ref()?.rows();
        library::row_of_paper(&rows, id).cloned()
    }

    /// The name a paper goes by: its source's title, or its file name.
    pub(crate) fn paper_title(&self, id: &str) -> String {
        self.paper_row(id).map(|row| row.title).filter(|title| !title.is_empty()).unwrap_or_else(|| base_name(id).to_string())
    }

    pub(crate) fn choose_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static) {
        let picked = crate::system::pick_files(Some("pdf"), false, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = Self::picked(&this, picked.await, cx).and_then(|paths| paths.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, window, cx| then(w, path, window, cx));
        }).detach();
    }

    /// Copies a PDF from outside the vault in as paper `id`.
    pub(crate) fn attach_paper(&mut self, path: &Path, id: &str, cx: &mut Context<Self>) {
        let result = std::fs::File::open(path).and_then(|mut file| {
            let mut head = [0; 5];
            std::io::Read::read_exact(&mut file, &mut head)?;
            Ok(head)
        });
        if !result.is_ok_and(|head| head == *PDF_HEADER) {
            return self.show_toast_kind(ToastKind::Error, "That file is not a PDF", cx);
        }
        let result = self.store.borrow_mut().attach(path, id);
        match result {
            Ok(()) => {
                self.refresh_entries(cx);
                self.show_toast("Added the paper", cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not add the paper: {e}"), cx),
        }
        cx.notify();
    }

    /// Makes an unlinked paper, and any notes on it, the paper of `key`.
    pub(crate) fn link_paper(&mut self, id: &str, key: &str, cx: &mut Context<Self>) {
        let to = paper_file(key);
        self.follow_key(id, &to, cx);
        self.follow_key(&note_file(id), &note_file(&to), cx);
        cx.notify();
    }

    /// A paper keeps following its entry when the citation key changes.
    pub(crate) fn rename_paper(&mut self, old: &str, new: &str, cx: &mut Context<Self>) {
        self.follow_key(&paper_file(old), &paper_file(new), cx);
        self.follow_key(&note_file(&paper_file(old)), &note_file(&paper_file(new)), cx);
    }

    /// Renames a file that is named after a citation key.
    pub(crate) fn follow_key(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        if from == to || !self.entries.iter().any(|e| e.id == from) { return; }
        let name = base_name(to).to_string();
        let result = self.store.borrow_mut().rename(from, &name);
        match result {
            Ok(moved) => {
                self.path_changed(from, &moved.id, &moved.rewritten, cx);
                self.show_warnings(&moved.warnings, cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not rename {from}: {e}"), cx),
        }
    }

    /// Downloads open-access papers for `keys`, one after another, skipping
    /// entries that have one or have nothing to find it by.
    pub(crate) fn fetch_papers(&mut self, keys: Vec<String>, cx: &mut Context<Self>) {
        let Some(library) = &self.citations.library else { return };
        let email = Some(crate::preferences::prefs().unpaywall_email).filter(|email| email.split_once('@').is_some_and(|(name, host)| !name.is_empty() && host.contains('.')));
        let jobs: Vec<_> = keys.iter().filter(|key| !self.has_paper(key)).filter_map(|key| {
            let ids = library.paper_ids(key);
            let dest = self.store.borrow().path(&paper_file(key)).ok()?;
            (ids.doi.is_some() || ids.arxiv.is_some()).then_some((ids, dest))
        }).collect();
        if jobs.is_empty() {
            if keys.len() == 1 && !self.has_paper(&keys[0]) { self.show_toast_kind(ToastKind::Error, "This entry has no DOI or arXiv ID to find a paper by", cx); }
            return;
        }
        let total = jobs.len();
        self.papers.fetching += total;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (mut found, mut problem) = (0, None);
            for (ids, dest) in jobs {
                let email = email.clone();
                let result = cx.background_spawn(async move { lookup::fetch_paper(&ids, email.as_deref(), &dest) }).await;
                let downloaded = matches!(result, Ok(true));
                if downloaded { found += 1; }
                if let Err(e) = result { problem = Some(e.to_string()); }
                let _ = this.update(cx, |w, cx| {
                    w.papers.fetching -= 1;
                    if downloaded { w.refresh_entries(cx); }
                    cx.notify();
                });
            }
            let _ = this.update(cx, |w, cx| match (found, problem) {
                (0, Some(problem)) => w.show_toast_kind(ToastKind::Error, &format!("Could not download the paper: {problem}"), cx),
                (0, None) if email.is_none() => w.show_toast_kind(ToastKind::Error, "No PDF on arXiv. Add an email in Settings to search Unpaywall", cx),
                (0, None) => w.show_toast_kind(ToastKind::Error, "No open-access PDF found", cx),
                _ if total == 1 => w.show_toast("Downloaded the paper", cx),
                _ => w.show_toast(&format!("Downloaded {found} of {total} papers"), cx),
            });
        }).detach();
    }
}
