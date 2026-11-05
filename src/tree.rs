//! The vault as a tree: folders first, then names in natural order. Also the
//! expand state, new files inside folders, and the breadcrumb chain.

mod move_dialog;

pub(crate) use move_dialog::MoveDialog;

use crate::app::Workspace;
use crate::store::Entry;
use thesio_core::paths::natural_cmp;
use gpui::*;
use std::collections::{HashMap, HashSet};

/// Children of every folder, keyed by parent (`None`: the vault root). An
/// entry whose parent is missing is shown at the top level.
pub(crate) struct Tree<'a> {
    roots: Vec<&'a Entry>,
    kids: HashMap<&'a str, Vec<&'a Entry>>,
}

impl<'a> Tree<'a> {
    pub fn new(entries: &'a [Entry]) -> Self {
        let ids: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        let parent = |e: &'a Entry| e.parent.as_deref().filter(|p| ids.contains(p));
        let mut roots: Vec<&Entry> = vec![];
        let mut kids: HashMap<&str, Vec<&Entry>> = HashMap::new();
        for e in entries {
            match parent(e) {
                Some(p) => kids.entry(p).or_default().push(e),
                None => roots.push(e),
            }
        }
        let order = |a: &&Entry, b: &&Entry| b.folder.cmp(&a.folder).then_with(|| natural_cmp(&a.title, &b.title));
        roots.sort_by(order);
        for v in kids.values_mut() {
            v.sort_by(order);
        }
        Tree { roots, kids }
    }

    pub fn children(&self, id: Option<&str>) -> &[&'a Entry] {
        match id {
            None => &self.roots,
            Some(id) => self.kids.get(id).map_or(&[], |v| v.as_slice()),
        }
    }


    /// Pre-order rows with their depth, descending into pages `open` says yes to.
    pub fn rows(&self, open: impl Fn(&str) -> bool) -> Vec<(&'a Entry, usize)> {
        let mut out = vec![];
        let mut stack: Vec<(&Entry, usize)> = self.children(None).iter().rev().map(|e| (*e, 0)).collect();
        while let Some((e, d)) = stack.pop() {
            out.push((e, d));
            if open(&e.id) {
                stack.extend(self.children(Some(&e.id)).iter().rev().map(|c| (*c, d + 1)));
            }
        }
        out
    }

    /// `id` and every page under it.
    pub fn subtree(&self, id: &str) -> Vec<String> {
        let mut out = vec![id.to_string()];
        let mut i = 0;
        while i < out.len() {
            let kids: Vec<String> = self.children(Some(&out[i])).iter().map(|e| e.id.clone()).collect();
            out.extend(kids);
            i += 1;
        }
        out
    }
}

impl Workspace {
    /// Ancestors of `id`, top level first.
    pub(crate) fn ancestors(&self, id: &str) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = vec![];
        let mut cur = self.entries.iter().find(|e| e.id == id);
        while let Some(p) = cur.and_then(|e| e.parent.as_deref()).and_then(|p| self.entries.iter().find(|e| e.id == p)) {
            if p.id == id || out.iter().any(|e| e.id == p.id) {
                break;
            }
            out.push(p);
            cur = Some(p);
        }
        out.reverse();
        out
    }

    /// Expand the sidebar down to `id`.
    pub(crate) fn reveal_in_tree(&mut self, id: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.ancestors(id).iter().map(|e| e.id.clone()).collect();
        for id in ids { self.set_expanded(&id, true, cx); }
    }


    /// A new document in the folder `parent`, or beside it when it is a file.
    pub fn new_child(&mut self, parent: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Only folders hold files: a file's "+" adds a sibling.
        let parent = self.entries.iter().find(|e| e.id == parent).and_then(|e| if e.folder { Some(e.id.clone()) } else { e.parent.clone() });
        self.when_saved(window, cx, move |w, window, cx| w.create_doc(parent.as_deref(), window, cx));
    }

    /// Opens or closes a folder and remembers it for the next launch.
    pub(crate) fn set_expanded(&mut self, id: &str, open: bool, cx: &mut Context<Self>) {
        let changed = if open { self.sidebar.expanded.insert(id.to_string()) } else { self.sidebar.expanded.remove(id) };
        if changed { self.save_expanded(cx); }
    }

    pub(crate) fn toggle_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        self.set_expanded(id, !self.sidebar.expanded.contains(id), cx);
        cx.notify();
    }

    pub(crate) fn save_expanded(&mut self, cx: &mut Context<Self>) {
        let mut expanded: Vec<String> = self.sidebar.expanded.iter().cloned().collect();
        expanded.sort();
        let result = self.store.borrow().update_settings(|s| s.expanded = expanded);
        if let Err(e) = result { self.show_toast_kind(crate::ui::ToastKind::Error, &format!("Could not remember open folders: {e}"), cx); }
    }
}
