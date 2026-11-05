//! A document and the files it includes, found the way the compiler finds
//! them, and which document opens at launch.

use crate::store::visit_vault;
use crate::typst_source::{dir_of, include_literal, vault_path, walk};
use std::collections::HashMap;
use std::path::Path;
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{LinkedNode, Source, SyntaxKind};

/// Gives a file's text by vault path.
pub type Read<'a> = &'a mut dyn FnMut(&str) -> Option<String>;

/// Where `file` may be, nearest first. Typst runs one project per root, but a
/// vault holds several: `/template.typ` is sought from the main file's folder
/// upward, then at the vault root, as `typst compile` run in that project would.
pub fn candidates<'a>(main: &'a str, file: &'a str) -> impl Iterator<Item = String> + 'a {
    let folder = |path: &&'a str| path.rsplit_once('/').map(|(parent, _)| parent);
    std::iter::successors(folder(&main), folder).map(move |dir| format!("{dir}/{file}")).chain(std::iter::once(file.to_string()))
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

struct File {
    path: String,
    text: String,
    /// Where each include is written, and the path it asks for.
    includes: Vec<(usize, String)>,
    headings: Vec<Heading>,
}

fn parse(path: String, text: String) -> File {
    let source = Source::detached(text);
    let dir = dir_of(&path);
    let (mut includes, mut headings) = (vec![], vec![]);
    walk(&LinkedNode::new(source.root()), &mut |node| match node.kind() {
        SyntaxKind::Heading => {
            let Some(heading) = ast::Heading::from_untyped(node.get()) else { return };
            let body = heading.body().to_untyped().full_text();
            let title = body.split(" <").next().unwrap_or_default().trim().to_string();
            if !title.is_empty() { headings.push(Heading { path: path.clone(), offset: node.offset(), level: heading.depth().get(), title }); }
        }
        _ => if let Some(file) = include_literal(node).and_then(|literal| vault_path(&dir, &literal)) { includes.push((node.offset(), file)); }
    });
    let text = source.text().to_string();
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

    /// Each file's vault path and text.
    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> { self.files.iter().map(|file| (file.path.as_str(), file.text.as_str())) }

    /// The headings of these files in reading order. A file that includes
    /// itself is followed once.
    pub fn outline(&self) -> Vec<Heading> {
        let mut out = vec![];
        if let Some(start) = &self.start { self.read_headings(start, &mut vec![], &mut out); }
        out
    }

    fn read_headings(&self, path: &str, open: &mut Vec<String>, out: &mut Vec<Heading>) {
        if open.iter().any(|file| file == path) { return; }
        let Some(file) = self.files.iter().find(|file| file.path == path) else { return };
        open.push(path.to_string());
        let mut includes = file.includes.iter().filter_map(|(offset, wanted)| Some((*offset, self.found.get(wanted)?))).peekable();
        for heading in &file.headings {
            while let Some((_, included)) = includes.next_if(|(offset, _)| *offset < heading.offset) { self.read_headings(included, open, out); }
            out.push(heading.clone());
        }
        for (_, included) in includes { self.read_headings(included, open, out); }
        open.pop();
    }
}

/// Which documents include which.
pub struct Documents {
    projects: Vec<(String, Project)>,
}

impl Documents {
    pub fn load<'a>(ids: impl IntoIterator<Item = &'a str>, read: Read) -> Self {
        let mut known: HashMap<String, Option<String>> = HashMap::new();
        let mut read = |path: &str| known.entry(path.to_string()).or_insert_with(|| read(path)).clone();
        Self { projects: ids.into_iter().map(|id| (id.to_string(), Project::open(id, &mut read))).collect() }
    }

    /// The documents that include `id`, directly or through other files.
    pub fn includers(&self, id: &str) -> Vec<&str> {
        self.projects.iter().filter(|(top, project)| top != id && project.contains(id)).map(|(top, _)| top.as_str()).collect()
    }

    /// The top-level document `id` belongs to: the first that includes it and
    /// that nothing includes, or `id` itself.
    pub fn top_document(&self, id: &str) -> String {
        self.includers(id).into_iter().find(|top| self.includers(top).is_empty()).unwrap_or(id).to_string()
    }
}

/// The document to open when nothing asks for another: the newest of these
/// (id, modified) pairs, the first of those modified together.
pub fn newest<'a>(documents: impl IntoIterator<Item = (&'a str, u64)>) -> Option<&'a str> {
    documents.into_iter().min_by_key(|(_, modified)| std::cmp::Reverse(*modified)).map(|(id, _)| id)
}

/// What launch will most likely open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchTarget {
    pub document: String,
    /// The top-level file that includes `document`.
    pub main: String,
    /// `document`'s text.
    pub text: String,
}

/// Read-only, for compiling ahead of the UI; a wrong guess only wastes that work.
pub fn launch_target(root: &Path) -> Option<LaunchTarget> {
    let mut documents = vec![];
    visit_vault(root, &mut |item| {
        if item.is_document() && let Ok(text) = std::fs::read_to_string(&item.path) { documents.push((item.id.clone(), item.modified(), text)); }
    });
    let document = newest(documents.iter().map(|(id, modified, _)| (id.as_str(), *modified)))?.to_string();
    documents.sort_by_key(|(_, modified, _)| std::cmp::Reverse(*modified));
    let texts: HashMap<&str, &str> = documents.iter().map(|(id, _, text)| (id.as_str(), text.as_str())).collect();
    let main = Documents::load(documents.iter().map(|(id, ..)| id.as_str()), &mut |path| texts.get(path).map(|text| text.to_string())).top_document(&document);
    let text = texts.get(document.as_str())?.to_string();
    Some(LaunchTarget { document, main, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_target_is_the_newest_document_within_its_top_file() {
        let root = std::env::temp_dir().join(format!("thesio-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Thesis/chapters")).unwrap();
        std::fs::create_dir_all(root.join(".trash")).unwrap();
        let write = |path: &str, text: &str, age: u64| {
            let file = std::fs::File::create(root.join(path)).unwrap();
            std::io::Write::write_all(&mut &file, text.as_bytes()).unwrap();
            file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age)).unwrap();
        };
        write("old.typ", "old", 500);
        write("Thesis/thesis.typ", "#include \"chapters/one.typ\"", 300);
        write("Thesis/chapters/one.typ", "newest", 100);
        write(".trash/hidden.typ", "hidden", 0);
        let expected = LaunchTarget { document: "Thesis/chapters/one.typ".into(), main: "Thesis/thesis.typ".into(), text: "newest".into() };
        assert_eq!(launch_target(&root), Some(expected));
        std::fs::remove_dir_all(root).unwrap();
    }

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
        let summary: Vec<(&str, usize, &str)> = headings.iter().map(|h| (h.path.as_str(), h.level, h.title.as_str())).collect();
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
        assert_eq!(project.outline()[0].title, "Right folder");
        let documents = Documents::load(files.map(|(id, _)| id), &mut reader(&files));
        assert_eq!(documents.includers("Thesis/shared.typ"), ["Thesis/thesis.typ"]);
        assert_eq!(documents.top_document("shared.typ"), "shared.typ");
        assert_eq!(documents.top_document("Thesis/shared.typ"), "Thesis/thesis.typ");
    }
}
