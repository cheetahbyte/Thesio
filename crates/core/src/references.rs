//! Reads and rewrites what Typst source points at: citations, headings, note
//! links, and the file paths that change when a file moves. Only path
//! arguments are touched, found with Typst's parser, so matching text in prose,
//! comments, or ordinary strings is left alone.

use std::ops::Range;
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{LinkedNode, Side, Source, SyntaxKind, is_id_continue};

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

/// The target of `#link("note.typ")`, which Thesio treats as a path: the
/// first string of a `link` call, naming a `.typ` file and not a web address.
fn note_literal(node: &LinkedNode) -> Option<String> {
    let args = node.parent().filter(|parent| parent.kind() == SyntaxKind::Args)?;
    args.parent().filter(|call| call.kind() == SyntaxKind::FuncCall && callee(call).as_deref() == Some("link"))?;
    (args.children().find(|child| child.kind() == SyntaxKind::Str)?.offset() == node.offset()).then_some(())?;
    let literal = ast::Str::from_untyped(node.get())?.get().to_string();
    (literal.ends_with(".typ") && !literal.contains("://")).then_some(literal)
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
    let dir = dir_of(file);
    let mut links = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| {
        if node.kind() != SyntaxKind::Str { return; }
        let Some(literal) = note_literal(node) else { return };
        let Some(target) = resolve(&dir, &literal) else { return };
        let Some(call) = node.parent().and_then(|args| args.parent()) else { return };
        let range = call.range();
        let start = range.start - usize::from(text[..range.start].ends_with('#'));
        links.push(NoteLink { literal, target, range: start..range.end });
    });
    links
}

/// The `#link` to write in `file` for a note: `#link("a/b.typ")[Title]`.
pub fn note_link(file: &str, target: &str, title: &str) -> String {
    let path = relative_to(file, target).replace('\\', "\\\\").replace('"', "\\\"");
    let mut label = String::new();
    for c in title.chars() {
        if matches!(c, '\\' | '[' | ']' | '#' | '*' | '_' | '`' | '$' | '@' | '<' | '~' | '/') { label.push('\\'); }
        label.push(c);
    }
    format!("#link(\"{path}\")[{label}]")
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
    // Raw text or a string still being typed parses as an error.
    let plain = !leaf.kind().is_error() && !matches!(leaf.kind(), SyntaxKind::Str | SyntaxKind::LineComment | SyntaxKind::BlockComment | SyntaxKind::RawDelim) && leaf.parent_kind() != Some(SyntaxKind::Raw);
    plain.then(|| at..if text[caret..].starts_with("]]") { caret + 2 } else { caret })
}

fn collect(node: &LinkedNode, out: &mut Vec<(Range<usize>, String)>) {
    if node.kind() == SyntaxKind::Str && (is_path(node) || note_literal(node).is_some()) && let Some(value) = ast::Str::from_untyped(node.get()) {
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

/// A heading in a document's outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    /// The file it is written in, and where.
    pub path: String,
    pub offset: usize,
    /// 1 for `=`.
    pub level: usize,
    pub title: String,
}

/// The headings of `root` and the files it includes, in reading order. `read`
/// gives a file's text. A file that includes itself is followed once.
pub fn outline(root: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Vec<Heading> {
    fn collect(file: &str, read: &mut dyn FnMut(&str) -> Option<String>, open: &mut Vec<String>, out: &mut Vec<Heading>) {
        if open.iter().any(|path| path == file) { return; }
        let Some(text) = read(file) else { return };
        let source = Source::detached(text);
        let dir = dir_of(file);
        let (mut headings, mut includes) = (vec![], vec![]);
        walk(&LinkedNode::new(source.root()), &mut |node| match node.kind() {
            SyntaxKind::Heading => {
                let Some(heading) = ast::Heading::from_untyped(node.get()) else { return };
                let body = heading.body().to_untyped().full_text();
                let title = body.split(" <").next().unwrap_or_default().trim().to_string();
                if !title.is_empty() { headings.push(Heading { path: file.to_string(), offset: node.offset(), level: heading.depth().get(), title }); }
            }
            SyntaxKind::Str if node.parent_kind() == Some(SyntaxKind::ModuleInclude) => {
                if let Some(path) = ast::Str::from_untyped(node.get()).and_then(|value| resolve(&dir, &value.get())) { includes.push((node.offset(), path)); }
            }
            _ => {}
        });
        open.push(file.to_string());
        let mut includes = includes.into_iter().peekable();
        for heading in headings {
            while let Some((_, path)) = includes.next_if(|(offset, _)| *offset < heading.offset) { collect(&path, read, open, out); }
            out.push(heading);
        }
        for (_, path) in includes { collect(&path, read, open, out); }
        open.pop();
    }
    let mut out = vec![];
    collect(root, read, &mut vec![], &mut out);
    out
}

/// Where `key` is cited in `text`: the byte after each `@` or `<`.
pub fn citations_of(text: &str, key: &str) -> Vec<usize> {
    let source = Source::detached(text);
    let mut starts = vec![];
    walk(&LinkedNode::new(source.root()), &mut |node| {
        let target = match node.kind() {
            SyntaxKind::Ref => ast::Ref::from_untyped(node.get()).map(|reference| reference.target().to_string()),
            SyntaxKind::Label if node.parent().is_some_and(|args| args.kind() == SyntaxKind::Args && args.parent().and_then(callee).as_deref() == Some("cite")) => {
                ast::Label::from_untyped(node.get()).map(|label| label.get().to_string())
            }
            _ => None,
        };
        // Past the `@` or `<`.
        if target.as_deref() == Some(key) { starts.push(node.offset() + 1); }
    });
    starts
}

/// `text` with every citation of `old` changed to `new`, or `None` without any.
pub fn rename_citation(text: &str, old: &str, new: &str) -> Option<String> {
    let starts = citations_of(text, old);
    if starts.is_empty() { return None; }
    let mut out = text.to_string();
    for start in starts.into_iter().rev() { out.replace_range(start..start + old.len(), new); }
    Some(out)
}

/// The arguments of the call starting at `start`.
fn call_args<'a>(root: &'a LinkedNode<'a>, start: usize) -> Option<LinkedNode<'a>> {
    let leaf = root.leaf_at(start + 1, Side::After)?;
    let call = std::iter::successors(Some(leaf), |node| node.parent().cloned()).find(|node| node.kind() == SyntaxKind::FuncCall)?;
    call.children().find(|child| child.kind() == SyntaxKind::Args)
}

/// The string given for a call's `style` argument.
fn style_value<'a>(args: &LinkedNode<'a>) -> Option<LinkedNode<'a>> {
    let named = args.children().filter(|child| child.kind() == SyntaxKind::Named)
        .find(|child| child.children().next().is_some_and(|name| name.get().full_text() == "style"))?;
    named.children().find(|child| child.kind() == SyntaxKind::Str)
}

/// The `style` a document's `#bibliography` asks for.
pub fn bibliography_style(text: &str, file: &str) -> Option<String> {
    let call = bibliography_call(text, file)?;
    let source = Source::detached(text);
    let root = LinkedNode::new(source.root());
    let value = style_value(&call_args(&root, call.range.start)?)?;
    ast::Str::from_untyped(value.get()).map(|value| value.get().to_string())
}

/// `text` with its `#bibliography` using `style`.
pub fn set_bibliography_style(text: &str, file: &str, style: &str) -> Option<String> {
    let call = bibliography_call(text, file)?;
    let source = Source::detached(text);
    let root = LinkedNode::new(source.root());
    let args = call_args(&root, call.range.start)?;
    let literal = format!("\"{style}\"");
    if let Some(value) = style_value(&args) {
        return Some(format!("{}{literal}{}", &text[..value.range().start], &text[value.range().end..]));
    }
    // Before the closing parenthesis, which a trailing comma may precede.
    let close = args.range().end - 1;
    let head = text[..close].trim_end();
    let separator = if head.ends_with('(') { "" } else if head.ends_with(',') { " " } else { ", " };
    Some(format!("{head}{separator}style: {literal}{}", &text[close..]))
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
    let markup = leaf.kind() == SyntaxKind::RefMarker || leaf.kind() == SyntaxKind::Text && leaf.parent_kind() != Some(SyntaxKind::Raw);
    markup.then(|| at..at + text[at..end].trim_end_matches(['.', ':']).len())
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
