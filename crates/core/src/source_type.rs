//! The kinds of source the Library knows, and which fields each one has. A kind
//! is a Hayagriva type plus the type of its parent, so `library.yaml` stays
//! plain Hayagriva and entries from any tool get a kind without a new field.

use crate::identifier::arxiv_of_link;
use serde_yaml::{Mapping, Value};

/// Where a field's value lives in an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The Hayagriva type.
    Type,
    /// The Hayagriva type of the work this one was published in.
    ParentType,
    Own(&'static str),
    /// On the work this one was published in.
    Parent(&'static str),
    /// On the entry or its parent, whichever has it. Lookups put these on the parent.
    Either(&'static str),
    Serial(&'static str),
    Persons(&'static str),
    Date,
    Publisher,
    /// Where it was published: with the publisher, or on the entry without one.
    Place,
    Url,
    Accessed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub label: &'static str,
    pub hint: &'static str,
    /// Wraps over several lines in the editor.
    pub long: bool,
    pub slot: Slot,
    /// Its name in BibLaTeX, or `None` where BibLaTeX has no place for it.
    /// Export and import read this, so a field states it once, here.
    pub biblatex: Option<&'static str>,
}

impl Field {
    /// Its name in an entry of BibLaTeX type `kind`.
    pub fn biblatex_name(&self, kind: &str) -> Option<&'static str> {
        self.biblatex.map(|name| match name {
            "booktitle" if kind == "article" => "journaltitle",
            "organization" if matches!(kind, "thesis" | "report") => "institution",
            name => name,
        })
    }
}

/// BibLaTeX's field for tags, and the one that says what `eprint` holds.
pub const KEYWORDS: &str = "keywords";
pub const EPRINT_TYPE: (&str, &str) = ("eprinttype", "arxiv");

const fn field(label: &'static str, hint: &'static str, slot: Slot, biblatex: &'static str) -> Field { Field { label, hint, long: false, slot, biblatex: Some(biblatex) } }
const fn wrapped(label: &'static str, slot: Slot, biblatex: &'static str) -> Field { Field { label, hint: "", long: true, slot, biblatex: Some(biblatex) } }
const fn unexported(label: &'static str, hint: &'static str, slot: Slot) -> Field { Field { label, hint, long: false, slot, biblatex: None } }

const WORK_ID_KINDS: [&str; 4] = ["doi", "arxiv", "pmid", "isbn"];

/// A bare `serial-number` counts as this kind of number.
const PLAIN_SERIAL: &str = "serial";

pub const TITLE: Field = wrapped("Title", Slot::Own("title"), "title");
pub const AUTHORS: Field = wrapped("Authors", Slot::Persons("author"), "author");
const EDITORS: Field = wrapped("Editors", Slot::Persons("editor"), "editor");
pub const DATE: Field = field("Date", "2024 or 2024-03-15", Slot::Date, "date");
pub const PUBLISHER: Field = field("Publisher", "", Slot::Publisher, "publisher");
const PLACE: Field = field("Place", "", Slot::Place, "location");
const VOLUME: Field = field("Volume", "", Slot::Either("volume"), "volume");
const ISSUE: Field = field("Issue", "", Slot::Either("issue"), "number");
const EDITION: Field = field("Edition", "", Slot::Own("edition"), "edition");
pub const PAGES: Field = field("Pages", "891-921", Slot::Own("page-range"), "pages");
const PAGE_COUNT: Field = field("Pages", "", Slot::Own("page-total"), "pagetotal");
const NUMBER: Field = field("Number", "", Slot::Serial(PLAIN_SERIAL), "number");
const RUNTIME: Field = unexported("Running Time", "01:30:00", Slot::Own("runtime"));
pub const DOI: Field = field("DOI", "", Slot::Serial("doi"), "doi");
pub const ISBN: Field = field("ISBN", "", Slot::Serial("isbn"), "isbn");
const ISSN: Field = field("ISSN", "", Slot::Serial("issn"), "issn");
pub const ARXIV: Field = field("arXiv ID", "2112.12331", Slot::Serial("arxiv"), "eprint");
pub const VERSION: Field = field("Version", "", Slot::Serial("version"), "version");
pub const URL: Field = wrapped("URL", Slot::Url, "url");
pub const ACCESSED: Field = field("Accessed", "2024-03-15", Slot::Accessed, "urldate");
pub const LANGUAGE: Field = field("Language", "en", Slot::Own("language"), "language");
const ABSTRACT: Field = wrapped("Abstract", Slot::Own("abstract"), "abstract");
const NOTE: Field = wrapped("Note", Slot::Own("note"), "note");

pub const PMID: Slot = Slot::Serial("pmid");
pub const PARENT_TITLE: Slot = Slot::Parent("title");
pub const PARENT_VOLUME: Slot = Slot::Parent("volume");
pub const PARENT_ISSUE: Slot = Slot::Parent("issue");

const ARCHIVE: Slot = Slot::Own("archive");
const GENRE: Slot = Slot::Own("genre");
const ORGANIZATION: Slot = Slot::Own("organization");

const fn container(label: &'static str) -> Field { wrapped(label, PARENT_TITLE, "booktitle") }
const fn genre(hint: &'static str) -> Field { field("Type", hint, GENRE, "type") }
const fn organization(label: &'static str) -> Field { field(label, "", ORGANIZATION, "organization") }
const fn person(label: &'static str) -> Field { field(label, "", AUTHORS.slot, "author") }

/// Hayagriva's names for the types this crate writes.
pub const ARTICLE: &str = "Article";
pub const BOOK: &str = "Book";
pub const CHAPTER: &str = "Chapter";
pub const PERIODICAL: &str = "Periodical";
pub const REPOSITORY: &str = "Repository";
pub const WEB: &str = "Web";
pub const MISC: &str = "Misc";
const PROCEEDINGS: &str = "Proceedings";

/// Each Hayagriva type, the type this table knows it by, and its BibLaTeX
/// type. `@incollection` imports as an anthos in an anthology, which is a Book
/// Section here. A type not listed stands for itself and exports as `misc`.
const TYPE_NAMES: &[(&str, &str, &str)] = &[
    (ARTICLE, ARTICLE, "article"), (BOOK, BOOK, "book"), (CHAPTER, CHAPTER, "inbook"), ("Anthos", CHAPTER, "incollection"),
    ("Anthology", BOOK, "collection"), (PROCEEDINGS, PROCEEDINGS, "proceedings"), ("Conference", PROCEEDINGS, "misc"),
    ("Report", "Report", "report"), ("Thesis", "Thesis", "thesis"), (WEB, WEB, "online"), ("Manuscript", "Manuscript", "unpublished"),
    ("Patent", "Patent", "patent"), (REPOSITORY, REPOSITORY, "software"), ("Entry", "Entry", "inreference"), ("Video", "Video", "video"), ("Audio", "Audio", "audio"),
];

#[derive(Debug)]
pub struct SourceType {
    pub label: &'static str,
    entry: &'static str,
    parent: Option<&'static str>,
    /// The `genre` that marks an entry as this type, when nothing else tells it from its siblings.
    genre: Option<&'static str>,
    pub fields: &'static [Field],
}

macro_rules! kind {
    ($label:literal, $entry:expr, $parent:expr, [$($field:expr),* $(,)?]) => {
        SourceType { label: $label, entry: $entry, parent: $parent, genre: None, fields: &[$($field,)*] }
    };
}

pub static JOURNAL_ARTICLE: SourceType = kind!("Journal Article", ARTICLE, Some(PERIODICAL), [TITLE, AUTHORS, container("Publication"), DATE, VOLUME, ISSUE, PAGES, DOI, ISSN, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]);
pub static PREPRINT: SourceType = SourceType { genre: Some(PREPRINT_GENRE), ..kind!("Preprint", ARTICLE, None, [TITLE, AUTHORS, unexported("Repository", "arXiv", ARCHIVE), DATE, ARXIV, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]) };
pub static OTHER: SourceType = kind!("Other", MISC, None, [TITLE, AUTHORS, DATE, PUBLISHER, PLACE, PAGES, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]);

/// In the order of the type menu. Among types sharing a Hayagriva type, an
/// entry whose parent matches none of them gets the first.
pub static SOURCE_TYPES: &[&SourceType] = &[
    &JOURNAL_ARTICLE,
    &kind!("Book", BOOK, None, [TITLE, AUTHORS, EDITORS, DATE, PUBLISHER, PLACE, EDITION, VOLUME, PAGE_COUNT, ISBN, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Book Section", CHAPTER, Some(BOOK), [TITLE, AUTHORS, container("Book Title"), EDITORS, DATE, PUBLISHER, PLACE, EDITION, VOLUME, PAGES, ISBN, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Conference Paper", ARTICLE, Some(PROCEEDINGS), [TITLE, AUTHORS, container("Proceedings"), DATE, PUBLISHER, PLACE, VOLUME, PAGES, ISBN, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &PREPRINT,
    &kind!("Thesis", "Thesis", None, [TITLE, person("Author"), genre("PhD thesis"), organization("University"), PLACE, DATE, PAGE_COUNT, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Report", "Report", None, [TITLE, AUTHORS, genre("Technical report"), NUMBER, organization("Institution"), PLACE, DATE, PAGE_COUNT, DOI, ISBN, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Web Page", WEB, Some(WEB), [TITLE, AUTHORS, container("Website"), DATE, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Blog Post", ARTICLE, Some("Blog"), [TITLE, AUTHORS, container("Blog"), DATE, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Newspaper Article", ARTICLE, Some("Newspaper"), [TITLE, AUTHORS, container("Newspaper"), DATE, PLACE, PAGES, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Encyclopedia Entry", "Entry", Some("Reference"), [TITLE, AUTHORS, container("Encyclopedia"), EDITORS, DATE, PUBLISHER, PLACE, EDITION, VOLUME, PAGES, ISBN, URL, ACCESSED, LANGUAGE, NOTE]),
    &kind!("Software, Data", REPOSITORY, None, [TITLE, AUTHORS, VERSION, DATE, field("Publisher", "GitHub", Slot::Publisher, "publisher"), DOI, URL, ACCESSED, ABSTRACT, NOTE]),
    &kind!("Video", "Video", None, [TITLE, person("Director"), DATE, PUBLISHER, RUNTIME, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Audio", "Audio", None, [TITLE, AUTHORS, DATE, PUBLISHER, RUNTIME, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &kind!("Patent", "Patent", None, [TITLE, person("Inventors"), field("Patent Number", "", NUMBER.slot, "number"), organization("Assignee"), DATE, URL, ACCESSED, ABSTRACT, NOTE]),
    &kind!("Legal Case", "Case", None, [field("Case Name", "", TITLE.slot, "title"), organization("Court"), DATE, NUMBER, URL, ACCESSED, ABSTRACT, NOTE]),
    &kind!("Legislation", "Legislation", None, [field("Name of Act", "", TITLE.slot, "title"), DATE, NUMBER, PAGES, URL, ACCESSED, ABSTRACT, NOTE]),
    &kind!("Manuscript", "Manuscript", None, [TITLE, AUTHORS, genre(""), organization("Institution"), PLACE, DATE, PAGE_COUNT, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    &OTHER,
];

/// Every field of every type, each slot once.
pub fn all_fields() -> Vec<&'static Field> {
    let mut fields: Vec<&'static Field> = vec![];
    for field in SOURCE_TYPES.iter().flat_map(|kind| kind.fields) {
        if !fields.iter().any(|known| known.slot == field.slot) { fields.push(field); }
    }
    fields
}

fn same(a: &str, b: &str) -> bool { a.eq_ignore_ascii_case(b) }

const PREPRINT_GENRE: &str = "Preprint";

/// The arXiv ID an entry states, or that its link holds.
pub fn arxiv_id(entry: &Mapping) -> Option<String> {
    let stated = read(entry, ARXIV.slot);
    if !stated.is_empty() { return Some(stated); }
    arxiv_of_link(&read(entry, Slot::Url))
}

/// An article with no journal is a preprint only if something says so. Most
/// are journal articles still missing their journal.
fn is_preprint(entry: &Mapping) -> bool {
    arxiv_id(entry).is_some() || !read(entry, ARCHIVE).is_empty() || read(entry, GENRE) == PREPRINT_GENRE
}

/// The type this table knows a Hayagriva type by.
fn known_as(kind: &str) -> &str {
    TYPE_NAMES.iter().find(|(name, ..)| same(name, kind)).map_or(kind, |(_, known, _)| known)
}

/// An entry's BibLaTeX type. An article without a parent may be anything.
pub fn biblatex_type(entry: &Mapping) -> &'static str {
    let kind = read(entry, Slot::Type);
    if same(&kind, ARTICLE) {
        let Some(parent) = parent_of(entry) else { return "misc" };
        return if same(known_as(&text(parent.get("type"))), PROCEEDINGS) { "inproceedings" } else { "article" };
    }
    TYPE_NAMES.iter().find(|(name, ..)| same(name, &kind)).map_or("misc", |(.., biblatex)| biblatex)
}

/// The parent an entry's fields go on. Hayagriva also allows a list of them.
fn parent_of(entry: &Mapping) -> Option<&Mapping> {
    match entry.get("parent")? {
        Value::Sequence(parents) => parents.first()?.as_mapping(),
        parent => parent.as_mapping(),
    }
}

fn parent_of_mut(entry: &mut Mapping) -> Option<&mut Mapping> {
    match entry.get_mut("parent")? {
        Value::Sequence(parents) => parents.first_mut()?.as_mapping_mut(),
        parent => parent.as_mapping_mut(),
    }
}

/// The entry's parent, made if it has none.
fn ensure_parent(entry: &mut Mapping) -> Option<&mut Mapping> {
    if parent_of_mut(entry).is_none() { entry.insert("parent".into(), Value::Mapping(Mapping::new())); }
    parent_of_mut(entry)
}

impl SourceType {
    /// The type of a raw entry. Any Hayagriva type this table lacks is Other.
    pub fn of(entry: &Mapping) -> &'static SourceType {
        let kind = read(entry, Slot::Type);
        let kind = known_as(&kind);
        let parent = parent_of(entry).and_then(|p| p.get("type")).and_then(Value::as_str).map(known_as);
        let candidates = || SOURCE_TYPES.iter().copied().filter(|t| same(t.entry, kind));
        candidates().find(|t| match (t.parent, parent) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => t.genre.is_none() || is_preprint(entry),
            _ => false,
        }).or_else(|| candidates().next()).unwrap_or(&OTHER)
    }

    /// Whether changing `entry` to this type would lose what its parent holds.
    pub fn drops_parent(&self, entry: &Mapping) -> bool {
        self.parent.is_none() && parent_of(entry).is_some_and(|parent| parent.keys().any(|name| name.as_str() != Some("type")))
    }

    /// Makes `entry` this type. A parent this type has no use for goes with
    /// the old type, or it would be cited but could not be edited.
    pub fn apply(&self, entry: &mut Mapping) {
        write(entry, Slot::Type, self.entry);
        if read(entry, GENRE) == PREPRINT_GENRE { write(entry, GENRE, ""); }
        if let Some(genre) = self.genre { write(entry, GENRE, genre); }
        match self.parent {
            Some(kind) => write(entry, Slot::ParentType, kind),
            None => { entry.remove("parent"); }
        }
    }
}

/// The publisher, which a chapter's import leaves on its book.
fn publisher_of(entry: &Mapping) -> Option<&Value> {
    entry.get("publisher").or_else(|| parent_of(entry)?.get("publisher"))
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn text(value: Option<&Value>) -> String {
    value.and_then(scalar).unwrap_or_default()
}

fn person_name(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.clone()),
        Value::Mapping(map) => {
            let name = map.get("name")?.as_str()?;
            Some(map.get("given-name").and_then(Value::as_str).map_or(name.to_string(), |given| format!("{name}, {given}")))
        }
        _ => None,
    }
}

/// The names in a `Persons` slot, each `Family, Given`.
pub fn persons(entry: &Mapping, slot: Slot) -> Vec<String> {
    let Slot::Persons(name) = slot else { return vec![] };
    match entry.get(name) {
        Some(Value::Sequence(list)) => list.iter().filter_map(person_name).collect(),
        Some(one) => person_name(one).into_iter().collect(),
        None => vec![],
    }
}

/// The surname of an entry's first author.
pub fn first_surname(entry: &Mapping) -> Option<String> {
    let first = persons(entry, AUTHORS.slot).into_iter().next()?;
    first.split(',').next()?.split_whitespace().next_back().map(str::to_string)
}

/// The year of an entry's date.
pub fn year(entry: &Mapping) -> String {
    match entry.get("date") {
        Some(Value::String(date)) => date.chars().take(4).collect(),
        other => text(other),
    }
}

/// An entry's tags, which only Thesio reads. A hand-written `tags: reading` counts as one.
pub fn tags(entry: &Mapping) -> Vec<String> {
    match entry.get("tags") {
        Some(Value::Sequence(tags)) => tags.iter().filter_map(scalar).collect(),
        Some(tag) => scalar(tag).into_iter().collect(),
        None => vec![],
    }
}

pub fn set_tags(entry: &mut Mapping, tags: Vec<String>) {
    set(entry, "tags", (!tags.is_empty()).then(|| Value::Sequence(tags.into_iter().map(Value::from).collect())));
}

/// The identifiers that name one work, as (kind, number), lowercased for
/// comparing. An ISSN or a version number is shared by many. YAML reads a bare
/// arXiv ID as a number.
pub fn work_ids(entry: &Mapping) -> Vec<(String, String)> {
    entry.get("serial-number").and_then(Value::as_mapping).map_or(vec![], |serial| serial.iter().filter_map(|(kind, value)| {
        let kind = kind.as_str()?.to_lowercase();
        WORK_ID_KINDS.contains(&kind.as_str()).then_some(())?;
        Some((kind, scalar(value).filter(|number| !number.is_empty())?.to_lowercase()))
    }).collect())
}

/// A field's value as the editor shows it.
pub fn read(entry: &Mapping, slot: Slot) -> String {
    match slot {
        Slot::Type => text(entry.get("type")),
        Slot::ParentType => text(parent_of(entry).and_then(|p| p.get("type"))),
        Slot::Own(name) => text(entry.get(name)),
        Slot::Parent(name) => text(parent_of(entry).and_then(|p| p.get(name))),
        Slot::Either(name) => text(entry.get(name).or_else(|| parent_of(entry)?.get(name))),
        Slot::Serial(kind) => match entry.get("serial-number") {
            Some(Value::Mapping(serial)) => text(serial.get(kind)),
            other if kind == PLAIN_SERIAL => text(other),
            _ => String::new(),
        },
        Slot::Persons(_) => persons(entry, slot).join("; "),
        Slot::Date => text(entry.get("date")),
        Slot::Publisher => match publisher_of(entry) {
            Some(Value::Mapping(publisher)) => text(publisher.get("name")),
            other => text(other),
        },
        Slot::Place => text(publisher_of(entry).and_then(|publisher| publisher.get("location")).or_else(|| entry.get("location"))),
        Slot::Url => match entry.get("url") {
            Some(Value::Mapping(url)) => text(url.get("value")),
            other => text(other),
        },
        Slot::Accessed => text(entry.get("url").and_then(|url| url.get("date"))),
    }
}

fn set(map: &mut Mapping, name: &str, value: Option<Value>) {
    match value {
        Some(value) => { map.insert(name.into(), value); }
        None => { map.remove(name); }
    }
}

fn value_of(text: &str) -> Option<Value> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.into())
}

fn set_url(entry: &mut Mapping, url: &str, accessed: &str) {
    let value = value_of(url).map(|url| match value_of(accessed) {
        Some(date) => {
            let mut map = Mapping::new();
            map.insert("value".into(), url);
            map.insert("date".into(), date);
            Value::Mapping(map)
        }
        None => url,
    });
    set(entry, "url", value);
}

/// Writes one field. Validation is left to the commit, which runs Hayagriva.
pub fn write(entry: &mut Mapping, slot: Slot, new: &str) {
    match slot {
        Slot::Type => set(entry, "type", value_of(new)),
        Slot::ParentType => if let Some(parent) = ensure_parent(entry) { set(parent, "type", value_of(new)); },
        Slot::Own(name) => set(entry, name, value_of(new)),
        Slot::Parent(name) => if let Some(parent) = ensure_parent(entry) { set(parent, name, value_of(new)); },
        Slot::Either(name) => match parent_of_mut(entry).filter(|parent| parent.contains_key(name)) {
            Some(parent) => set(parent, name, value_of(new)),
            None => set(entry, name, value_of(new)),
        },
        Slot::Serial(kind) => {
            let mut serial = match entry.get("serial-number") {
                Some(Value::Mapping(serial)) => serial.clone(),
                Some(other) => Mapping::from_iter([(PLAIN_SERIAL.into(), other.clone())]),
                None => Mapping::new(),
            };
            set(&mut serial, kind, value_of(new));
            set(entry, "serial-number", (!serial.is_empty()).then_some(Value::Mapping(serial)));
        }
        Slot::Persons(name) => {
            let persons: Vec<Value> = new.split(';').filter_map(value_of).collect();
            set(entry, name, (!persons.is_empty()).then_some(Value::Sequence(persons)));
        }
        // A bare year is a number in Hayagriva files.
        Slot::Date => set(entry, "date", value_of(new).map(|date| date.as_str().and_then(|d| d.parse::<i64>().ok()).map_or(date.clone(), Value::from))),
        Slot::Publisher | Slot::Place if !entry.contains_key("publisher") && parent_of(entry).is_some_and(|parent| parent.contains_key("publisher")) => {
            if let Some(parent) = parent_of_mut(entry) { write(parent, slot, new); }
        }
        Slot::Publisher => match entry.get_mut("publisher").and_then(Value::as_mapping_mut) {
            Some(publisher) if value_of(new).is_some() => set(publisher, "name", value_of(new)),
            _ => set(entry, "publisher", value_of(new)),
        },
        Slot::Place => match entry.get_mut("publisher") {
            Some(Value::Mapping(publisher)) => set(publisher, "location", value_of(new)),
            Some(name) if value_of(new).is_some() => {
                *name = Value::Mapping(Mapping::from_iter([("name".into(), name.clone()), ("location".into(), new.trim().into())]));
            }
            _ => set(entry, "location", value_of(new)),
        },
        Slot::Url => {
            let accessed = read(entry, Slot::Accessed);
            set_url(entry, new, &accessed);
        }
        Slot::Accessed => {
            let url = read(entry, Slot::Url);
            set_url(entry, &url, new);
        }
    }
}
