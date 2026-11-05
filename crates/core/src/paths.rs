//! Names and paths in a vault, and writing a file whole. A vault path is
//! relative to the vault with `/` between folders.

use crate::time::unix_secs;
use anyhow::Result;
use std::cmp::Ordering;
use std::io::Write;
use std::path::{Path, PathBuf};

/// App state beside the files.
pub const STATE_DIR: &str = ".thesio";
pub const EXTENSION: &str = "typ";

/// Write `bytes` to `path` through a temporary file, so a crash leaves
/// either the old or the new contents.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
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

pub fn parent_of(path: &str) -> Option<&str> { path.rsplit_once('/').map(|(parent, _)| parent) }
pub fn base_name(path: &str) -> &str { path.rsplit_once('/').map_or(path, |(_, name)| name) }
pub fn join(parent: Option<&str>, name: &str) -> String { parent.map_or(name.to_string(), |p| format!("{p}/{name}")) }

/// The folders above `path`, as segments from the top.
pub fn dir_parts(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

/// Whether `path` is `ancestor` or lies inside it.
pub fn is_within(path: &str, ancestor: &str) -> bool {
    path.strip_prefix(ancestor).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Where `path` ends up when `from` moves to `to`, if that moves it.
pub fn moved(path: &str, from: &str, to: &str) -> Option<String> {
    is_within(path, from).then(|| format!("{to}{}", &path[from.len()..]))
}

/// The stem of a document's file name.
fn document_stem(name: &str) -> Option<&str> {
    name.rsplit_once('.').filter(|(_, extension)| *extension == EXTENSION).map(|(stem, _)| stem)
}

/// Whether a file name or path names a Typst document.
pub fn is_document(name: &str) -> bool { document_stem(name).is_some() }
/// A file's title: its name, without the extension if it is a document.
pub fn document_title(name: &str) -> &str { document_stem(name).unwrap_or(name) }
pub fn document_name(stem: &str) -> String { format!("{stem}.{EXTENSION}") }

/// A visible file or folder of a vault.
pub(crate) struct VaultItem {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub path: PathBuf,
    pub info: std::fs::Metadata,
}

impl VaultItem {
    pub fn is_document(&self) -> bool { !self.info.is_dir() && is_document(&self.name) }
    pub fn modified(&self) -> u64 { unix_secs(self.info.modified()) }
}

/// Visits every visible file and folder under `root`, parents before children.
pub(crate) fn visit_vault(root: &Path, visit: &mut dyn FnMut(VaultItem)) {
    fn walk(dir: &Path, parent: Option<&str>, visit: &mut dyn FnMut(VaultItem)) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut items: Vec<_> = read.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for item in items {
            let name = item.file_name().to_string_lossy().into_owned();
            let Ok(info) = item.metadata() else { continue };
            if name.starts_with('.') { continue; }
            let id = join(parent, &name);
            let folder = info.is_dir();
            visit(VaultItem { id: id.clone(), name, parent: parent.map(str::to_string), path: item.path(), info });
            if folder { walk(&item.path(), Some(&id), visit); }
        }
    }
    walk(root, None, visit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_contains_only_its_own_children() {
        assert!(is_within("a/b.typ", "a") && is_within("a", "a") && !is_within("ab/c.typ", "a"));
        assert_eq!(moved("a/b.typ", "a", "x/y").as_deref(), Some("x/y/b.typ"));
        assert_eq!(moved("a", "a", "b").as_deref(), Some("b"));
        assert_eq!(moved("ab", "a", "b"), None);
    }

    #[test]
    fn a_document_is_named_by_its_extension() {
        assert!(is_document("a/b.typ") && !is_document("b.pdf") && !is_document("typ"));
        assert_eq!(document_title("b.typ"), "b");
        assert_eq!(document_title("b.pdf"), "b.pdf");
    }
}
