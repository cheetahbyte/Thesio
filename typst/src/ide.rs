//! Questions about text being edited, answered by `typst_ide`: what can follow
//! the caret and what is under the pointer. The text is usually newer than the
//! last compile, so it stands in for its file in that compile's world. A
//! question may compile the whole document, so it belongs off the main thread.

use crate::compiler::{Compilation, with_large_stack};
use std::cell::RefCell;
use std::ops::Range;
use typst::syntax::ast;
use typst::syntax::{LinkedNode, Side, Source, SyntaxKind, is_id_continue};
use typst_ide::{CompletionKind, Tooltip};

/// A value's printed form can be any length, and a hover is only a glance.
const HOVER_CHARS: usize = 600;

/// Text to insert and the placeholders in it, in the order they are filled.
#[derive(Debug, PartialEq)]
pub struct Snippet {
    pub text: String,
    pub fields: Vec<Range<usize>>,
}

/// Reads `typst_ide`'s `${name}` and `${}`, which keep their name as text.
pub fn snippet(apply: &str) -> Snippet {
    let (mut text, mut fields, mut rest) = (String::new(), vec![], apply);
    while let Some((before, field)) = rest.split_once("${") {
        text.push_str(before);
        let Some((name, after)) = field.split_once('}') else {
            text.push_str("${");
            rest = field;
            continue;
        };
        fields.push(text.len()..text.len() + name.len());
        text.push_str(name);
        rest = after;
    }
    text.push_str(rest);
    Snippet { text, fields }
}

#[derive(Debug)]
pub struct Suggestion {
    pub title: String,
    pub kind: String,
    pub detail: Option<String>,
    /// Where the text it replaces starts.
    pub from: usize,
    /// Bytes after the caret it also replaces: the rest of a path in a string.
    pub tail: usize,
    pub insert: Snippet,
}

#[derive(Debug, PartialEq)]
pub struct Hover {
    pub text: String,
    /// `text` is Typst code rather than a sentence.
    pub code: bool,
}

fn kind_name(kind: &CompletionKind) -> String {
    match kind {
        CompletionKind::Syntax => "Snippet".into(),
        CompletionKind::Func => "Function".into(),
        CompletionKind::Type => "Type".into(),
        CompletionKind::Param => "Parameter".into(),
        CompletionKind::Constant => "Value".into(),
        CompletionKind::Path => "File".into(),
        CompletionKind::Package => "Package".into(),
        CompletionKind::Label => "Label".into(),
        CompletionKind::Font => "Font".into(),
        CompletionKind::Symbol(symbol) => format!("Symbol {symbol}"),
    }
}

/// `typst_ide` lists types first; what a document calls is what it looks for.
fn kind_rank(kind: &CompletionKind) -> u8 {
    match kind {
        CompletionKind::Param => 0,
        CompletionKind::Func => 1,
        _ => 2,
    }
}

fn error_count(source: &Source) -> usize {
    let mut errors = 0;
    crate::typst_source::walk(&LinkedNode::new(source.root()), &mut |node| errors += usize::from(node.kind().is_error()));
    errors
}

/// What has to follow `caret` for the parser to read an unfinished call or
/// string. It takes a `(` nobody closed for an error, and `typst_ide` finds no
/// call there to offer parameters for, while the editor closes nothing itself.
/// A string nobody closed runs to the next quote or the end of the file. One
/// that leaves the caret's line ends at the caret if that removes errors;
/// otherwise it is a real multi-line string.
fn closing_at(text: &str, caret: usize) -> &'static str {
    let source = Source::detached(text);
    let root = LinkedNode::new(source.root());
    let mut call = false;
    crate::typst_source::walk(&root, &mut |node| {
        call = call || node.kind().is_error() && node.offset() < caret && node.leaf_text() == "(";
    });
    let string = root.leaf_at(caret, Side::Before).is_some_and(|leaf| {
        let range = leaf.range();
        let ended = || {
            let closed = format!("{}\"{}{}", &text[..caret], if call { ")" } else { "" }, &text[caret..]);
            error_count(&Source::detached(closed)) < error_count(&source)
        };
        leaf.leaf_text().starts_with('"') && range.start < caret && caret <= range.end && (leaf.kind().is_error() || text[caret..range.end].contains('\n') && ended())
    });
    match (string, call) {
        (true, true) => "\")",
        (true, false) => "\"",
        (false, true) => ")",
        (false, false) => "",
    }
}

fn indent_at(text: &str, caret: usize) -> &str {
    let line = &text[text[..caret].rfind('\n').map_or(0, |newline| newline + 1)..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

impl Compilation {
    /// What can replace the text before `caret`, best match first. `typst_ide`
    /// offers everything that fits there and leaves matching what is typed to
    /// its caller. `explicit` says the user asked, which offers more.
    pub fn complete(&self, path: &str, text: &str, caret: usize, explicit: bool) -> Vec<Suggestion> {
        if !text.is_char_boundary(caret) { return vec![]; }
        let closing = closing_at(text, caret);
        let closed = format!("{}{closing}{}", &text[..caret], &text[caret..]);
        let Some((world, source)) = self.world.with_text(path, &closed) else { return vec![] };
        let answer = with_large_stack(|| typst_ide::autocomplete(&world, self.document.as_ref(), &source, caret, explicit));
        let Ok(Some((from, found))) = answer else { return vec![] };
        let from = from.min(word_start(text, caret));
        let typed = text[from..caret].to_lowercase();
        let (in_string, typed) = match typed.strip_prefix('"') {
            Some(inside) => (true, inside),
            None => (false, typed.as_str()),
        };
        let tail = if in_string { text[caret..].find(|c: char| c.is_whitespace() || matches!(c, '"' | ')' | ',')).unwrap_or(text.len() - caret) } else { 0 };
        let indent = format!("\n{}", indent_at(text, caret));
        let mut matching: Vec<_> = found.into_iter().filter_map(|completion| {
            let title = completion.label.trim_matches('"').to_string();
            let lower = title.to_lowercase();
            if !lower.contains(typed) || in_string && !completion.label.starts_with('"') { return None; }
            let mut apply = completion.apply.as_ref().unwrap_or(&completion.label).replace("\n", &indent);
            // It left off the closing quote it saw, which is not really there.
            if closing.starts_with('"') && apply.starts_with('"') && !apply.ends_with('"') { apply.push('"'); }
            let rank = (!lower.starts_with(typed), kind_rank(&completion.kind));
            let suggestion = Suggestion { kind: kind_name(&completion.kind), detail: completion.detail.map(Into::into), from, tail, insert: snippet(&apply), title };
            Some((rank, suggestion))
        }).collect();
        matching.sort_by_key(|(rank, _)| *rank);
        matching.into_iter().map(|(_, suggestion)| suggestion).collect()
    }

    /// What `typst_ide` says about the byte at `offset`.
    pub fn hover(&self, path: &str, text: &str, offset: usize) -> Option<Hover> {
        if !text.is_char_boundary(offset) { return None; }
        let (world, source) = self.world.with_text(path, text)?;
        let tooltip = with_large_stack(|| typst_ide::tooltip(&world, self.document.as_ref(), &source, offset, Side::After)).ok()??;
        let (text, code) = match tooltip {
            Tooltip::Text(text) => (text, false),
            Tooltip::Code(text) => (text, true),
        };
        let mut text = text.to_string();
        if let Some((cut, _)) = text.char_indices().nth(HOVER_CHARS) {
            text.truncate(cut);
            text.push('…');
        }
        Some(Hover { text, code })
    }
}

/// Where the identifier running up to `caret` starts.
pub fn word_start(text: &str, caret: usize) -> usize {
    let Some(head) = text.get(..caret) else { return caret };
    caret - head.chars().rev().take_while(|c| is_id_continue(*c)).map(char::len_utf8).sum::<usize>()
}

/// The identifier at `offset`, or else the character there.
pub fn word_around(text: &str, offset: usize) -> Range<usize> {
    let Some(tail) = text.get(offset..) else { return offset..offset };
    let run = tail.chars().take_while(|c| is_id_continue(*c)).map(char::len_utf8).sum::<usize>();
    let end = offset + if run > 0 { run } else { tail.chars().next().map_or(0, char::len_utf8) };
    word_start(text, offset)..end
}

/// Says whether text at a byte is code just begun. Keeps the parsed text, since
/// a dot ends most sentences and parsing a long document again on that keystroke
/// would show.
#[derive(Default)]
pub struct CodeStart(RefCell<Option<Source>>);

impl CodeStart {
    /// After a `#`, or after the dot of a field access. A dot in prose does not
    /// count, nor anything in a comment or string.
    pub fn opens(&self, text: &str, start: usize) -> bool {
        if !text.get(..start).is_some_and(|head| head.ends_with(['#', '.'])) { return false; }
        let mut parsed = self.0.borrow_mut();
        let source = parsed.get_or_insert_with(|| Source::detached(text));
        if source.text() != text { source.replace(text); }
        let root = LinkedNode::new(source.root());
        let Some(leaf) = root.leaf_at(start, Side::Before) else { return false };
        match leaf.kind() {
            SyntaxKind::Hash | SyntaxKind::Dot => true,
            // Markup has no dot of its own until a field name follows it.
            SyntaxKind::Text | SyntaxKind::MathText if leaf.leaf_text() == "." => leaf.prev_sibling().is_some_and(|before| {
                before.range().end == leaf.range().start && before.is::<ast::Expr>()
                    && (before.parent_kind() != Some(SyntaxKind::Markup) || before.prev_sibling_kind() == Some(SyntaxKind::Hash))
            }),
            _ => false,
        }
    }
}

/// Whether `typed` can still be what an open popup is completing, which may be
/// a path inside a string.
pub fn continues_completion(typed: &str) -> bool {
    typed.chars().all(|c| is_id_continue(c) || matches!(c, '.' | '/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use std::path::PathBuf;

    fn vault(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("thesio-ide-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    fn compiled(name: &str, files: &[(&str, &str)]) -> Compilation {
        Compiler::new(vault(name, files)).compile(files[0].0).unwrap()
    }

    /// Completes at `|` in `text`, as the unsaved text of `main.typ`.
    fn complete(compilation: &Compilation, text: &str, explicit: bool) -> Vec<Suggestion> {
        let caret = text.find('|').unwrap();
        compilation.complete("main.typ", &text.replace('|', ""), caret, explicit)
    }

    fn titles(suggestions: &[Suggestion]) -> Vec<&str> {
        suggestions.iter().map(|suggestion| suggestion.title.as_str()).collect()
    }

    fn one(field: Range<usize>) -> Vec<Range<usize>> { std::iter::once(field).collect() }

    #[test]
    fn a_snippet_keeps_each_placeholder_as_a_field_in_order() {
        let read = snippet("let ${name} = ${value}");
        assert_eq!(read.text, "let name = value");
        assert_eq!(read.fields.iter().map(|field| &read.text[field.clone()]).collect::<Vec<_>>(), ["name", "value"]);
    }

    #[test]
    fn an_empty_placeholder_is_a_caret() {
        assert_eq!(snippet("image(\"${}\")"), Snippet { text: "image(\"\")".into(), fields: one(7..7) });
        assert_eq!(snippet("text()${}"), Snippet { text: "text()".into(), fields: one(6..6) });
    }

    #[test]
    fn text_with_no_closing_brace_stays_text() {
        assert_eq!(snippet("a ${b"), Snippet { text: "a ${b".into(), fields: vec![] });
    }

    #[test]
    fn what_is_typed_narrows_a_function_name() {
        let compilation = compiled("functions", &[("main.typ", "Text.\n")]);
        let found = complete(&compilation, "#headi|", false);
        assert_eq!(titles(&found)[0], "heading");
        assert!(found.iter().all(|suggestion| suggestion.title.contains("headi")));
        assert_eq!(found[0].from, 1);
        assert_eq!(found[0].kind, "Function");
    }

    #[test]
    fn a_name_that_starts_with_the_text_comes_first() {
        let compilation = compiled("rank", &[("main.typ", "Text.\n")]);
        let found = complete(&compilation, "#ima|", false);
        assert_eq!(titles(&found).first(), Some(&"image"));
    }

    #[test]
    fn a_variable_written_since_the_last_compile_is_offered() {
        let compilation = compiled("unsaved", &[("main.typ", "Text.\n")]);
        let found = complete(&compilation, "#let unsaved-name = 1\n#unsa|", false);
        assert_eq!(titles(&found), ["unsaved-name"]);
    }

    #[test]
    fn fields_come_from_the_value_before_the_dot() {
        let compilation = compiled("fields", &[("main.typ", "Text.\n")]);
        let found = complete(&compilation, "#let point = (across: 1, down: 2)\n#point.|\n", false);
        let titles = titles(&found);
        assert!(titles.contains(&"across") && titles.contains(&"down"), "{titles:?}");
        assert_eq!(found[0].from, "#let point = (across: 1, down: 2)\n#point.".len());
    }

    #[test]
    fn text_that_no_longer_compiles_still_reads_what_the_last_compile_imported() {
        let compilation = compiled("imports", &[("main.typ", "#import \"lib.typ\": shared\n"), ("lib.typ", "#let shared = (inner: 1)\n")]);
        let found = complete(&compilation, "#import \"lib.typ\": shared\n#shared.|\n", false);
        assert!(titles(&found).contains(&"inner"), "{:?}", titles(&found));
    }

    #[test]
    fn a_call_offers_its_parameters() {
        let compilation = compiled("params", &[("main.typ", "Text.\n")]);
        let found = complete(&compilation, "#text(fi|)", true);
        let fill = found.iter().find(|suggestion| suggestion.title == "fill").unwrap();
        assert_eq!(fill.kind, "Parameter");
        assert_eq!(fill.insert, Snippet { text: "fill: ".into(), fields: one(6..6) });
    }

    #[test]
    fn paths_are_relative_to_the_file_and_keep_a_closing_quote() {
        let compilation = compiled("paths", &[("main.typ", "Text.\n"), ("figures/plot.png", ""), ("notes.txt", "")]);
        let text = "#image(\"|\")";
        let found = complete(&compilation, text, true);
        assert_eq!(titles(&found), ["figures/plot.png"]);
        assert_eq!(found[0].from, 7, "the quote is replaced too");
        assert_eq!(found[0].insert.text, "\"figures/plot.png");
    }

    #[test]
    fn a_call_nobody_closed_still_offers_its_parameters() {
        let compilation = compiled("open-call", &[("main.typ", "Text.\n")]);
        assert_eq!(titles(&complete(&compilation, "#text(fi|", true)).first(), Some(&"fill"));
        assert!(titles(&complete(&compilation, "#text(\n  size: 1pt,\n  fi|\nMore", true)).contains(&"fill"));
    }

    #[test]
    fn a_string_nobody_closed_gets_its_closing_quote() {
        let compilation = compiled("open-string", &[("main.typ", "Text.\n"), ("figures/plot.png", ""), ("lib.typ", "")]);
        assert_eq!(complete(&compilation, "#image(\"fig|", true)[0].insert.text, "\"figures/plot.png\"");
        assert_eq!(complete(&compilation, "#import \"li|", true)[0].insert.text, "\"lib.typ\"");
    }

    #[test]
    fn a_string_nobody_closed_is_completed_with_text_below_it() {
        let compilation = compiled("open-string-below", &[("main.typ", "Text.\n"), ("figures/plot.png", "")]);
        for text in ["#image(\"fig|\n\nMore text below.\n", "#image(\"|\n\nMore\n", "= H\n#image(\"fig|\n= Next\nSome \"quoted\" text\n"] {
            assert_eq!(titles(&complete(&compilation, text, true)), ["figures/plot.png"], "{text}");
        }
    }

    #[test]
    fn a_string_closed_on_its_line_stays_as_it_is() {
        assert_eq!(closing_at("#image(\"fig\")\nMore \"quoted\"\n", 10), "");
        assert_eq!(closing_at("#image(\"fig\")\nMore\n", 12), "");
        assert_eq!(closing_at("#image(\"fig\n\nMore\n", 10), "\")");
        assert_eq!(closing_at("#image(\"fig\nMore \"quoted\"\n", 10), "\")");
        assert_eq!(closing_at("#let x = \"a\nb\"\n", 11), "", "a real multi-line string");
        assert_eq!(closing_at("#f(\"a\nb\") #g(\n", 5), "", "with an error elsewhere");
    }

    #[test]
    fn a_closed_call_gets_nothing_more() {
        assert_eq!(closing_at("#text(a) b", 10), "");
        assert_eq!(closing_at("#text(fill: red)", 15), "");
    }

    #[test]
    fn hidden_folders_are_not_offered_as_paths() {
        let compilation = compiled("hidden", &[("main.typ", "Text.\n"), (".thesio/state.png", "")]);
        assert!(complete(&compilation, "#image(\"|\")", true).is_empty());
    }

    #[test]
    fn a_snippet_over_several_lines_keeps_the_line_indent() {
        let compilation = compiled("indent", &[("main.typ", "Text.\n")]);
        let found = complete(&compilation, "#{\n    tabl|\n}", false);
        let table = found.iter().find(|suggestion| suggestion.title == "table").unwrap();
        assert_eq!(table.insert.text, "table(\n      \n    )", "{table:?}");
    }

    #[test]
    fn labels_come_from_the_last_document() {
        let compilation = compiled("labels", &[("main.typ", "= One <first>\n")]);
        let found = complete(&compilation, "= One <first>\n#ref(<|)", true);
        assert!(titles(&found).contains(&"first"), "{:?}", titles(&found));
    }

    #[test]
    fn labels_cover_every_included_file_with_their_kind_and_page() {
        let compilation = compiled("document-labels", &[
            ("main.typ", "#set heading(numbering: \"1.\")\n#set math.equation(numbering: \"1\")\n= Intro <intro>\n#include \"chapters/one.typ\"\n"),
            ("chapters/one.typ", concat!(
                "#pagebreak()\n",
                "#figure(rect(), caption: [A\n  plot]) <fig:plot>\n",
                "#figure(table(), caption: [Results]) <tab:results>\n",
                "#figure(rect(), supplement: none) <bare>\n",
                "#figure(rect(), supplement: [Plate], kind: \"plate\") <plate>\n",
                "$ a + b $ <eq>\n",
                "Some words <plain>\n",
                "#metadata(1) <meta>\n",
            )),
        ]);
        let summary: Vec<(&str, &str, &str, Option<usize>)> = compilation.labels.iter().map(|label| (label.key.as_str(), label.title.as_str(), label.kind.as_str(), label.page)).collect();
        assert_eq!(summary[..3], [("intro", "Intro", "Heading", Some(1)), ("fig:plot", "A plot", "Figure", Some(2)), ("tab:results", "Results", "Table", Some(2))]);
        let find = |key: &str| compilation.labels.iter().find(|label| label.key == key).unwrap();
        assert_eq!((find("bare").title.as_str(), find("bare").kind.as_str()), ("bare", "Figure"));
        assert_eq!(find("plate").kind, "Plate");
        assert_eq!(find("eq").kind, "Equation");
        assert!(compilation.labels.iter().all(|label| label.key != "plain" && label.key != "meta"));
    }

    #[test]
    fn sources_and_labels_that_at_cannot_write_stay_out_of_the_labels() {
        let compilation = compiled("label-limits", &[
            ("main.typ", "#set page(numbering: none)\n#set heading(numbering: \"1.\")\n= One <first>\n= Two <first>\n#figure(rect()) #label(\"has space\")\n#metadata(1) <end.>\n@key\n#bibliography(\"refs.yaml\")\n"),
            ("refs.yaml", "key:\n  type: book\n  title: A Book\n"),
        ]);
        let keys: Vec<(&str, &str)> = compilation.labels.iter().map(|label| (label.key.as_str(), label.title.as_str())).collect();
        assert_eq!(keys, [("first", "One")]);
    }

    #[test]
    fn a_path_replaces_the_rest_of_the_path_after_the_caret() {
        let compilation = compiled("tail", &[("main.typ", "Text.\n"), ("figures/plot.png", "")]);
        for (text, caret, tail) in [("#image(\"fig.png\")\n", 11, 4), ("#image(\"fig.png\n\nMore\n", 11, 4), ("#image(\"fig\")\n", 11, 0)] {
            let found = compilation.complete("main.typ", text, caret, true);
            let path = found.iter().find(|suggestion| suggestion.title == "figures/plot.png").unwrap_or_else(|| panic!("{text}: {found:?}"));
            assert_eq!(path.tail, tail, "{text}");
        }
    }

    #[test]
    fn a_hover_describes_a_function_in_a_sentence() {
        let text = "#heading[One]\n";
        let hover = compiled("hover-func", &[("main.typ", text)]).hover("main.typ", text, 2).unwrap();
        assert!(!hover.code);
        assert!(hover.text.contains("heading"), "{hover:?}");
    }

    #[test]
    fn a_hover_shows_a_variable_as_code_in_text_newer_than_the_compile() {
        let compilation = compiled("hover-value", &[("main.typ", "Text.\n")]);
        let text = "#let total = 1 + 2\n#total\n";
        let hover = compilation.hover("main.typ", text, text.rfind("total").unwrap()).unwrap();
        assert_eq!(hover, Hover { text: "3".into(), code: true });
    }

    #[test]
    fn a_hover_over_plain_text_says_nothing() {
        let text = "Plain words.\n";
        assert_eq!(compiled("hover-plain", &[("main.typ", text)]).hover("main.typ", text, 3), None);
    }

    #[test]
    fn a_long_value_is_cut_in_a_hover() {
        let text = "#let long = \"é\" * 2000\n#long\n";
        let hover = compiled("hover-long", &[("main.typ", text)]).hover("main.typ", text, text.rfind("long").unwrap()).unwrap();
        assert_eq!(hover.text.chars().count(), HOVER_CHARS + 1);
    }

    #[test]
    fn a_word_is_read_either_way_from_a_byte_in_it() {
        assert_eq!(word_around("#héading(x)", 4), 1..9);
        assert_eq!(word_around("a (b", 2), 2..3);
        assert_eq!(word_start("#text.fi", 8), 6);
    }

    fn opens_completion(text: &str, start: usize) -> bool { CodeStart::default().opens(text, start) }

    #[test]
    fn an_edit_is_read_from_the_text_kept_before_it() {
        let starts = CodeStart::default();
        assert!(!starts.opens("// #", 4));
        assert!(starts.opens("/ #", 3));
        assert!(!starts.opens("The end.", 8));
    }

    #[test]
    fn a_hash_opens_where_code_can_start() {
        assert!(opens_completion("Text #", 6));
        assert!(opens_completion("Text #he", 6));
        assert!(!opens_completion("Text \\#", 7));
        assert!(!opens_completion("// #", 4));
        assert!(!opens_completion("`#", 2));
        assert!(!opens_completion("https://x.org/#", 15));
        assert!(!opens_completion("Text", 4));
    }

    #[test]
    fn a_dot_opens_after_code_and_not_in_prose() {
        assert!(opens_completion("#calc.", 6));
        assert!(opens_completion("#calc.ab", 6));
        assert!(opens_completion("#heading(1).", 12));
        assert!(opens_completion("#{ calc. }", 8));
        assert!(!opens_completion("The end.", 8));
        assert!(!opens_completion("#calc. and on", 6));
        assert!(!opens_completion("#[e.g.", 6));
    }

    #[test]
    fn a_popup_follows_a_path_and_stops_at_what_cannot_be_one() {
        assert!(continues_completion("figures/plot.pn"));
        assert!(!continues_completion("he "));
        assert!(!continues_completion("he("));
    }
}
