//! Rewrites the file paths in Typst source after a file moves. Only path
//! arguments are touched, found with Typst's parser, so matching text in prose,
//! comments, or ordinary strings is left alone.

use std::ops::Range;
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{LinkedNode, Source, SyntaxKind};

/// Functions whose first arguments are vault paths.
const PATH_FUNCTIONS: &[&str] = &["image", "bibliography", "read", "csv", "json", "yaml", "toml", "xml", "cbor", "plugin"];

/// The dir of a vault path, as path segments.
fn dir_of(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

/// The vault path a literal in `dir` points at, unless it leaves the vault.
fn resolve(dir: &[&str], literal: &str) -> Option<String> {
    let (mut parts, rest) = match literal.strip_prefix('/') {
        Some(rest) => (vec![], rest),
        None => (dir.to_vec(), literal),
    };
    for segment in rest.split('/') {
        match segment {
            "" | "." => {}
            ".." => { parts.pop()?; }
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `target` as a path relative to `dir`.
fn relative(dir: &[&str], target: &str) -> String {
    let parts: Vec<&str> = target.split('/').collect();
    let shared = dir.iter().zip(&parts).take_while(|(a, b)| a == b).count();
    let mut out = vec![".."; dir.len() - shared];
    out.extend(&parts[shared..]);
    out.join("/")
}

fn callee(call: &LinkedNode) -> Option<String> {
    let target = call.children().find(|child| child.kind() == SyntaxKind::Ident)?;
    Some(ast::Ident::from_untyped(target.get())?.get().to_string())
}

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

fn collect(node: &LinkedNode, out: &mut Vec<(Range<usize>, String)>) {
    if node.kind() == SyntaxKind::Str && is_path(node) && let Some(value) = ast::Str::from_untyped(node.get()) {
        out.push((node.range(), value.get().to_string()));
    }
    for child in node.children() {
        collect(&child, out);
    }
}

/// `text` with its paths updated, or `None` when nothing changed. The file
/// itself moved from `old_file` to `new_file` (they are equal when it stayed),
/// and `moved` maps an old vault path to its new one.
pub fn rewrite_paths(text: &str, old_file: &str, new_file: &str, moved: impl Fn(&str) -> Option<String>) -> Option<String> {
    let source = Source::detached(text);
    let mut literals = vec![];
    collect(&LinkedNode::new(source.root()), &mut literals);
    let (old_dir, new_dir) = (dir_of(old_file), dir_of(new_file));
    let mut out = text.to_string();
    let mut changed = false;
    for (range, literal) in literals.into_iter().rev() {
        // `@preview/…` is a package, and a URL has no place in the vault.
        if literal.starts_with('@') || literal.contains("://") { continue; }
        let Some(target) = resolve(&old_dir, &literal) else { continue };
        let renamed = moved(&target);
        if renamed.is_none() && old_dir == new_dir { continue; }
        let target = renamed.unwrap_or(target);
        let updated = if literal.starts_with('/') { format!("/{target}") } else { relative(&new_dir, &target) };
        if updated != literal {
            out.replace_range(range, &format!("\"{}\"", updated.replace('\\', "\\\\").replace('"', "\\\"")));
            changed = true;
        }
    }
    changed.then_some(out)
}

/// A document's `#bibliography` call.
pub struct BibliographyCall {
    /// The vault path of its first source, when that is a string path.
    pub path: Option<String>,
    pub range: Range<usize>,
}

fn walk(node: &LinkedNode, visit: &mut impl FnMut(&LinkedNode)) {
    visit(node);
    for child in node.children() {
        walk(&child, visit);
    }
}

/// The first `#bibliography` call in `text`, a document at vault path `file`.
pub fn bibliography_call(text: &str, file: &str) -> Option<BibliographyCall> {
    let source = Source::detached(text);
    let mut found = None;
    walk(&LinkedNode::new(source.root()), &mut |node| {
        if found.is_none() && node.kind() == SyntaxKind::FuncCall && callee(node).as_deref() == Some("bibliography") {
            let path = node.children().find(|c| c.kind() == SyntaxKind::Args)
                .and_then(|args| args.children().find(|c| c.kind() == SyntaxKind::Str))
                .and_then(|s| ast::Str::from_untyped(s.get()).map(|v| v.get().to_string()))
                .and_then(|literal| resolve(&dir_of(file), &literal));
            found = Some(BibliographyCall { path, range: node.range() });
        }
    });
    found
}

/// Citation keys in `text`: `@key` and `#cite(<key>)`.
pub fn cited_keys(text: &str) -> Vec<String> {
    let source = Source::detached(text);
    let mut keys = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| match node.kind() {
        SyntaxKind::Ref => {
            if let Some(reference) = ast::Ref::from_untyped(node.get()) { keys.push(reference.target().to_string()); }
        }
        SyntaxKind::Label if node.parent().is_some_and(|args| args.kind() == SyntaxKind::Args && args.parent().and_then(callee).as_deref() == Some("cite")) => {
            if let Some(label) = ast::Label::from_untyped(node.get()) { keys.push(label.get().to_string()); }
        }
        _ => {}
    });
    keys
}

/// Vault paths of the files `text`, at vault path `file`, includes directly.
pub fn included_files(text: &str, file: &str) -> Vec<String> {
    let source = Source::detached(text);
    let mut files = vec![];
    let dir = dir_of(file);
    walk(&LinkedNode::new(source.root()), &mut |node| {
        if node.kind() == SyntaxKind::Str && node.parent_kind() == Some(SyntaxKind::ModuleInclude)
            && let Some(value) = ast::Str::from_untyped(node.get())
            && let Some(path) = resolve(&dir, &value.get()) {
                files.push(path);
            }
    });
    files
}

/// `target` as a path relative to the document at vault path `file`.
pub fn relative_to(file: &str, target: &str) -> String {
    relative(&dir_of(file), target)
}
