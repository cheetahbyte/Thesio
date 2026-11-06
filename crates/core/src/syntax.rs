//! Syntax highlighting from Typst's own parser.

use crate::typst_source::descend;
use std::ops::Range;
use typst::syntax::{LinkedNode, Source, Tag, highlight};

pub use typst::syntax::Tag as SyntaxTag;

/// Highlighted leaves of `text`, sorted and not overlapping.
pub fn highlights(text: &str) -> Vec<(Range<usize>, Tag)> {
    let source = Source::detached(text);
    let mut out = vec![];
    descend(&LinkedNode::new(source.root()), None, &mut |node, inherited| {
        let tag = highlight(node).or(inherited);
        if node.children().next().is_none() && let Some(tag) = tag && !node.range().is_empty() { out.push((node.range(), tag)); }
        tag
    });
    out
}
