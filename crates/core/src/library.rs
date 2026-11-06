//! The citation library: Hayagriva entries in `library.yaml` at the vault root.
//! The YAML mapping is the source of truth, so editing an entry keeps every
//! field the form does not show.

use crate::biblatex;
use crate::history::Versions;
use crate::identifier::PaperIds;
use crate::paths::{LIBRARY_FILE, base_name, document_name, natural_cmp, write_atomic};
use crate::source_type::{self, DOI, Field, MISC, Slot, SourceType, read, write};
use anyhow::{Context, Result};
use serde_yaml::{Mapping, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The vault folder that holds downloaded papers.
pub const PAPERS_DIR: &str = "sources";

/// The vault folder that holds notes on sources.
pub const NOTES_DIR: &str = "notes";

/// The notes document that goes with paper `id`, or with `paper_file(key)`.
pub fn note_file(paper: &str) -> String {
    let name = base_name(paper);
    format!("{NOTES_DIR}/{}", document_name(name.strip_suffix(".pdf").unwrap_or(name)))
}

/// The vault path of the paper for `key`. Keys may hold characters a file name should not.
pub fn paper_file(key: &str) -> String {
    let stem: String = key.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect();
    paper_named(stem.trim_start_matches('.'))
}

/// The vault path of a paper with no source, under the name it came with.
pub fn paper_named(stem: &str) -> String {
    format!("{PAPERS_DIR}/{stem}.pdf")
}

/// A table row.
#[derive(Clone, Debug)]
pub struct Row {
    pub key: String,
    pub kind: String,
    pub title: String,
    pub authors: String,
    pub year: String,
    pub tags: Vec<String>,
}

impl Row {
    /// What to call it: its title, or its key without one.
    pub fn label(&self) -> &str {
        if self.title.is_empty() { &self.key } else { &self.title }
    }

    /// Whether a lowercase search `query` finds this row.
    pub fn matches(&self, query: &str) -> bool {
        [&self.title, &self.authors, &self.year, &self.key].into_iter().chain(&self.tags).any(|text| text.to_lowercase().contains(query))
    }
}

pub fn row_of_paper<'a>(rows: &'a [Row], id: &str) -> Option<&'a Row> {
    rows.iter().find(|row| paper_file(&row.key) == id)
}

/// The outcome of adding a looked-up entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Added {
    New(String),
    /// It was in the library already, under this key.
    Duplicate(String),
}

pub struct Library {
    path: PathBuf,
    entries: Mapping,
    parsed: hayagriva::Library,
}

/// Text Hayagriva or YAML refused as a library.
#[derive(Debug)]
pub struct InvalidLibrary {
    path: PathBuf,
    pub reason: String,
}

impl std::fmt::Display for InvalidLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} is not a valid library: {}", self.path.display(), self.reason)
    }
}

impl std::error::Error for InvalidLibrary {}

fn yaml_error(path: &Path, error: impl std::fmt::Display) -> anyhow::Error {
    InvalidLibrary { path: path.to_path_buf(), reason: error.to_string() }.into()
}

impl Library {
    /// Reads the vault's library. A file that does not parse is an error, so
    /// that nothing overwrites it.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(LIBRARY_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("Could not read {}", path.display())),
        };
        Self::parse(path, &text)
    }

    fn parse(path: PathBuf, text: &str) -> Result<Self> {
        let entries: Mapping = match serde_yaml::from_str::<Option<Mapping>>(text) {
            Ok(entries) => entries.unwrap_or_default(),
            Err(e) => return Err(yaml_error(&path, e)),
        };
        let parsed = if entries.is_empty() { hayagriva::Library::new() } else { hayagriva::io::from_yaml_str(text).map_err(|e| yaml_error(&path, e))? };
        Ok(Self { path, entries, parsed })
    }

    pub fn versions(&self) -> Versions {
        Versions::of(self.path.parent().unwrap_or(Path::new("")), LIBRARY_FILE)
    }

    /// The file as it stands.
    pub fn text(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }

    /// Keeps the file as it stands before a change. A run of quick edits keeps
    /// the state before the first, unless `now`. A file that cannot be read
    /// cannot be kept, which stops the change.
    fn keep_version(&self, now: bool) -> Result<()> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).with_context(|| format!("Could not read {}", self.path.display())),
        };
        if text.is_empty() { return Ok(()); }
        let versions = self.versions();
        (if now { versions.keep(&text) } else { versions.keep_paced(&text) }).map(|_| ())
    }

    /// Puts a version back, keeping what it replaces. It is checked first, so
    /// a version that does not parse changes nothing.
    pub fn restore(&mut self, text: &str) -> Result<()> {
        self.replace(text, true)
    }

    /// Validates `entries` with Hayagriva, then writes them.
    fn commit(&mut self, entries: Mapping) -> Result<()> {
        let text = if entries.is_empty() { String::new() } else { serde_yaml::to_string(&entries)? };
        self.replace(&text, false)
    }

    /// Writes `text` over the file if it parses, keeping a version first.
    fn replace(&mut self, text: &str, now: bool) -> Result<()> {
        let next = Self::parse(self.path.clone(), text)?;
        self.keep_version(now)?;
        write_atomic(&self.path, text.as_bytes())?;
        *self = next;
        Ok(())
    }

    pub fn len(&self) -> usize { self.parsed.len() }
    pub fn is_empty(&self) -> bool { self.parsed.is_empty() }
    pub fn contains(&self, key: &str) -> bool { self.parsed.get(key).is_some() }

    fn entry(&self, key: &str) -> Option<&Mapping> { self.entries.get(key)?.as_mapping() }

    pub fn paper_ids(&self, key: &str) -> PaperIds {
        let Some(entry) = self.entry(key) else { return PaperIds::default() };
        PaperIds { doi: Some(read(entry, DOI.slot)).filter(|id| !id.is_empty()), arxiv: source_type::arxiv_id(entry) }
    }

    pub fn rows(&self) -> Vec<Row> {
        self.parsed.iter().map(|entry| Row {
            key: entry.key().to_string(),
            kind: self.source_type(entry.key()).map_or("", |kind| kind.label).to_string(),
            title: entry.title().map(|t| t.to_string()).unwrap_or_default(),
            authors: entry.authors().unwrap_or_default().iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", "),
            year: entry.date_any().map(|d| d.year.to_string()).unwrap_or_default(),
            tags: self.entry(entry.key()).map(source_type::tags).unwrap_or_default(),
        }).collect()
    }

    /// Every tag in use with its number of entries, in natural order.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts = std::collections::BTreeMap::new();
        for tag in self.entries.values().filter_map(Value::as_mapping).flat_map(source_type::tags) { *counts.entry(tag).or_insert(0) += 1; }
        let mut tags: Vec<(String, usize)> = counts.into_iter().collect();
        tags.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        tags
    }

    /// The tag in use that `name` differs from only in case.
    pub fn tag_like(&self, name: &str) -> Option<String> {
        self.tags().into_iter().map(|(tag, _)| tag).find(|tag| tag.eq_ignore_ascii_case(name))
    }

    /// `tag` as it should be stored. A name that differs only in case from a
    /// tag in use, other than `old`, becomes that tag.
    fn tag_name(&self, tag: &str, old: Option<&str>) -> Result<String> {
        let tag = tag.trim();
        anyhow::ensure!(!tag.is_empty(), "Enter a tag");
        anyhow::ensure!(!tag.contains(','), "Tags cannot contain commas");
        let known = self.tags().into_iter().map(|(name, _)| name).find(|name| Some(name.as_str()) != old && name.eq_ignore_ascii_case(tag));
        Ok(known.unwrap_or_else(|| tag.to_string()))
    }

    /// Adds `tag` to the entries in `keys`, or removes it.
    pub fn set_tag(&mut self, keys: &[String], tag: &str, on: bool) -> Result<()> {
        let tag = self.tag_name(tag, None)?;
        let mut entries = self.entries.clone();
        for key in keys {
            let Some(entry) = entries.get_mut(key.as_str()).and_then(Value::as_mapping_mut) else { continue };
            let mut tags = source_type::tags(entry);
            tags.retain(|name| *name != tag);
            if on { tags.push(tag.clone()); }
            source_type::set_tags(entry, tags);
        }
        self.commit(entries)
    }

    pub fn source_type(&self, key: &str) -> Option<&'static SourceType> {
        self.entry(key).map(SourceType::of)
    }

    /// What the editor shows for a field of `key`.
    pub fn field_text(&self, key: &str, field: &Field) -> String {
        self.entry(key).map(|entry| read(entry, field.slot)).unwrap_or_default()
    }

    /// Adds an empty entry of `kind` for the editor to fill in.
    pub fn add_blank(&mut self, kind: &SourceType) -> Result<String> {
        let mut entry = Mapping::new();
        kind.apply(&mut entry);
        let key = self.fresh_key(&entry);
        let mut entries = self.entries.clone();
        entries.insert(key.as_str().into(), Value::Mapping(entry));
        self.commit(entries)?;
        Ok(key)
    }

    /// Whether changing `key` to `kind` would lose the work it was published in.
    pub fn type_change_drops_parent(&self, key: &str, kind: &SourceType) -> bool {
        self.entry(key).is_some_and(|entry| kind.drops_parent(entry))
    }

    /// Changes `key` to `kind`, keeping every field the types share.
    pub fn set_source_type(&mut self, key: &str, kind: &SourceType) -> Result<()> {
        let mut entries = self.entries.clone();
        let Some(entry) = entries.get_mut(key).and_then(Value::as_mapping_mut) else { return Ok(()) };
        kind.apply(entry);
        self.commit(entries)
    }

    /// Writes the fields that changed and renames the entry to `new_key`.
    /// Fields left alone keep their exact form, such as a person's `prefix`.
    /// An entry that would not parse is rejected unwritten. True if anything changed.
    pub fn save_fields(&mut self, key: &str, new_key: &str, fields: &[(Field, String)]) -> Result<bool> {
        let new_key = new_key.trim();
        anyhow::ensure!(!new_key.is_empty(), "Enter a citation key");
        anyhow::ensure!(!new_key.contains(char::is_whitespace), "Citation keys cannot contain spaces");
        anyhow::ensure!(key == new_key || !self.contains(new_key), "{new_key} is already in the library");
        let Some(original) = self.entry(key) else { return Ok(false) };
        let mut entry = original.clone();
        for (field, text) in fields {
            if read(original, field.slot) != text.trim() { write(&mut entry, field.slot, text); }
        }
        if key == new_key && entry == *original { return Ok(false); }
        let entries = self.entries.iter().map(|(k, v)| {
            if k.as_str() == Some(key) { (new_key.into(), Value::Mapping(entry.clone())) } else { (k.clone(), v.clone()) }
        }).collect();
        self.commit(entries)?;
        Ok(true)
    }

    /// Renames a tag on every entry. Renaming to a tag in use merges the two.
    pub fn rename_tag(&mut self, old: &str, new: &str) -> Result<()> {
        let new = self.tag_name(new, Some(old))?;
        self.change_tag(old, Some(&new))
    }

    pub fn delete_tag(&mut self, tag: &str) -> Result<()> {
        self.change_tag(tag, None)
    }

    fn change_tag(&mut self, old: &str, new: Option<&str>) -> Result<()> {
        let mut entries = self.entries.clone();
        for entry in entries.values_mut().filter_map(Value::as_mapping_mut) {
            let tags = source_type::tags(entry);
            if !tags.iter().any(|tag| tag == old) { continue; }
            let mut next: Vec<String> = vec![];
            for tag in tags.iter().flat_map(|tag| if tag == old { new.map(str::to_string) } else { Some(tag.clone()) }) {
                if !next.contains(&tag) { next.push(tag); }
            }
            source_type::set_tags(entry, next);
        }
        self.commit(entries)
    }

    pub fn delete_entries(&mut self, keys: &[String]) -> Result<()> {
        let mut entries = self.entries.clone();
        for key in keys { entries.remove(key.as_str()); }
        self.commit(entries)
    }

    /// Imports BibLaTeX text. Keys already in the library are kept as they
    /// are. Returns the number of entries added and skipped.
    pub fn import_biblatex(&mut self, text: &str) -> Result<(usize, usize)> {
        let mut entries = self.entries.clone();
        let (mut added, mut skipped) = (0, 0);
        for (key, entry) in biblatex::import(text)? {
            if entries.contains_key(&key) || self.find_duplicate(&entry).is_some() {
                skipped += 1;
                continue;
            }
            entries.insert(key, Value::Mapping(entry));
            added += 1;
        }
        if added > 0 { self.commit(entries)?; }
        Ok((added, skipped))
    }

    /// Adds a looked-up entry under a generated key, unless an entry with the
    /// same DOI, PMID, ISBN, or arXiv ID is here already.
    pub fn add_fetched(&mut self, mut entry: Mapping) -> Result<Added> {
        if let Some(key) = self.find_duplicate(&entry) { return Ok(Added::Duplicate(key)); }
        let key = self.fresh_key(&entry);
        if read(&entry, Slot::Type).is_empty() { write(&mut entry, Slot::Type, MISC); }
        let mut entries = self.entries.clone();
        entries.insert(key.as_str().into(), Value::Mapping(entry));
        self.commit(entries)?;
        Ok(Added::New(key))
    }

    fn find_duplicate(&self, entry: &Mapping) -> Option<String> {
        let wanted = source_type::work_ids(entry);
        self.entries.iter().find(|(_, existing)| existing.as_mapping().is_some_and(|map| source_type::work_ids(map).iter().any(|n| wanted.contains(n)))).and_then(|(key, _)| key.as_str()).map(str::to_string)
    }

    /// `surname` + `year`, with a letter added when the key is taken.
    fn fresh_key(&self, entry: &Mapping) -> String {
        let surname = source_type::first_surname(entry).map_or(String::new(), |name| ascii_key(&name));
        let year = source_type::year(entry);
        let base = match (surname.is_empty(), year.is_empty()) {
            (true, true) => "entry".to_string(),
            (true, false) => format!("entry{year}"),
            _ => format!("{surname}{year}"),
        };
        if !self.contains(&base) { return base; }
        ('b'..='z').map(|letter| format!("{base}{letter}")).find(|key| !self.contains(key))
            .unwrap_or_else(|| (2..).map(|n| format!("{base}-{n}")).find(|key| !self.contains(key)).unwrap_or(base))
    }

    pub fn export_biblatex(&self) -> String { biblatex::export(&self.parsed, &self.entries) }

    /// YAML for the entries in `keys`, in library order. Tags stay out: this
    /// goes into documents, and a new tag should not rewrite them.
    pub fn subset(&self, keys: &BTreeSet<String>) -> Result<String> {
        let entries: Mapping = self.entries.iter().filter(|(k, _)| k.as_str().is_some_and(|k| keys.contains(k))).map(|(k, v)| {
            let mut entry = v.clone();
            if let Some(map) = entry.as_mapping_mut() { source_type::set_tags(map, vec![]); }
            (k.clone(), entry)
        }).collect();
        Ok(if entries.is_empty() { String::new() } else { serde_yaml::to_string(&entries)? })
    }
}

/// Lowercase ASCII for a citation key, folding common accented letters.
fn ascii_key(text: &str) -> String {
    let mut key = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' | 'æ' => key.push_str("ae"),
            'ö' | 'œ' => key.push_str("oe"),
            'ü' => key.push_str("ue"),
            'ß' => key.push_str("ss"),
            'à' | 'á' | 'â' | 'ã' | 'å' => key.push('a'),
            'ç' | 'č' | 'ć' => key.push('c'),
            'è' | 'é' | 'ê' | 'ë' => key.push('e'),
            'ì' | 'í' | 'î' | 'ï' => key.push('i'),
            'ñ' | 'ń' => key.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ø' => key.push('o'),
            'š' | 'ś' => key.push('s'),
            'ù' | 'ú' | 'û' => key.push('u'),
            'ý' | 'ÿ' => key.push('y'),
            'ž' | 'ź' => key.push('z'),
            c if c.is_ascii_alphanumeric() => key.push(c),
            _ => {}
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("thesio-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn library(name: &str) -> Library {
        Library::load(&root(name)).unwrap()
    }

    fn tags_of(library: &Library, key: &str) -> Vec<String> {
        library.rows().into_iter().find(|row| row.key == key).unwrap().tags
    }

    const ZOTERO: &str = r#"
@article{parry2022, title = {Flaky}, author = {Parry, Owain}, journal = {TOSEM}, volume = {31}, number = {1}, pages = {1--74},
  year = {2022}, doi = {10.1145/3476105}, issn = {1049-331X}, language = {en}, keywords = {testing, flaky tests}}
@book{knuth1984, title = {TeXbook}, author = {Knuth, Donald}, editor = {Doe, Jane}, publisher = {AW}, address = {Reading},
  edition = {2}, isbn = {0-201-13447-0}, year = {1984}, note = {A note}}
@incollection{chap2020, title = {Chapter}, booktitle = {Big Book}, publisher = {Pub}, author = {Doe, A.}, year = {2020}}
@phdthesis{phd2020, title = {On Tests}, type = {PhD thesis}, school = {TU Berlin}, author = {Doe, Jane}, year = {2020}}
@misc{arx2021, title = {Preprint}, author = {Doe, A.}, year = {2021}, eprint = {2112.12331}, eprinttype = {arxiv}}
@software{sw2022, title = {Tool}, author = {Doe, A.}, version = {1.2}, year = {2022}}
@misc{web2024, title = {Page}, url = {https://example.com}, urldate = {2024-03-15}, year = {2024}}
"#;

    fn zotero(name: &str) -> Library {
        let mut library = library(name);
        assert_eq!(library.import_biblatex(ZOTERO).unwrap(), (7, 0));
        library
    }

    fn kinds(library: &Library) -> Vec<(String, String)> {
        let mut kinds: Vec<_> = library.rows().into_iter().map(|row| (row.key, row.kind)).collect();
        kinds.sort();
        kinds
    }

    #[test]
    fn a_zotero_export_keeps_its_types() {
        assert_eq!(kinds(&zotero("zotero-types")), [
            ("arx2021", "Preprint"), ("chap2020", "Book Section"), ("knuth1984", "Book"), ("parry2022", "Journal Article"),
            ("phd2020", "Thesis"), ("sw2022", "Software, Data"), ("web2024", "Web Page"),
        ].map(|(key, label)| (key.to_string(), label.to_string())));
    }

    #[test]
    fn keywords_become_tags() {
        let tags: Vec<String> = zotero("zotero-tags").tags().into_iter().map(|(tag, _)| tag).collect();
        assert_eq!(tags, ["flaky tests", "testing"]);
    }

    #[test]
    fn import_reads_what_hayagriva_drops_and_what_it_files_on_the_parent() {
        let library = zotero("zotero-details");
        assert_eq!(library.field_text("parry2022", &source_type::LANGUAGE), "en");
        assert_eq!(library.field_text("chap2020", &source_type::PUBLISHER), "Pub");
    }

    #[test]
    fn importing_the_same_file_again_adds_nothing() {
        assert_eq!(zotero("zotero-again").import_biblatex(ZOTERO).unwrap(), (0, 7));
    }

    fn tagged(name: &str) -> Library {
        let mut library = library(name);
        library.import_biblatex("@book{a, title = {A}, keywords = {old, keep}}\n@book{b, title = {B}, keywords = {old, merged}}\n@book{c, title = {C}, keywords = {merged}}").unwrap();
        library
    }

    #[test]
    fn a_renamed_tag_changes_on_every_entry() {
        let mut library = tagged("tag-rename");
        library.rename_tag("old", "fresh").unwrap();
        assert_eq!(tags_of(&library, "a"), ["fresh", "keep"]);
    }

    #[test]
    fn a_renamed_tag_merges_into_one_in_use() {
        let mut library = tagged("tag-merge");
        library.rename_tag("old", "MERGED").unwrap();
        assert_eq!(library.tags(), [("keep".to_string(), 1), ("merged".to_string(), 3)]);
        assert_eq!(tags_of(&library, "b"), ["merged"]);
    }

    #[test]
    fn a_deleted_tag_leaves_no_empty_list_behind() {
        let mut library = library("tag-delete");
        library.import_biblatex("@book{a, title = {A}, keywords = {only}}").unwrap();
        library.delete_tag("only").unwrap();
        assert!(library.tags().is_empty());
        assert!(!library.text().contains("tags"));
    }

    #[test]
    fn choosing_preprint_sticks_and_choosing_again_leaves_no_trace() {
        let mut library = library("preprint");
        library.import_biblatex("@article{a, title = {A}}").unwrap();
        library.set_source_type("a", &source_type::PREPRINT).unwrap();
        assert_eq!(library.source_type("a").unwrap().label, "Preprint");
        library.set_source_type("a", &source_type::JOURNAL_ARTICLE).unwrap();
        assert_eq!(library.source_type("a").unwrap().label, "Journal Article");
        assert!(!library.text().contains("genre"));
    }

    #[test]
    fn a_deleted_entry_can_come_back_from_a_version() {
        let mut library = zotero("versions");
        let before = library.text();
        library.delete_entries(&["knuth1984".to_string()]).unwrap();
        assert!(!library.contains("knuth1984"));
        let versions = library.versions().list();
        assert_eq!(library.versions().read(versions[0]).unwrap(), before);
        library.restore(&before).unwrap();
        assert!(library.contains("knuth1984"));
    }

    #[test]
    fn a_version_that_does_not_parse_changes_nothing() {
        let mut library = zotero("bad-version");
        let before = library.text();
        assert!(library.restore("not: [valid").is_err());
        assert_eq!(library.text(), before);
    }

    #[test]
    fn a_new_key_for_a_known_doi_is_a_duplicate() {
        let mut library = zotero("duplicate");
        assert_eq!(library.import_biblatex("@article{other, title = {Flaky}, doi = {10.1145/3476105}}").unwrap(), (0, 1));
    }

    #[test]
    fn a_numeric_arxiv_id_is_a_duplicate() {
        let mut library = library("numeric-arxiv");
        let entry = |text: &str| serde_yaml::from_str::<Mapping>(text).unwrap();
        let first = library.add_fetched(entry("type: Article\ntitle: A\nserial-number:\n  arxiv: 2112.12331\n")).unwrap();
        assert_eq!(first, Added::New("entry".to_string()));
        assert_eq!(library.add_fetched(entry("type: Article\ntitle: B\nserial-number:\n  arxiv: 2112.12331\n")).unwrap(), Added::Duplicate("entry".to_string()));
    }

    #[test]
    fn numbers_many_works_share_do_not_make_a_duplicate() {
        let mut library = library("shared-serial");
        let entry = |doi: &str| serde_yaml::from_str::<Mapping>(&format!("type: Article\ntitle: T\nserial-number:\n  doi: {doi}\n  issn: 1049-331X\n  version: 1\n")).unwrap();
        library.add_fetched(entry("10.1/a")).unwrap();
        assert!(matches!(library.add_fetched(entry("10.1/b")).unwrap(), Added::New(_)));
    }

    #[test]
    fn a_library_that_cannot_be_read_back_is_not_replaced() {
        let root = root("unreadable");
        let mut library = Library::load(&root).unwrap();
        library.import_biblatex("@book{a, title = {A}}").unwrap();
        let unreadable = [0xff, 0xfe, b'a'];
        std::fs::write(root.join(LIBRARY_FILE), unreadable).unwrap();
        assert!(library.delete_entries(&["a".to_string()]).is_err());
        assert_eq!(std::fs::read(root.join(LIBRARY_FILE)).unwrap(), unreadable);
    }

    #[test]
    fn a_refused_edit_gives_its_reason_apart_from_the_path() {
        let mut library = library("refused");
        library.import_biblatex("@book{a, title = {A}}").unwrap();
        let date = (source_type::DATE, "not a date".to_string());
        let error = library.save_fields("a", "a", &[date]).unwrap_err();
        let reason = &error.downcast_ref::<InvalidLibrary>().unwrap().reason;
        assert!(!reason.is_empty() && !reason.contains(LIBRARY_FILE), "{reason}");
    }

    #[test]
    fn export_then_import_loses_nothing_it_can_hold() {
        let first = zotero("export");
        let mut second = library("reimport");
        assert_eq!(second.import_biblatex(&first.export_biblatex()).unwrap(), (7, 0));
        assert_eq!(kinds(&second), kinds(&first));
        for key in first.rows().into_iter().map(|row| row.key) {
            for field in first.source_type(&key).unwrap().fields {
                assert_eq!(second.field_text(&key, field), first.field_text(&key, field), "{key}: {}", field.label);
            }
            assert_eq!(tags_of(&second, &key), tags_of(&first, &key), "{key}: tags");
        }
    }
}
