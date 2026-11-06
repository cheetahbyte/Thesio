//! Syntax highlighting from Typst's own parser.

use crate::typst_source::descend;
use std::ops::Range;
use typst::syntax::{LinkedNode, Source, Tag, highlight};

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

/// The kebab-case name of a tag, as the ABI spells it.
pub fn tag_name(tag: Tag) -> &'static str {
    match tag {
        Tag::Comment => "comment",
        Tag::Punctuation => "punctuation",
        Tag::Escape => "escape",
        Tag::Strong => "strong",
        Tag::Emph => "emph",
        Tag::Link => "link",
        Tag::Raw => "raw",
        Tag::Label => "label",
        Tag::Ref => "ref",
        Tag::Heading => "heading",
        Tag::ListMarker => "list-marker",
        Tag::ListTerm => "list-term",
        Tag::MathDelimiter => "math-delimiter",
        Tag::MathOperator => "math-operator",
        Tag::MathGroupingParens => "math-grouping-parens",
        Tag::Keyword => "keyword",
        Tag::Operator => "operator",
        Tag::Number => "number",
        Tag::String => "string",
        Tag::Function => "function",
        Tag::Interpolated => "interpolated",
        Tag::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_take_the_tag_of_what_holds_them() {
        let text = "= Head\n#let x = 1 // note\n";
        let found: Vec<(&str, &str)> = highlights(text).into_iter().map(|(range, tag)| (&text[range], tag_name(tag))).collect();
        assert_eq!(found[0], ("=", "heading"));
        assert!(found.contains(&("Head", "heading")), "{found:?}");
        assert!(found.contains(&("let", "keyword")) && found.contains(&("1", "number")) && found.contains(&("// note", "comment")), "{found:?}");
    }
}
