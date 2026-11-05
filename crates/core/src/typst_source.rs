//! Reading and writing Typst source through its parser: the one tree walker,
//! the rules for path arguments, and escaping for text written back.

use std::ops::Range;
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{LinkedNode, SyntaxKind};

/// Visits `node` and everything under it, each node given what its parent's
/// visit returned.
pub(crate) fn descend<'a, S: Copy>(node: &LinkedNode<'a>, state: S, visit: &mut impl FnMut(&LinkedNode<'a>, S) -> S) {
    let state = visit(node, state);
    for child in node.children() { descend(&child, state, visit); }
}

pub(crate) fn walk<'a>(node: &LinkedNode<'a>, visit: &mut impl FnMut(&LinkedNode<'a>)) {
    descend(node, (), &mut |node, ()| visit(node));
}

/// The name a call or set rule calls.
pub(crate) fn callee(call: &LinkedNode) -> Option<String> {
    let target = call.children().find(|child| child.kind() == SyntaxKind::Ident)?;
    Some(ast::Ident::from_untyped(target.get())?.get().to_string())
}

pub(crate) fn args_of<'a>(call: &LinkedNode<'a>) -> Option<LinkedNode<'a>> {
    call.children().find(|child| child.kind() == SyntaxKind::Args)
}

/// A string literal's value.
pub(crate) fn string(node: &LinkedNode) -> Option<String> {
    Some(ast::Str::from_untyped(node.get())?.get().to_string())
}

/// `node`'s value when it is the path of an `#include`.
pub(crate) fn include_literal(node: &LinkedNode) -> Option<String> {
    string(node).filter(|_| node.parent_kind() == Some(SyntaxKind::ModuleInclude))
}

/// The key `node` cites: `@key` or the label in `#cite(<key>)`.
pub(crate) fn cited_key(node: &LinkedNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Ref => ast::Ref::from_untyped(node.get()).map(|reference| reference.target().to_string()),
        SyntaxKind::Label if node.parent().is_some_and(|args| args.kind() == SyntaxKind::Args && args.parent().and_then(callee).as_deref() == Some("cite")) => {
            ast::Label::from_untyped(node.get()).map(|label| label.get().to_string())
        }
        _ => None,
    }
}

/// Whether text at this leaf is ordinary document text: not raw text, a
/// comment, or a string, which parses as an error while still being typed.
pub(crate) fn in_plain_text(leaf: &LinkedNode) -> bool {
    !leaf.kind().is_error() && !matches!(leaf.kind(), SyntaxKind::Str | SyntaxKind::LineComment | SyntaxKind::BlockComment | SyntaxKind::RawDelim) && leaf.parent_kind() != Some(SyntaxKind::Raw)
}

/// `node`'s range with the `#` that introduces it.
pub(crate) fn with_hash(text: &str, node: &LinkedNode) -> Range<usize> {
    let range = node.range();
    range.start - usize::from(text[..range.start].ends_with('#'))..range.end
}

/// The dir of a vault path, as path segments.
pub(crate) fn dir_of(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

/// The vault path a path literal in `dir` points at, unless it leaves the
/// vault. `@preview/…` is a package, and a URL has no place in the vault.
pub(crate) fn vault_path(dir: &[&str], literal: &str) -> Option<String> {
    if literal.starts_with('@') || literal.contains("://") { return None; }
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
pub(crate) fn relative(dir: &[&str], target: &str) -> String {
    let parts: Vec<&str> = target.split('/').collect();
    let shared = dir.iter().zip(&parts).take_while(|(a, b)| a == b).count();
    let mut out = vec![".."; dir.len() - shared];
    out.extend(&parts[shared..]);
    out.join("/")
}

/// `text` as the inside of a string literal.
pub(crate) fn escape_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `text` as a string literal.
pub(crate) fn quoted(text: &str) -> String {
    format!("\"{}\"", escape_string(text))
}

/// `text` as Typst markup that renders literally.
pub(crate) fn escape_markup(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let special = match c {
            '\\' | '#' | '$' | '*' | '_' | '`' | '@' | '<' | '>' | '[' | ']' | '~' => true,
            // Comments, and a term list at the start.
            '/' => i == 0 || matches!(next, Some('/' | '*')),
            // Lists, dashes, soft hyphens, and minus signs.
            '-' => i == 0 || i > 0 && chars[i - 1] == '-' || next.is_some_and(|n| matches!(n, '-' | '?') || n.is_ascii_digit()),
            '+' | '=' => i == 0,
            // A numbered list at the start, and the ellipsis shorthand.
            '.' => (i > 0 && chars[..i].iter().all(char::is_ascii_digit)) || chars[i..].starts_with(&['.', '.', '.']),
            _ => false,
        };
        if special { out.push('\\'); }
        out.push(c);
    }
    out
}
