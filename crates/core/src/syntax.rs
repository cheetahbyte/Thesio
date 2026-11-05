//! Syntax highlighting from Typst's own parser.

use std::ops::Range;
use typst::syntax::{LinkedNode, Source, Tag, highlight};

pub use typst::syntax::Tag as SyntaxTag;

/// Highlighted leaves of `text`, sorted and not overlapping.
pub fn highlights(text: &str) -> Vec<(Range<usize>, Tag)> {
    let source = Source::detached(text);
    let mut out = vec![];
    collect(&LinkedNode::new(source.root()), None, &mut out);
    out
}

fn collect(node: &LinkedNode, inherited: Option<Tag>, out: &mut Vec<(Range<usize>, Tag)>) {
    let tag = highlight(node).or(inherited);
    let mut leaf = true;
    for child in node.children() {
        leaf = false;
        collect(&child, tag, out);
    }
    if leaf && let Some(tag) = tag && !node.range().is_empty() {
        out.push((node.range(), tag));
    }
}
