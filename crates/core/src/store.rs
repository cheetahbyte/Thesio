//! The vault: a plain folder of `.typ` files, by default `~/Documents/Thesio`.
//! A document's id is its vault-relative path, so renaming or moving a file
//! changes its id. App state lives beside the files in `.thesio/`, and deleted
//! files move to `.trash/`.

use crate::history::Versions;
use crate::images::{IMAGES_DIR, Incoming};
use crate::paths::{self, EXTENSION, STATE_DIR, VaultItem, base_name, document_name, document_title, is_document, join, parent_of, visit_vault, write_atomic};
use crate::project::Documents;
use crate::references::rewrite_paths;
use crate::typst_source::relative;
use crate::time::{DAY, now, now_ms};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

const TRASH_DIR: &str = ".trash";
/// How far numbering a taken name goes before giving up.
const MAX_NUMBERED: u32 = 10_000;
const FORMAT_FILE: &str = "format.json";
const INDEX_FILE: &str = "index.json";
const TRASH_FILE: &str = "trash.json";
const SETTINGS_FILE: &str = "settings.json";
pub const UNTITLED: &str = "Untitled";

/// How a title reads: trimmed, and never blank.
pub fn display_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() { UNTITLED.to_string() } else { title.to_string() }
}

/// App state for a vault path. Timestamps come from the file; this holds what
/// a plain file cannot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub opened: u64,
    /// Star time (0: not starred).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub starred: u64,
    /// A PDF's printed page numbers minus the ones it reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_offset: Option<i32>,
}

fn is_zero(v: &u64) -> bool { *v == 0 }


/// Window-level UI state (`.thesio/settings.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Dragged sidebar width; `None` is the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<f32>,
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

pub const TRASH_DAYS: u64 = 30;

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub meta: Meta,
    /// The file's own time.
    pub modified: u64,
    pub parent: Option<String>,
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
}

/// The vault as the compiler saw it before a move.
struct Layout {
    files: HashSet<String>,
    documents: Documents,
}

/// A rename or move that went through.
#[derive(Debug)]
pub struct Moved {
    pub id: String,
    /// The documents whose paths were updated.
    pub rewritten: Vec<String>,
    /// What went wrong without stopping it.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct Restored {
    /// It came back under a new name because its old one was taken.
    pub renamed: bool,
    pub warnings: Vec<String>,
}

/// Version of `.thesio/`. Bump it when older builds would misread the vault.
pub const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FormatMarker {
    format: u32,
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

/// A file name for a title: no separators or leading dots.
fn file_stem(title: &str) -> String {
    let clean: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    if clean.is_empty() { UNTITLED.to_string() } else { clean.to_string() }
}

/// A file name from a title. A document's title may leave out the extension;
/// other names are whole.
fn renamed_file(title: &str, typst: bool) -> String {
    let stem = file_stem(title);
    if !typst { return stem; }
    document_name(document_title(&stem))
}

/// `stem`'s `n`th name, for when the ones before it are taken.
fn numbered_name(stem: &str, extension: Option<&str>, n: u32) -> String {
    let numbered = if n == 0 { stem.to_string() } else { format!("{stem} {n}") };
    extension.map_or(numbered.clone(), |e| format!("{numbered}.{e}"))
}

impl Store {
    /// `THESIO_HOME`, else a folder of its own in the user's Documents.
    pub fn root_dir() -> PathBuf {
        std::env::var_os("THESIO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents"))).unwrap_or_else(|| PathBuf::from(".")).join("Thesio"))
    }

    /// The vault, and problems found opening it, for the window to show once.
    pub fn open() -> Result<(Self, Vec<String>)> {
        Self::open_at(Self::root_dir())
    }

    pub fn open_at(root: PathBuf) -> Result<(Self, Vec<String>)> {
        std::fs::create_dir_all(root.join(STATE_DIR))?;
        let state = |file: &str| root.join(STATE_DIR).join(file);
        let mut warnings = vec![];
        let format_path = state(FORMAT_FILE);
        match load_json::<Option<FormatMarker>>(&format_path, "the vault format", &mut warnings) {
            Some(m) if m.format > FORMAT => warnings.push("This vault was created by a newer version of Thesio. Update the app before editing it.".into()),
            Some(_) => {}
            None => save_json(&format_path, &FormatMarker { format: FORMAT })?,
        }
        let index = load_json(&state(INDEX_FILE), "the vault index", &mut warnings);
        let trash = load_json(&state(TRASH_FILE), "the Trash index", &mut warnings);
        Ok((Self { root, index, trash, bodies: HashMap::new() }, warnings))
    }

    pub fn root(&self) -> &Path { &self.root }

    /// The file behind `id`, refusing paths that leave the vault.
    pub fn path(&self, id: &str) -> Result<PathBuf> {
        let relative = Path::new(id);
        anyhow::ensure!(!id.is_empty() && relative.components().all(|c| matches!(c, Component::Normal(_))), "Invalid vault path: {id}");
        Ok(self.root.join(relative))
    }

    fn state_file(&self, file: &str) -> PathBuf { self.root.join(STATE_DIR).join(file) }
    fn save_index(&self) -> Result<()> { save_json(&self.state_file(INDEX_FILE), &self.index) }
    fn save_trash(&self) -> Result<()> { save_json(&self.state_file(TRASH_FILE), &self.trash) }

    /// Every visible file and folder, parents before children.
    pub fn list(&mut self) -> Vec<Entry> {
        let mut out = vec![];
        visit_vault(&self.root.clone(), &mut |item| out.push(self.entry(item)));
        out
    }

    fn entry(&mut self, item: VaultItem) -> Entry {
        let meta = self.index.get(&item.id).cloned().unwrap_or_default();
        let modified = item.modified();
        let (document, folder, length) = (item.is_document(), item.info.is_dir(), item.info.len());
        let VaultItem { id, name, parent, path, .. } = item;
        if !document { return Entry { id, meta, modified, parent, title: name, folder, typst: false, body: String::new() }; }
        let stamp = (modified, length);
        let fresh = self.bodies.get(&id).filter(|b| b.stamp == stamp).map(|b| b.text.clone());
        let body = fresh.unwrap_or_else(|| {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            self.bodies.insert(id.clone(), CachedBody { stamp, text: text.clone() });
            text
        });
        Entry { id, meta, modified, parent, title: document_title(&name).to_string(), folder, typst: true, body }
    }

    pub fn load(&self, id: &str) -> Result<String> {
        std::fs::read_to_string(self.path(id)?).with_context(|| format!("Could not read {id}"))
    }

    pub fn save(&mut self, id: &str, text: &str) -> Result<()> {
        // A second write can leave the time and size `list` compares unchanged.
        self.bodies.remove(id);
        write_atomic(&self.path(id)?, text.as_bytes())
    }

    /// `id`'s earlier versions.
    pub fn versions(&self, id: &str) -> Result<Versions> {
        self.path(id)?;
        Ok(Versions::of(&self.root, id))
    }

    /// A free name for `stem` in `parent`.
    fn free_id(&self, parent: Option<&str>, stem: &str, extension: Option<&str>) -> Result<String> {
        for n in 0..MAX_NUMBERED {
            let id = join(parent, &numbered_name(stem, extension, n));
            if !self.path(&id)?.try_exists()? { return Ok(id); }
        }
        anyhow::bail!("Too many files named {stem}")
    }

    pub fn create(&mut self, parent: Option<&str>) -> Result<String> {
        self.create_from(parent, UNTITLED, "")
    }

    pub fn create_from(&mut self, parent: Option<&str>, title: &str, text: &str) -> Result<String> {
        let id = self.free_id(parent, &file_stem(title), Some(EXTENSION))?;
        self.save(&id, text)?;
        Ok(id)
    }

    pub fn create_folder(&mut self, parent: Option<&str>, title: &str) -> Result<String> {
        let id = self.free_id(parent, &file_stem(title), None)?;
        std::fs::create_dir(self.path(&id)?)?;
        Ok(id)
    }

    /// Moves `old` to `new`, carrying app state, including for a folder's files,
    /// and updates the paths in vault documents.
    fn relocate(&mut self, old: &str, new: &str) -> Result<Moved> {
        let mut outcome = Moved { id: new.to_string(), rewritten: vec![], warnings: vec![] };
        if old == new { return Ok(outcome); }
        let target = self.path(new)?;
        // A change of case alone names the same file on case-insensitive disks.
        anyhow::ensure!(old.to_lowercase() == new.to_lowercase() || !target.try_exists()?, "{new} already exists");
        let before = self.layout();
        std::fs::rename(self.path(old)?, target)?;
        // A file's versions follow it. Losing them must not fail the move.
        if let Err(e) = self.versions(old)?.move_to(&self.versions(new)?) { outcome.warnings.push(format!("Could not move the history of {old}: {e}")); }
        let moved: Vec<String> = self.index.keys().filter(|k| paths::is_within(k, old)).cloned().collect();
        for key in moved {
            if let Some(meta) = self.index.remove(&key) && let Some(renamed) = paths::moved(&key, old, new) { self.index.insert(renamed, meta); }
        }
        self.save_index()?;
        self.rewrite_references(old, new, &before, &mut outcome);
        Ok(outcome)
    }

    fn layout(&mut self) -> Layout {
        let entries = self.list();
        let bodies: HashMap<&str, &str> = entries.iter().filter(|e| e.typst).map(|e| (e.id.as_str(), e.body.as_str())).collect();
        let documents = Documents::load(entries.iter().filter(|e| e.typst).map(|e| e.id.as_str()), &mut |path| bodies.get(path).map(|body| body.to_string()));
        Layout { files: entries.iter().filter(|e| !e.folder).map(|e| e.id.clone()).collect(), documents }
    }

    /// Fixes paths that the move from `old` to `new` broke. A file that cannot
    /// be rewritten keeps its text and raises a warning.
    fn rewrite_references(&mut self, old: &str, new: &str, before: &Layout, outcome: &mut Moved) {
        for entry in self.list().into_iter().filter(|e| e.typst) {
            let text = match self.load(&entry.id) {
                Ok(text) => text,
                Err(e) => {
                    outcome.warnings.push(format!("Could not update paths in {}: {e}", entry.id));
                    continue;
                }
            };
            let previous = paths::moved(&entry.id, new, old).unwrap_or_else(|| entry.id.clone());
            let main = before.documents.top_document(&previous);
            let Some(updated) = rewrite_paths(&text, &previous, &entry.id, &main, |path| before.files.contains(path), |path| paths::moved(path, old, new)) else { continue };
            match self.save(&entry.id, &updated) {
                Ok(()) => outcome.rewritten.push(entry.id),
                Err(e) => outcome.warnings.push(format!("Could not update paths in {}: {e}", entry.id)),
            }
        }
    }

    /// Copies the outside file `from` to `id`, never over an existing file.
    pub fn attach(&mut self, from: &Path, id: &str) -> Result<()> {
        let target = self.path(id)?;
        anyhow::ensure!(!target.try_exists()?, "{id} already exists");
        if let Some(dir) = target.parent() { std::fs::create_dir_all(dir)?; }
        std::fs::copy(from, target)?;
        Ok(())
    }

    /// Copies `image` into `images/` beside `document` and returns its path as
    /// the document writes it. A file with the same bytes is used as it is, and
    /// no other file is replaced.
    pub fn add_image(&mut self, document: &str, image: &Incoming) -> Result<String> {
        let image = image.read()?;
        let folder = join(parent_of(document), IMAGES_DIR);
        let stem = file_stem(&image.stem);
        std::fs::create_dir_all(self.path(&folder)?)?;
        for n in 0..MAX_NUMBERED {
            let id = join(Some(&folder), &numbered_name(&stem, Some(&image.extension), n));
            let path = self.path(&id)?;
            let same = match std::fs::symlink_metadata(&path) {
                Ok(found) => found.is_file() && found.len() == image.bytes.len() as u64 && std::fs::read(&path)? == image.bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    write_atomic(&path, &image.bytes)?;
                    true
                }
                Err(e) => return Err(e.into()),
            };
            if same { return Ok(relative(&paths::dir_parts(document), &id)); }
        }
        anyhow::bail!("Too many files named {stem}")
    }

    pub fn rename(&mut self, id: &str, title: &str) -> Result<Moved> {
        let typst = self.path(id)?.is_file() && is_document(id);
        self.relocate(id, &join(parent_of(id), &renamed_file(title, typst)))
    }

    pub fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<Moved> {
        if let Some(parent) = parent {
            anyhow::ensure!(!paths::is_within(parent, id), "A folder cannot move into itself");
        }
        self.relocate(id, &join(parent, base_name(id)))
    }

    /// Moves a file or folder to the Trash. Returns what went wrong without stopping it.
    pub fn delete(&mut self, id: &str) -> Result<Vec<String>> {
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
        self.index.retain(|k, _| !paths::is_within(k, id));
        Ok(self.save_index().err().map(|e| format!("Moved to Trash, but the vault index could not be cleaned up: {e}")).into_iter().collect())
    }

    /// Permanently deletes what is past its time in the Trash. Returns what could not be.
    pub fn purge_expired(&mut self) -> Vec<String> {
        let cutoff = now().saturating_sub(TRASH_DAYS * DAY);
        let expired: Vec<String> = self.trash.iter().filter(|(_, t)| t.deleted < cutoff).map(|(k, _)| k.clone()).collect();
        expired.iter().filter_map(|key| self.purge(key).err()).map(|e| format!("Could not empty Trash: {e}")).collect()
    }

    /// Trash contents, newest first.
    pub fn list_trash(&self) -> Vec<TrashEntry> {
        let mut out: Vec<TrashEntry> = self.trash.iter().map(|(key, t)| TrashEntry {
            id: key.clone(),
            title: document_title(base_name(&t.original)).to_string(),
            deleted: t.deleted,
            parent: parent_of(&t.original).map(str::to_string),
        }).collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.deleted));
        out
    }

    /// Puts a trashed item back, under a new name when its old one is taken.
    pub fn restore(&mut self, key: &str) -> Result<Restored> {
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
        let warnings = self.save_trash().err().map(|e| format!("Restored, but the Trash index could not be cleaned up: {e}")).into_iter().collect();
        Ok(Restored { renamed: taken, warnings })
    }

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
        load_json(&self.state_file(SETTINGS_FILE), "settings", &mut warnings)
    }

    /// Changes the settings as they are on disk: other windows and copies write them too.
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut settings = self.settings();
        change(&mut settings);
        save_json(&self.state_file(SETTINGS_FILE), &settings)
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

    pub fn set_page_offset(&mut self, id: &str, offset: i32) -> Result<()> {
        self.update_meta(id, |m| m.page_offset = (offset != 0).then_some(offset))
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> {
        self.update_meta(id, |m| m.pinned = pinned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(name: &str) -> Store {
        let root = std::env::temp_dir().join(format!("thesio-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Store::open_at(root).unwrap().0
    }

    #[test]
    fn a_save_shows_in_the_list_even_within_the_same_second_and_size() {
        let mut store = vault("bodies");
        let id = store.create_from(None, "doc", "aaa").unwrap();
        assert_eq!(store.list()[0].body, "aaa");
        store.save(&id, "bbb").unwrap();
        assert_eq!(store.list()[0].body, "bbb");
    }

    /// A vault holding a project in `paper/`, and a file at the root with the same name as one in it.
    fn project_vault(name: &str) -> Store {
        let mut store = vault(name);
        store.create_folder(None, "paper").unwrap();
        store.create_folder(Some("paper"), "lib").unwrap();
        store.create_folder(Some("paper"), "tools").unwrap();
        store.create_folder(None, "lib").unwrap();
        store.create_from(Some("paper"), "main", "#include \"chapter.typ\"\n").unwrap();
        store.create_from(Some("paper"), "chapter", "#import \"/lib/shared.typ\"\n").unwrap();
        store.create_from(Some("paper/lib"), "shared", "").unwrap();
        store.create_from(Some("lib"), "shared", "").unwrap();
        store
    }

    #[test]
    fn a_rooted_path_follows_the_file_the_compiler_finds() {
        let mut store = project_vault("rooted-found");
        let moved = store.set_parent("paper/lib/shared.typ", Some("paper/tools")).unwrap();
        assert_eq!(moved.rewritten, ["paper/chapter.typ"]);
        assert_eq!(store.load("paper/chapter.typ").unwrap(), "#import \"/tools/shared.typ\"\n");
    }

    #[test]
    fn a_rooted_path_ignores_a_file_it_does_not_reach() {
        let mut store = project_vault("rooted-shadowed");
        let moved = store.set_parent("lib/shared.typ", Some("paper/tools")).unwrap();
        assert!(moved.rewritten.is_empty());
        assert_eq!(store.load("paper/chapter.typ").unwrap(), "#import \"/lib/shared.typ\"\n");
    }

    #[test]
    fn a_rooted_path_names_the_vault_root_when_its_project_is_left_behind() {
        let mut store = project_vault("rooted-left");
        store.create_from(Some("paper"), "note", "#import \"/lib/shared.typ\"\n").unwrap();
        store.set_parent("paper/note.typ", None).unwrap();
        assert_eq!(store.load("note.typ").unwrap(), "#import \"/paper/lib/shared.typ\"\n");
    }

    #[test]
    fn listing_the_trash_deletes_nothing() {
        let mut store = vault("trash");
        let id = store.create(None).unwrap();
        store.delete(&id).unwrap();
        let root = store.root().to_path_buf();
        let index = root.join(STATE_DIR).join(TRASH_FILE);
        let mut trash: serde_json::Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
        for entry in trash.as_object_mut().unwrap().values_mut() { entry["deleted"] = 1.into(); }
        std::fs::write(&index, trash.to_string()).unwrap();
        let (mut store, _) = Store::open_at(root).unwrap();
        assert_eq!(store.list_trash().len(), 1);
        assert_eq!(store.purge_expired(), Vec::<String>::new());
        assert!(store.list_trash().is_empty());
    }

    #[test]
    fn a_rename_updates_the_documents_that_include_it() {
        let mut store = vault("moved");
        let id = store.create_from(None, "a", "x").unwrap();
        store.create_from(None, "b", "#include \"a.typ\"").unwrap();
        let moved = store.rename(&id, "c").unwrap();
        assert_eq!((moved.id.as_str(), moved.rewritten.as_slice()), ("c.typ", ["b.typ".to_string()].as_slice()));
        assert_eq!(store.load("b.typ").unwrap(), "#include \"c.typ\"");
    }

    #[test]
    fn a_move_warns_about_a_document_it_cannot_read() {
        let mut store = vault("unreadable");
        let id = store.create_from(None, "a", "x").unwrap();
        std::fs::write(store.root().join("b.typ"), [0xff, 0xfe]).unwrap();
        let moved = store.rename(&id, "c").unwrap();
        assert_eq!(moved.warnings.len(), 1, "{:?}", moved.warnings);
        assert!(moved.warnings[0].contains("b.typ"), "{:?}", moved.warnings);
    }

    #[test]
    fn a_document_name_gets_one_extension() {
        let mut store = vault("names");
        let id = store.create_from(None, "a", "").unwrap();
        let bare = store.rename(&id, "flaky2024").unwrap().id;
        assert_eq!(bare, "flaky2024.typ");
        assert_eq!(store.rename(&bare, "other.typ").unwrap().id, "other.typ");
    }

    #[test]
    fn another_file_keeps_the_name_it_is_given() {
        let mut store = vault("other-names");
        std::fs::write(store.root().join("paper.pdf"), "%PDF-").unwrap();
        assert_eq!(store.rename("paper.pdf", "notes.pdf").unwrap().id, "notes.pdf");
    }
}
