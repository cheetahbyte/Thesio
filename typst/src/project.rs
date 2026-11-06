//! A document and the files it includes, found the way the compiler finds them.

use crate::paths::{dir_parts, parent_of};
use crate::typst_source::{include_literal, vault_path, walk};
use std::collections::HashMap;
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{LinkedNode, Source, SyntaxKind};

/// Gives a file's text by vault path.
pub type Read<'a> = &'a mut dyn FnMut(&str) -> Option<String>;

/// Where `file` may be, nearest first. Typst runs one project per root, but a
/// vault holds several: `/template.typ` is sought from the main file's folder
/// upward, then at the vault root, as `typst compile` run in that project would.
pub fn candidates<'a>(main: &'a str, file: &'a str) -> impl Iterator<Item = String> + 'a {
    std::iter::successors(parent_of(main), |dir| parent_of(dir)).map(move |dir| format!("{dir}/{file}")).chain(std::iter::once(file.to_string()))
}

/// A heading as written in one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub offset: usize,
    /// 1 for `=`.
    pub level: usize,
    pub title: String,
}

/// What one file's text says about its place in a document: its headings, and
/// each `#include` with where it is written and the path as written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub headings: Vec<Heading>,
    pub includes: Vec<(usize, String)>,
}

pub fn parse_document(text: &str) -> Parsed {
    let source = Source::detached(text);
    let mut parsed = Parsed::default();
    walk(&LinkedNode::new(source.root()), &mut |node| match node.kind() {
        SyntaxKind::Heading => {
            let Some(heading) = ast::Heading::from_untyped(node.get()) else { return };
            let body = heading.body().to_untyped().full_text();
            let title = body.split(" <").next().unwrap_or_default().trim().to_string();
            if !title.is_empty() { parsed.headings.push(Heading { offset: node.offset(), level: heading.depth().get(), title }); }
        }
        _ => if let Some(literal) = include_literal(node) { parsed.includes.push((node.offset(), literal)); }
    });
    parsed
}

struct File {
    path: String,
    text: String,
    /// Where each include is written, and the path it asks for.
    includes: Vec<(usize, String)>,
    headings: Vec<Heading>,
}

fn parse(path: String, text: String) -> File {
    let Parsed { headings, includes } = parse_document(&text);
    let dir = dir_parts(&path);
    let includes = includes.into_iter().filter_map(|(offset, literal)| Some((offset, vault_path(&dir, &literal)?))).collect();
    File { path, text, includes, headings }
}

/// The files of one compile, each once.
pub struct Project {
    start: Option<String>,
    files: Vec<File>,
    /// The file each requested path turned out to be.
    found: HashMap<String, String>,
}

impl Project {
    /// `main` and everything it includes.
    pub fn open(main: &str, read: Read) -> Self { Self::within(main, main, read) }

    /// `start` and what it includes, when `main` is the file being compiled.
    pub fn within(main: &str, start: &str, read: Read) -> Self {
        let (mut files, mut found): (Vec<File>, HashMap<String, String>) = (vec![], HashMap::new());
        let mut pending = vec![start.to_string()];
        while let Some(wanted) = pending.pop() {
            if found.contains_key(&wanted) { continue; }
            let Some((path, text)) = candidates(main, &wanted).find_map(|path| read(&path).map(|text| (path, text))) else { continue };
            found.insert(wanted, path.clone());
            if files.iter().any(|file| file.path == path) { continue; }
            let file = parse(path, text);
            pending.extend(file.includes.iter().map(|(_, path)| path.clone()));
            files.push(file);
        }
        Self { start: found.get(start).cloned(), files, found }
    }

    pub fn contains(&self, path: &str) -> bool { self.files.iter().any(|file| file.path == path) }

    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> { self.files.iter().map(|file| (file.path.as_str(), file.text.as_str())) }

    /// The headings of these files in reading order, with the file each is in.
    /// A file that includes itself is followed once.
    pub fn outline(&self) -> Vec<(String, Heading)> {
        let mut out = vec![];
        if let Some(start) = &self.start { self.read_headings(start, &mut vec![], &mut out); }
        out
    }

    fn read_headings(&self, path: &str, open: &mut Vec<String>, out: &mut Vec<(String, Heading)>) {
        if open.iter().any(|file| file == path) { return; }
        let Some(file) = self.files.iter().find(|file| file.path == path) else { return };
        open.push(path.to_string());
        let mut includes = file.includes.iter().filter_map(|(offset, wanted)| Some((*offset, self.found.get(wanted)?))).peekable();
        for heading in &file.headings {
            while let Some((_, included)) = includes.next_if(|(offset, _)| *offset < heading.offset) { self.read_headings(included, open, out); }
            out.push((path.to_string(), heading.clone()));
        }
        for (_, included) in includes { self.read_headings(included, open, out); }
        open.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader<'a>(files: &'a [(&'a str, &'a str)]) -> impl FnMut(&str) -> Option<String> + 'a {
        |path| files.iter().find(|(file, _)| *file == path).map(|(_, text)| text.to_string())
    }

    #[test]
    fn an_outline_reads_through_includes_in_order() {
        let files = [
            ("main.typ", "= One <one>\n#include \"ch/a.typ\"\n== Two\n#include \"ch/b.typ\"\n// = Not a heading\n`= Nor this`\n"),
            ("ch/a.typ", "= In A\nText\n#include \"../main.typ\"\n"),
            ("ch/b.typ", "=== In *B*\n"),
        ];
        let headings = Project::open("main.typ", &mut reader(&files)).outline();
        let summary: Vec<(&str, usize, &str)> = headings.iter().map(|(path, h)| (path.as_str(), h.level, h.title.as_str())).collect();
        assert_eq!(summary, [("main.typ", 1, "One"), ("ch/a.typ", 1, "In A"), ("main.typ", 2, "Two"), ("ch/b.typ", 3, "In *B*")]);
    }

    #[test]
    fn a_root_path_means_what_the_compiler_means_by_it() {
        let files = [
            ("shared.typ", "= Wrong folder\n"),
            ("Thesis/shared.typ", "= Right folder\n"),
            ("Thesis/thesis.typ", "#include \"/shared.typ\"\n"),
        ];
        let project = Project::open("Thesis/thesis.typ", &mut reader(&files));
        assert!(project.contains("Thesis/shared.typ"));
        assert!(!project.contains("shared.typ"));
        assert_eq!(project.outline()[0].1.title, "Right folder");
    }

    #[test]
    fn a_document_lists_its_includes_as_written() {
        let parsed = parse_document("= One <one>\n#include \"../ch/a.typ\"\n#include \"https://x\"\n");
        assert_eq!(parsed.headings, [Heading { offset: 0, level: 1, title: "One".into() }]);
        assert_eq!(parsed.includes, [(21, "../ch/a.typ".to_string()), (44, "https://x".to_string())]);
    }
}
