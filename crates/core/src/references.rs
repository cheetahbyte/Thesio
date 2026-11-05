//! Reads and rewrites what Typst source points at: citations, note links, and
//! the file paths that change when a file moves. Only path arguments are
//! touched, found with Typst's parser, so matching text in prose, comments, or
//! ordinary strings is left alone.

use crate::paths::{dir_parts, is_document};
use crate::project::candidates;
use crate::typst_source::{callee, cited_key, escape_markup, in_plain_text, quoted, relative, string, vault_path, walk, with_hash};
use std::ops::Range;
use typst::syntax::{LinkedNode, Side, Source, SyntaxKind, is_id_continue};

/// Functions whose first arguments are vault paths.
const PATH_FUNCTIONS: &[&str] = &["image", "bibliography", "read", "csv", "json", "yaml", "toml", "xml", "cbor", "plugin"];

/// Whether a string node is a path argument.
fn is_path(node: &LinkedNode) -> bool {
    let Some(parent) = node.parent() else { return false };
    match parent.kind() {
        SyntaxKind::ModuleInclude | SyntaxKind::ModuleImport => true,
        SyntaxKind::Args => in_path_call(parent),
        // `bibliography(("a.bib", "b.bib"))`
        SyntaxKind::Array => parent.parent().is_some_and(|args| args.kind() == SyntaxKind::Args && in_path_call(args)),
        _ => false,
    }
}

fn in_path_call(args: &LinkedNode) -> bool {
    args.parent().is_some_and(|call| matches!(call.kind(), SyntaxKind::FuncCall | SyntaxKind::SetRule)
        && callee(call).is_some_and(|name| PATH_FUNCTIONS.contains(&name.as_str())))
}

/// The target of `#link("note.typ")`, which Thesio treats as a path: the
/// first string of a `link` call, naming a `.typ` file and not a web address.
fn note_literal(node: &LinkedNode) -> Option<String> {
    let args = node.parent().filter(|parent| parent.kind() == SyntaxKind::Args)?;
    args.parent().filter(|call| call.kind() == SyntaxKind::FuncCall && callee(call).as_deref() == Some("link"))?;
    (args.children().find(|child| child.kind() == SyntaxKind::Str)?.offset() == node.offset()).then_some(())?;
    string(node).filter(|literal| is_document(literal))
}

/// A `#link` to another file in the vault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteLink {
    /// As written in the source.
    pub literal: String,
    /// The vault path it points at.
    pub target: String,
    /// The whole call.
    pub range: Range<usize>,
}

/// The note links in `text`, a document at vault path `file`.
pub fn note_links(text: &str, file: &str) -> Vec<NoteLink> {
    let source = Source::detached(text);
    let dir = dir_parts(file);
    let mut links = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| {
        if node.kind() != SyntaxKind::Str { return; }
        let Some(literal) = note_literal(node) else { return };
        let Some(target) = vault_path(&dir, &literal) else { return };
        let Some(call) = node.parent().and_then(|args| args.parent()) else { return };
        links.push(NoteLink { literal, target, range: with_hash(text, call) });
    });
    links
}

/// The `#link` to write in `file` for a note: `#link("a/b.typ")[Title]`.
pub fn note_link(file: &str, target: &str, title: &str) -> String {
    format!("#link({})[{}]", quoted(&relative(&dir_parts(file), target)), escape_markup(title))
}

/// The `[[query` being written at `caret`, brackets included. Strings, raw
/// text, and comments hold no links.
pub fn note_link_at(text: &str, caret: usize) -> Option<Range<usize>> {
    let head = text.get(..caret)?;
    let line = head.rfind('\n').map_or(0, |i| i + 1);
    let at = line + head[line..].rfind("[[")?;
    if head[at + 2..].contains(']') { return None; }
    let source = Source::detached(text);
    let root = LinkedNode::new(source.root());
    let leaf = root.leaf_at(at + 1, Side::After)?;
    in_plain_text(&leaf).then(|| at..if text[caret..].starts_with("]]") { caret + 2 } else { caret })
}

/// `text` with its paths updated, or `None` when nothing changed. The file
/// itself moved from `old_file` to `new_file` (they are equal when it stayed),
/// and `moved` maps an old vault path to its new one. `main` and `exists` say
/// what the compiler saw before the move, so `/x` means the file it would find.
pub fn rewrite_paths(text: &str, old_file: &str, new_file: &str, main: &str, exists: impl Fn(&str) -> bool, moved: impl Fn(&str) -> Option<String>) -> Option<String> {
    let source = Source::detached(text);
    let mut literals = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| {
        if node.kind() == SyntaxKind::Str && (is_path(node) || note_literal(node).is_some()) && let Some(value) = string(node) {
            literals.push((node.range(), value));
        }
    });
    let (old_dir, new_dir) = (dir_parts(old_file), dir_parts(new_file));
    let new_main = moved(main).unwrap_or_else(|| main.to_string());
    let mut out = text.to_string();
    let mut changed = false;
    for (range, literal) in literals.into_iter().rev() {
        let Some(wanted) = vault_path(&old_dir, &literal) else { continue };
        let rooted = literal.starts_with('/');
        let target = if rooted { candidates(main, &wanted).find(|path| exists(path)).unwrap_or_else(|| wanted.clone()) } else { wanted.clone() };
        let renamed = moved(&target);
        let new_target = renamed.clone().unwrap_or_else(|| target.clone());
        // The folder `/` stood for: it still does while the main file and the target stay inside it.
        let project = target.strip_suffix(wanted.as_str()).unwrap_or_default();
        let in_project = new_main.starts_with(project) && new_target.starts_with(project);
        if renamed.is_none() && (if rooted { in_project } else { old_dir == new_dir }) { continue; }
        let updated = match (rooted, in_project) {
            (true, true) => format!("/{}", &new_target[project.len()..]),
            (true, false) => format!("/{new_target}"),
            (false, _) => relative(&new_dir, &new_target),
        };
        if updated != literal {
            out.replace_range(range, &quoted(&updated));
            changed = true;
        }
    }
    changed.then_some(out)
}

/// Citation keys in `text`: `@key` and `#cite(<key>)`.
pub fn cited_keys(text: &str) -> Vec<String> {
    let source = Source::detached(text);
    let mut keys = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| keys.extend(cited_key(node)));
    keys
}

/// Where `key` is cited in `text`: the byte after each `@` or `<`.
pub fn citations_of(text: &str, key: &str) -> Vec<usize> {
    let source = Source::detached(text);
    let mut starts = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| {
        if cited_key(node).as_deref() == Some(key) { starts.push(node.offset() + 1); }
    });
    starts
}

/// `text` with every citation of `old` changed to `new`, or `None` without any.
/// In a document with a label `old`, `@old` means the label, so `labelled`
/// leaves it and changes only `#cite(<old>)`.
pub fn rename_citation(text: &str, old: &str, new: &str, labelled: bool) -> Option<String> {
    let source = Source::detached(text);
    let mut starts = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| {
        if cited_key(node).as_deref() == Some(old) && !(labelled && node.kind() == SyntaxKind::Ref) { starts.push(node.offset() + 1); }
    });
    if starts.is_empty() { return None; }
    let mut out = text.to_string();
    for start in starts.into_iter().rev() { out.replace_range(start..start + old.len(), new); }
    Some(out)
}

/// The `@key` being written at `caret`, `@` included. An `@` after a word is an
/// email address, and raw text, strings, comments, and math hold no citations.
pub fn citation_at(text: &str, caret: usize) -> Option<Range<usize>> {
    let in_key = |c: char| is_id_continue(c) || matches!(c, '.' | ':');
    let (head, tail) = text.split_at_checked(caret)?;
    let at = head.trim_end_matches(in_key).strip_suffix('@')?.len();
    if text[..at].chars().next_back().is_some_and(char::is_alphanumeric) { return None; }
    let end = caret + tail.find(|c| !in_key(c)).unwrap_or(tail.len());
    let source = Source::detached(text);
    let root = LinkedNode::new(source.root());
    let leaf = root.leaf_at(at + 1, Side::Before)?;
    // Typst only lexes `@` as a reference once a key follows it.
    (matches!(leaf.kind(), SyntaxKind::RefMarker | SyntaxKind::Text) && in_plain_text(&leaf)).then(|| at..at + text[at..end].trim_end_matches(['.', ':']).len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINKS: &str = "#link(\"../b.typ\")[B] #link(\"https://x.org/a.typ\")[web] #link(\"c.pdf\")[pdf]\n#link(\"d.typ\", \"e.typ\")\n`#link(\"raw.typ\")`\n";

    #[test]
    fn only_links_to_vault_documents_are_note_links() {
        let links = note_links(LINKS, "notes/a.typ");
        assert_eq!(links.iter().map(|link| link.target.as_str()).collect::<Vec<_>>(), ["b.typ", "notes/d.typ"]);
        assert_eq!(&LINKS[links[0].range.clone()], "#link(\"../b.typ\")[B]");
    }

    #[test]
    fn a_note_link_follows_its_target() {
        let moved = rewrite_paths(LINKS, "notes/a.typ", "notes/a.typ", "notes/a.typ", |_| false, |path| (path == "b.typ").then(|| "archive/b.typ".to_string())).unwrap();
        assert!(moved.starts_with("#link(\"../archive/b.typ\")[B] #link(\"https://x.org/a.typ\")"), "{moved}");
    }

    #[test]
    fn a_note_link_is_written_relative_with_its_title_escaped() {
        assert_eq!(note_link("notes/a.typ", "ch/b.typ", "Why [this] #1"), "#link(\"../ch/b.typ\")[Why \\[this\\] \\#1]");
    }

    #[test]
    fn a_note_link_is_being_written_up_to_its_closing_brackets() {
        assert_eq!(note_link_at("see [[fla", 9), Some(4..9));
        assert_eq!(note_link_at("see [[fla]] x", 9), Some(4..11));
        assert_eq!(note_link_at("see [[a]] b", 11), None);
    }

    #[test]
    fn raw_text_comments_and_strings_hold_no_note_links() {
        assert_eq!(note_link_at("`[[a", 4), None);
        assert_eq!(note_link_at("// [[a", 6), None);
        assert_eq!(note_link_at("#let a = \"[[a", 13), None);
    }

    #[test]
    fn a_citation_is_being_written_only_in_document_text() {
        assert_eq!(citation_at("see @fla", 8), Some(4..8));
        assert_eq!(citation_at("a@fla", 5), None);
        assert_eq!(citation_at("`@fla`", 5), None);
        assert_eq!(citation_at("// @fla", 7), None);
    }

    #[test]
    fn renaming_a_key_changes_only_its_citations() {
        let text = "See @old, @older[p. 2], @old[p. 1] and #cite(<old>). <old>\n`@old` // @old\n";
        assert_eq!(rename_citation(text, "old", "new", false).unwrap(), "See @new, @older[p. 2], @new[p. 1] and #cite(<new>). <old>\n`@old` // @old\n");
        assert_eq!(rename_citation("nothing here", "old", "new", false), None);
        assert_eq!(citations_of(text, "old"), [5, 25, 46]);
    }

    #[test]
    fn in_a_labelled_document_at_means_the_label() {
        let text = "See @old and #cite(<old>).\n= One <old>\n";
        assert_eq!(rename_citation(text, "old", "new", true).unwrap(), "See @old and #cite(<new>).\n= One <old>\n");
        assert_eq!(rename_citation("See @old.\n", "old", "new", true), None);
    }
}
