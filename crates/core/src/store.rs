//! The vault: a plain folder of `.typ` files, by default `~/Documents/Thesio`.
//! A document's id is its vault-relative path, so renaming or moving a file
//! changes its id. App state lives beside the files in `.thesio/`, and deleted
//! files move to `.trash/`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use crate::time::{now, now_ms};
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_DIR: &str = ".thesio";
const TRASH_DIR: &str = ".trash";
const EXTENSION: &str = "typ";
pub const UNTITLED: &str = "Untitled";

/// App state for a vault path. Timestamps come from the file; this holds what
/// a plain file cannot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    #[serde(skip)]
    pub created: u64,
    #[serde(skip)]
    pub modified: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// Last time the document was opened.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub opened: u64,
    /// The containing folder; derived from the path.
    #[serde(skip)]
    pub parent: Option<String>,
    /// Star time (0: not starred).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub starred: u64,
}

fn is_zero(v: &u64) -> bool { *v == 0 }


/// Window-level UI state (`.thesio/settings.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Dragged sidebar width; `None` is the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<f32>,
    /// Home: list instead of grid, and the sort order.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home_list: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_sort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_size: Option<[f32; 2]>,
    /// Folders open in the sidebar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expanded: Vec<String>,
}

/// A trashed file or folder, as kept in `.thesio/trash.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trashed {
    pub meta: Meta,
    pub deleted: u64,
    /// Its vault path before deletion.
    pub original: String,
    /// App state of the files inside a folder, by path relative to it.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub inner: HashMap<String, Meta>,
}

/// A row of the Trash page; `id` is its key in the trash, not a vault path.
#[derive(Clone, Debug)]
pub struct TrashEntry {
    pub id: String,
    pub title: String,
    pub deleted: u64,
    /// The folder it was in.
    pub parent: Option<String>,
}

/// Trash is emptied after 30 days.
pub const TRASH_DAYS: u64 = 30;

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub meta: Meta,
    pub title: String,
    pub folder: bool,
    /// A `.typ` document, as opposed to a folder or another kind of file.
    pub typst: bool,
    /// A document's text, for search and snippets.
    pub body: String,
}

struct CachedBody {
    stamp: (u64, u64),
    text: String,
}

pub struct Store {
    root: PathBuf,
    index: HashMap<String, Meta>,
    trash: HashMap<String, Trashed>,
    bodies: HashMap<String, CachedBody>,
    /// Problems found while opening the vault, for the window to show once.
    pub warnings: Vec<String>,
}

/// Version of `.thesio/`. Bump it when older builds would misread the vault.
pub const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FormatMarker {
    format: u32,
}

/// Write `bytes` to `path` through a temporary file, so a crash leaves
/// either the old or the new contents.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(suffix);
    PathBuf::from(p)
}

/// Read a JSON file written by `save_json`. One that does not parse is set
/// aside as `<name>.corrupt-<time>` and its `.bak` is used instead.
fn load_json<T: serde::de::DeserializeOwned + Default>(path: &Path, what: &str, warnings: &mut Vec<String>) -> T {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return T::default(),
        Err(e) => {
            warnings.push(format!("Could not read {what}: {e}"));
            return T::default();
        }
    };
    if let Ok(v) = serde_json::from_slice(&bytes) {
        return v;
    }
    let aside = with_suffix(path, &format!(".corrupt-{}", now()));
    let _ = std::fs::rename(path, &aside);
    let name = aside.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match std::fs::read(with_suffix(path, ".bak")).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(v) => {
            warnings.push(format!("{what} was damaged and restored from backup (original kept as {name})"));
            v
        }
        None => {
            warnings.push(format!("{what} was damaged with no usable backup, so pins and stars were reset (original kept as {name})"));
            T::default()
        }
    }
}

fn save_json<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(v)?;
    if path.exists() {
        write_atomic(&with_suffix(path, ".bak"), &std::fs::read(path)?)?;
    }
    write_atomic(path, &bytes)
}

fn unix_secs(time: std::io::Result<SystemTime>) -> u64 {
    time.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// A file name for a title: no separators or leading dots.
fn file_stem(title: &str) -> String {
    let clean: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    if clean.is_empty() { UNTITLED.to_string() } else { clean.to_string() }
}

/// Orders names as people read them: ignoring case, with runs of digits
/// compared by value, so "Chapter 2" sorts before "Chapter 10".
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(d) = it.next_if(char::is_ascii_digit) { digits.push(d); }
                    digits
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if order != Ordering::Equal { return order; }
            }
            (Some(p), Some(q)) => {
                let order = p.to_lowercase().cmp(q.to_lowercase());
                if order != Ordering::Equal { return order; }
                x.next();
                y.next();
            }
        }
    }
}

fn parent_of(id: &str) -> Option<&str> { id.rsplit_once('/').map(|(parent, _)| parent) }
fn base_name(id: &str) -> &str { id.rsplit_once('/').map_or(id, |(_, name)| name) }
fn join(parent: Option<&str>, name: &str) -> String { parent.map_or(name.to_string(), |p| format!("{p}/{name}")) }

impl Store {
    /// `THESIO_HOME`, else a folder of its own in the user's Documents.
    pub fn root_dir() -> PathBuf {
        std::env::var_os("THESIO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents"))).unwrap_or_else(|| PathBuf::from(".")).join("Thesio"))
    }

    pub fn open() -> Result<Self> {
        Self::open_at(Self::root_dir())
    }

    pub fn open_at(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join(STATE_DIR))?;
        let mut warnings = vec![];
        let format_path = root.join(STATE_DIR).join("format.json");
        match load_json::<Option<FormatMarker>>(&format_path, "the vault format", &mut warnings) {
            Some(m) if m.format > FORMAT => warnings.push("This vault was created by a newer version of Thesio. Update the app before editing it.".into()),
            Some(_) => {}
            None => save_json(&format_path, &FormatMarker { format: FORMAT })?,
        }
        let index = load_json(&root.join(STATE_DIR).join("index.json"), "the vault index", &mut warnings);
        let trash = load_json(&root.join(STATE_DIR).join("trash.json"), "the Trash index", &mut warnings);
        Ok(Self { root, index, trash, bodies: HashMap::new(), warnings })
    }

    pub fn root(&self) -> &Path { &self.root }

    /// The file behind `id`, refusing paths that leave the vault.
    pub fn path(&self, id: &str) -> Result<PathBuf> {
        let relative = Path::new(id);
        anyhow::ensure!(!id.is_empty() && relative.components().all(|c| matches!(c, Component::Normal(_))), "Invalid vault path: {id}");
        Ok(self.root.join(relative))
    }

    fn save_index(&self) -> Result<()> { save_json(&self.root.join(STATE_DIR).join("index.json"), &self.index) }
    fn save_trash(&self) -> Result<()> { save_json(&self.root.join(STATE_DIR).join("trash.json"), &self.trash) }

    /// Every visible file and folder, parents before children.
    pub fn list(&mut self) -> Vec<Entry> {
        let mut out = vec![];
        self.scan(&self.root.clone(), None, &mut out);
        out
    }

    fn scan(&mut self, dir: &Path, parent: Option<&str>, out: &mut Vec<Entry>) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut items: Vec<_> = read.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for item in items {
            let name = item.file_name().to_string_lossy().into_owned();
            let Ok(info) = item.metadata() else { continue };
            if name.starts_with('.') { continue; }
            let id = join(parent, &name);
            let mut meta = self.index.get(&id).cloned().unwrap_or_default();
            meta.created = unix_secs(info.created());
            meta.modified = unix_secs(info.modified());
            meta.parent = parent.map(str::to_string);
            if info.is_dir() {
                out.push(Entry { id: id.clone(), meta, title: name, folder: true, typst: false, body: String::new() });
                self.scan(&item.path(), Some(&id), out);
            } else if !item.path().extension().is_some_and(|e| e == EXTENSION) {
                out.push(Entry { id, meta, title: name, folder: false, typst: false, body: String::new() });
            } else {
                let stamp = (meta.modified, info.len());
                let fresh = self.bodies.get(&id).filter(|b| b.stamp == stamp).map(|b| b.text.clone());
                let body = fresh.unwrap_or_else(|| {
                    let text = std::fs::read_to_string(item.path()).unwrap_or_default();
                    self.bodies.insert(id.clone(), CachedBody { stamp, text: text.clone() });
                    text
                });
                let title = name.strip_suffix(".typ").unwrap_or(&name).to_string();
                out.push(Entry { id, meta, title, folder: false, typst: true, body });
            }
        }
    }

    pub fn load(&self, id: &str) -> Result<String> {
        std::fs::read_to_string(self.path(id)?).with_context(|| format!("Could not read {id}"))
    }

    pub fn save(&mut self, id: &str, text: &str) -> Result<()> {
        write_atomic(&self.path(id)?, text.as_bytes())
    }

    /// A free name for `stem` in `parent`.
    fn free_id(&self, parent: Option<&str>, stem: &str, extension: Option<&str>) -> Result<String> {
        let name = |n: u32| {
            let numbered = if n == 0 { stem.to_string() } else { format!("{stem} {n}") };
            extension.map_or(numbered.clone(), |e| format!("{numbered}.{e}"))
        };
        for n in 0..10_000 {
            let id = join(parent, &name(n));
            if !self.path(&id)?.try_exists()? { return Ok(id); }
        }
        anyhow::bail!("Too many files named {stem}")
    }

    /// A new empty document.
    pub fn create(&mut self, parent: Option<&str>) -> Result<String> {
        self.create_from(parent, UNTITLED, "")
    }

    pub fn create_from(&mut self, parent: Option<&str>, title: &str, text: &str) -> Result<String> {
        let id = self.free_id(parent, &file_stem(title), Some(EXTENSION))?;
        write_atomic(&self.path(&id)?, text.as_bytes())?;
        Ok(id)
    }

    pub fn create_folder(&mut self, parent: Option<&str>, title: &str) -> Result<String> {
        let id = self.free_id(parent, &file_stem(title), None)?;
        std::fs::create_dir(self.path(&id)?)?;
        Ok(id)
    }

    pub fn size_of(&self, id: &str) -> u64 {
        self.path(id).ok().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len())
    }

    /// Moves `old` to `new`, carrying app state, including for a folder's files,
    /// and updates the paths in vault documents. Returns the documents rewritten.
    fn relocate(&mut self, old: &str, new: &str) -> Result<Vec<String>> {
        if old == new { return Ok(vec![]); }
        let target = self.path(new)?;
        // A change of case alone names the same file on case-insensitive disks.
        anyhow::ensure!(old.to_lowercase() == new.to_lowercase() || !target.try_exists()?, "{new} already exists");
        std::fs::rename(self.path(old)?, target)?;
        let prefix = format!("{old}/");
        let moved: Vec<String> = self.index.keys().filter(|k| *k == old || k.starts_with(&prefix)).cloned().collect();
        for key in moved {
            if let Some(meta) = self.index.remove(&key) {
                self.index.insert(format!("{new}{}", &key[old.len()..]), meta);
            }
        }
        self.save_index()?;
        Ok(self.rewrite_references(old, new))
    }

    /// Fixes paths that the move from `old` to `new` broke. A file that cannot
    /// be rewritten keeps its text and raises a warning.
    fn rewrite_references(&mut self, old: &str, new: &str) -> Vec<String> {
        let swap = |path: &str, from: &str, to: &str| {
            if path == from { return Some(to.to_string()); }
            path.strip_prefix(from)?.strip_prefix('/').map(|rest| format!("{to}/{rest}"))
        };
        let mut rewritten = vec![];
        for entry in self.list().into_iter().filter(|e| e.typst) {
            let Ok(text) = self.load(&entry.id) else { continue };
            let previous = swap(&entry.id, new, old).unwrap_or_else(|| entry.id.clone());
            let Some(updated) = crate::references::rewrite_paths(&text, &previous, &entry.id, |path| swap(path, old, new)) else { continue };
            match self.save(&entry.id, &updated) {
                Ok(()) => rewritten.push(entry.id),
                Err(e) => self.warnings.push(format!("Could not update paths in {}: {e}", entry.id)),
            }
        }
        rewritten
    }

    /// Copies the outside file `from` to `id`, never over an existing file.
    pub fn attach(&mut self, from: &Path, id: &str) -> Result<()> {
        let target = self.path(id)?;
        anyhow::ensure!(!target.try_exists()?, "{id} already exists");
        if let Some(dir) = target.parent() { std::fs::create_dir_all(dir)?; }
        std::fs::copy(from, target)?;
        Ok(())
    }

    /// Renames a file or folder in place. Returns its new id and the documents
    /// whose paths were updated.
    pub fn rename(&mut self, id: &str, title: &str) -> Result<(String, Vec<String>)> {
        // A document's title leaves out the extension; other names are whole.
        let typst = self.path(id)?.is_file() && id.ends_with(".typ");
        let name = if typst { format!("{}.{EXTENSION}", file_stem(title)) } else { file_stem(title) };
        let new = join(parent_of(id), &name);
        let rewritten = self.relocate(id, &new)?;
        Ok((new, rewritten))
    }

    /// Moves a file or folder into `parent`. Returns its new id and the
    /// documents whose paths were updated.
    pub fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<(String, Vec<String>)> {
        if let Some(parent) = parent {
            anyhow::ensure!(parent != id && !parent.starts_with(&format!("{id}/")), "A folder cannot move into itself");
        }
        let new = join(parent, base_name(id));
        let rewritten = self.relocate(id, &new)?;
        Ok((new, rewritten))
    }

    /// Moves a file or folder to the Trash.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        let from = self.path(id)?;
        let mut stamp = now_ms();
        // Items deleted together can share a millisecond.
        while self.trash.contains_key(&stamp.to_string()) || self.root.join(TRASH_DIR).join(stamp.to_string()).exists() { stamp += 1; }
        let key = stamp.to_string();
        let dir = self.root.join(TRASH_DIR).join(&key);
        std::fs::create_dir_all(&dir)?;
        let meta = self.index.get(id).cloned().unwrap_or_default();
        // Record it first: an interrupted move leaves an extra entry, never a lost file.
        let prefix = format!("{id}/");
        let inner = self.index.iter().filter_map(|(k, m)| Some((k.strip_prefix(&prefix)?.to_string(), m.clone()))).collect();
        self.trash.insert(key.clone(), Trashed { meta, deleted: now(), original: id.to_string(), inner });
        self.save_trash()?;
        if let Err(error) = std::fs::rename(from, dir.join(base_name(id))) {
            self.trash.remove(&key);
            let _ = self.save_trash();
            return Err(error.into());
        }
        self.index.retain(|k, _| k != id && !k.starts_with(&prefix));
        if let Err(e) = self.save_index() {
            self.warnings.push(format!("Moved to Trash, but the vault index could not be cleaned up: {e}"));
        }
        Ok(())
    }

    /// Trash contents, newest first, dropping what is past its time.
    pub fn list_trash(&mut self) -> Vec<TrashEntry> {
        let cutoff = now().saturating_sub(TRASH_DAYS * 24 * 3600);
        let expired: Vec<String> = self.trash.iter().filter(|(_, t)| t.deleted < cutoff).map(|(k, _)| k.clone()).collect();
        for key in expired {
            if let Err(e) = self.purge(&key) { self.warnings.push(format!("Could not empty Trash: {e}")); }
        }
        let mut out: Vec<TrashEntry> = self.trash.iter().map(|(key, t)| TrashEntry {
            id: key.clone(),
            title: base_name(&t.original).strip_suffix(".typ").unwrap_or(base_name(&t.original)).to_string(),
            deleted: t.deleted,
            parent: parent_of(&t.original).map(str::to_string),
        }).collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.deleted));
        out
    }

    /// Puts a trashed item back, under a new name when its old one is taken.
    /// Returns true in that case.
    pub fn restore(&mut self, key: &str) -> Result<bool> {
        let entry = self.trash.get(key).cloned().context("Not in Trash")?;
        let original = &entry.original;
        let parent = parent_of(original);
        let mut target = original.clone();
        let taken = self.path(original)?.try_exists()?;
        if taken {
            let name = base_name(original);
            let (stem, extension) = match name.rsplit_once('.') { Some((s, e)) => (s, Some(e)), None => (name, None) };
            target = self.free_id(parent, stem, extension)?;
        }
        if let Some(parent) = parent { std::fs::create_dir_all(self.path(parent)?)?; }
        let dir = self.root.join(TRASH_DIR).join(key);
        std::fs::rename(dir.join(base_name(original)), self.path(&target)?)?;
        let _ = std::fs::remove_dir(dir);
        for (relative, meta) in entry.inner {
            self.index.insert(format!("{target}/{relative}"), meta);
        }
        self.index.insert(target, entry.meta);
        self.trash.remove(key);
        self.save_index()?;
        if let Err(e) = self.save_trash() {
            self.warnings.push(format!("Restored, but the Trash index could not be cleaned up: {e}"));
        }
        Ok(taken)
    }

    /// Permanently deletes a trashed item.
    pub fn purge(&mut self, key: &str) -> Result<()> {
        anyhow::ensure!(self.trash.contains_key(key), "Not in Trash");
        match std::fs::remove_dir_all(self.root.join(TRASH_DIR).join(key)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        self.trash.remove(key);
        self.save_trash()
    }

    /// Copies a file into the vault root and returns its vault path.
    pub fn import(&mut self, path: &Path) -> Result<String> {
        let stem = path.file_stem().map_or(UNTITLED.into(), |s| file_stem(&s.to_string_lossy()));
        let extension = path.extension().map(|e| e.to_string_lossy().into_owned());
        let id = self.free_id(None, &stem, extension.as_deref())?;
        std::fs::copy(path, self.path(&id)?)?;
        Ok(id)
    }

    pub fn update_meta(&mut self, id: &str, f: impl FnOnce(&mut Meta)) -> Result<()> {
        f(self.index.entry(id.to_string()).or_default());
        self.save_index()
    }

    pub fn settings(&self) -> Settings {
        let mut warnings = vec![];
        load_json(&self.root.join(STATE_DIR).join("settings.json"), "settings", &mut warnings)
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        save_json(&self.root.join(STATE_DIR).join("settings.json"), settings)
    }

    /// Records that a document was opened.
    pub fn touch(&mut self, id: &str) -> Result<()> {
        self.update_meta(id, |m| m.opened = now())
    }

    pub fn set_starred(&mut self, id: &str, on: bool) -> Result<u64> {
        let time = if on { now() } else { 0 };
        self.update_meta(id, |m| m.starred = time)?;
        Ok(time)
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> {
        self.update_meta(id, |m| m.pinned = pinned)
    }
}
