//! Vault paths: relative to the vault with `/` between folders.

use std::path::Path;

pub const EXTENSION: &str = "typ";
/// The citation library, at the vault root.
pub const LIBRARY_FILE: &str = "library.yaml";

pub fn parent_of(path: &str) -> Option<&str> { path.rsplit_once('/').map(|(parent, _)| parent) }
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

pub fn is_document(name: &str) -> bool { name.rsplit_once('.').is_some_and(|(_, extension)| extension == EXTENSION) }

/// Visits the vault path of every visible file under `root`; dot-folders such as `.thesio` are hidden.
pub(crate) fn visit_files(root: &Path, visit: &mut dyn FnMut(String)) {
    fn walk(dir: &Path, parent: Option<&str>, visit: &mut dyn FnMut(String)) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut items: Vec<_> = read.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for item in items {
            let name = item.file_name().to_string_lossy().into_owned();
            let Ok(info) = item.metadata() else { continue };
            if name.starts_with('.') { continue; }
            let id = join(parent, &name);
            if info.is_dir() { walk(&item.path(), Some(&id), visit); } else { visit(id); }
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
    }

    #[test]
    fn a_move_carries_a_folder_and_what_it_holds_and_nothing_else() {
        assert_eq!(moved("a/b.typ", "a", "x/y").as_deref(), Some("x/y/b.typ"));
        assert_eq!(moved("a", "a", "b").as_deref(), Some("b"));
        assert_eq!(moved("ab", "a", "b"), None);
    }

    #[test]
    fn a_document_is_named_by_its_extension() {
        assert!(is_document("a/b.typ") && !is_document("b.pdf") && !is_document("typ"));
    }
}
