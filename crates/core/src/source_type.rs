//! The kinds of source the Library knows, and which fields each one has. A kind
//! is a Hayagriva type plus the type of its parent, so `library.yaml` stays
//! plain Hayagriva and entries from any tool get a kind without a new field.

use serde_yaml::{Mapping, Value};

/// Where a field's value lives in an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
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
}

const fn field(label: &'static str, hint: &'static str, slot: Slot) -> Field { Field { label, hint, long: false, slot } }
const fn wrapped(label: &'static str, slot: Slot) -> Field { Field { label, hint: "", long: true, slot } }

const TITLE: Field = wrapped("Title", Slot::Own("title"));
const AUTHORS: Field = wrapped("Authors", Slot::Persons("author"));
const EDITORS: Field = wrapped("Editors", Slot::Persons("editor"));
const DATE: Field = field("Date", "2024 or 2024-03-15", Slot::Date);
const PUBLISHER: Field = field("Publisher", "", Slot::Publisher);
const PLACE: Field = field("Place", "", Slot::Place);
const VOLUME: Field = field("Volume", "", Slot::Either("volume"));
const ISSUE: Field = field("Issue", "", Slot::Either("issue"));
const EDITION: Field = field("Edition", "", Slot::Own("edition"));
const PAGES: Field = field("Pages", "891-921", Slot::Own("page-range"));
const PAGE_COUNT: Field = field("Pages", "", Slot::Own("page-total"));
const NUMBER: Field = field("Number", "", Slot::Serial("serial"));
const RUNTIME: Field = field("Running Time", "01:30:00", Slot::Own("runtime"));
const DOI: Field = field("DOI", "", Slot::Serial("doi"));
const ISBN: Field = field("ISBN", "", Slot::Serial("isbn"));
const ISSN: Field = field("ISSN", "", Slot::Serial("issn"));
const URL: Field = wrapped("URL", Slot::Url);
const ACCESSED: Field = field("Accessed", "2024-03-15", Slot::Accessed);
const LANGUAGE: Field = field("Language", "en", Slot::Own("language"));
const ABSTRACT: Field = wrapped("Abstract", Slot::Own("abstract"));
const NOTE: Field = wrapped("Note", Slot::Own("note"));

const fn container(label: &'static str) -> Field { wrapped(label, Slot::Parent("title")) }

#[derive(Debug)]
pub struct SourceType {
    pub label: &'static str,
    entry: &'static str,
    parent: Option<&'static str>,
    pub fields: &'static [Field],
}

macro_rules! kind {
    ($label:literal, $entry:literal, $parent:expr, [$($field:expr),* $(,)?]) => {
        SourceType { label: $label, entry: $entry, parent: $parent, fields: &[$($field,)*] }
    };
}

/// In the order of the type menu. Among types sharing a Hayagriva type, an
/// entry whose parent matches none of them gets the first.
pub static SOURCE_TYPES: &[SourceType] = &[
    kind!("Journal Article", "Article", Some("Periodical"), [TITLE, AUTHORS, container("Publication"), DATE, VOLUME, ISSUE, PAGES, DOI, ISSN, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Book", "Book", None, [TITLE, AUTHORS, EDITORS, DATE, PUBLISHER, PLACE, EDITION, VOLUME, PAGE_COUNT, ISBN, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Book Section", "Chapter", Some("Book"), [TITLE, AUTHORS, container("Book Title"), EDITORS, DATE, PUBLISHER, PLACE, EDITION, VOLUME, PAGES, ISBN, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Conference Paper", "Article", Some("Proceedings"), [TITLE, AUTHORS, container("Proceedings"), DATE, PUBLISHER, PLACE, VOLUME, PAGES, ISBN, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Preprint", "Article", None, [TITLE, AUTHORS, field("Repository", "arXiv", Slot::Own("archive")), DATE, field("arXiv ID", "2112.12331", Slot::Serial("arxiv")), DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Thesis", "Thesis", None, [TITLE, field("Author", "", Slot::Persons("author")), field("Type", "PhD thesis", Slot::Own("genre")), field("University", "", Slot::Own("organization")), PLACE, DATE, PAGE_COUNT, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Report", "Report", None, [TITLE, AUTHORS, field("Type", "Technical report", Slot::Own("genre")), NUMBER, field("Institution", "", Slot::Own("organization")), PLACE, DATE, PAGE_COUNT, DOI, ISBN, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Web Page", "Web", Some("Web"), [TITLE, AUTHORS, container("Website"), DATE, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Blog Post", "Article", Some("Blog"), [TITLE, AUTHORS, container("Blog"), DATE, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Newspaper Article", "Article", Some("Newspaper"), [TITLE, AUTHORS, container("Newspaper"), DATE, PLACE, PAGES, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Encyclopedia Entry", "Entry", Some("Reference"), [TITLE, AUTHORS, container("Encyclopedia"), EDITORS, DATE, PUBLISHER, PLACE, EDITION, VOLUME, PAGES, ISBN, URL, ACCESSED, LANGUAGE, NOTE]),
    kind!("Software, Data", "Repository", None, [TITLE, AUTHORS, field("Version", "", Slot::Serial("version")), DATE, field("Publisher", "GitHub", Slot::Publisher), DOI, URL, ACCESSED, ABSTRACT, NOTE]),
    kind!("Video", "Video", None, [TITLE, field("Director", "", Slot::Persons("author")), DATE, PUBLISHER, RUNTIME, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Audio", "Audio", None, [TITLE, AUTHORS, DATE, PUBLISHER, RUNTIME, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Patent", "Patent", None, [TITLE, field("Inventors", "", Slot::Persons("author")), field("Patent Number", "", Slot::Serial("serial")), field("Assignee", "", Slot::Own("organization")), DATE, URL, ACCESSED, ABSTRACT, NOTE]),
    kind!("Legal Case", "Case", None, [field("Case Name", "", Slot::Own("title")), field("Court", "", Slot::Own("organization")), DATE, NUMBER, URL, ACCESSED, ABSTRACT, NOTE]),
    kind!("Legislation", "Legislation", None, [field("Name of Act", "", Slot::Own("title")), DATE, NUMBER, PAGES, URL, ACCESSED, ABSTRACT, NOTE]),
    kind!("Manuscript", "Manuscript", None, [TITLE, AUTHORS, field("Type", "", Slot::Own("genre")), field("Institution", "", Slot::Own("organization")), PLACE, DATE, PAGE_COUNT, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
    kind!("Other", "Misc", None, [TITLE, AUTHORS, DATE, PUBLISHER, PLACE, PAGES, DOI, URL, ACCESSED, LANGUAGE, ABSTRACT, NOTE]),
];

fn same(a: &str, b: &str) -> bool { a.eq_ignore_ascii_case(b) }

const PREPRINT: &str = "Preprint";

/// An article with no journal is a preprint only if something says so. Most
/// are journal articles still missing their journal.
fn is_preprint(entry: &Value) -> bool {
    !read(entry, Slot::Serial("arxiv")).is_empty() || !read(entry, Slot::Own("archive")).is_empty() || read(entry, Slot::Own("genre")) == PREPRINT
        || matches!(crate::lookup::identify(&read(entry, Slot::Url)), Some(crate::lookup::Identifier::Arxiv(_)))
}

/// The type this table knows a Hayagriva type by. `@incollection` imports as
/// an anthos in an anthology, which is a Book Section here.
fn alias(kind: &str) -> &str {
    match kind.to_ascii_lowercase().as_str() {
        "anthos" => "Chapter",
        "anthology" => "Book",
        "conference" => "Proceedings",
        _ => kind,
    }
}

/// Gives an imported entry a better type than BibLaTeX could: `@misc` holds
/// preprints and web pages alike, and Hayagriva reads `@software` as misc too.
pub fn refine_imported(entry: &mut Mapping) {
    let value = Value::Mapping(entry.clone());
    if !value.get("type").and_then(Value::as_str).is_some_and(|kind| kind.eq_ignore_ascii_case("misc")) { return; }
    let kind = if !read(&value, Slot::Serial("arxiv")).is_empty() { "Article" }
        else if !read(&value, Slot::Serial("version")).is_empty() { "Repository" }
        else if !read(&value, Slot::Accessed).is_empty() { "Web" }
        else { return };
    entry.insert("type".into(), kind.into());
}

/// The parent an entry's fields go on. Hayagriva also allows a list of them.
fn parent_of(entry: &Value) -> Option<&Value> {
    match entry.get("parent")? {
        Value::Sequence(parents) => parents.first(),
        parent => Some(parent),
    }
}

fn parent_of_mut(entry: &mut Mapping) -> Option<&mut Mapping> {
    match entry.get_mut("parent")? {
        Value::Sequence(parents) => parents.first_mut()?.as_mapping_mut(),
        parent => parent.as_mapping_mut(),
    }
}

impl SourceType {
    /// The type of a raw entry. Any Hayagriva type this table lacks is Other.
    pub fn of(entry: &Value) -> &'static SourceType {
        let kind = alias(entry.get("type").and_then(Value::as_str).unwrap_or_default());
        let parent = parent_of(entry).and_then(|p| p.get("type")).and_then(Value::as_str).map(alias);
        let candidates = || SOURCE_TYPES.iter().filter(|t| same(t.entry, kind));
        candidates().find(|t| match (t.parent, parent) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => t.label != PREPRINT || is_preprint(entry),
            _ => false,
        }).or_else(|| candidates().next()).unwrap_or_else(|| SOURCE_TYPES.last().expect("the table has entries"))
    }

    /// Whether changing `entry` to this type would lose what its parent holds.
    pub fn drops_parent(&self, entry: &Value) -> bool {
        self.parent.is_none() && parent_of(entry).and_then(Value::as_mapping).is_some_and(|parent| parent.keys().any(|name| name.as_str() != Some("type")))
    }

    /// Makes `entry` this type. A parent this type has no use for goes with
    /// the old type, or it would be cited but could not be edited.
    pub fn apply(&self, entry: &mut Mapping) {
        entry.insert("type".into(), self.entry.into());
        if text(entry.get("genre")) == PREPRINT { entry.remove("genre"); }
        if self.label == PREPRINT { entry.insert("genre".into(), PREPRINT.into()); }
        match self.parent {
            Some(kind) => {
                if parent_of_mut(entry).is_none() { entry.insert("parent".into(), Value::Mapping(Mapping::new())); }
                if let Some(parent) = parent_of_mut(entry) { parent.insert("type".into(), kind.into()); }
            }
            None => { entry.remove("parent"); }
        }
    }
}

/// The publisher, which a chapter's import leaves on its book.
fn publisher_of(entry: &Value) -> Option<&Value> {
    entry.get("publisher").or_else(|| parent_of(entry)?.get("publisher"))
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn person(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.clone()),
        Value::Mapping(map) => {
            let name = map.get("name")?.as_str()?;
            Some(map.get("given-name").and_then(Value::as_str).map_or(name.to_string(), |given| format!("{name}, {given}")))
        }
        _ => None,
    }
}

/// A field's value as the editor shows it.
pub fn read(entry: &Value, slot: Slot) -> String {
    match slot {
        Slot::Own(name) => text(entry.get(name)),
        Slot::Parent(name) => text(parent_of(entry).and_then(|p| p.get(name))),
        Slot::Either(name) => text(entry.get(name).or_else(|| parent_of(entry)?.get(name))),
        Slot::Serial(kind) => match entry.get("serial-number") {
            Some(Value::Mapping(serial)) => text(serial.get(kind)),
            other if kind == "serial" => text(other),
            _ => String::new(),
        },
        Slot::Persons(name) => match entry.get(name) {
            Some(Value::Sequence(list)) => list.iter().filter_map(person).collect::<Vec<_>>().join("; "),
            Some(one) => person(one).unwrap_or_default(),
            None => String::new(),
        },
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
        Slot::Own(name) => set(entry, name, value_of(new)),
        Slot::Parent(name) => {
            if parent_of_mut(entry).is_none() { entry.insert("parent".into(), Value::Mapping(Mapping::new())); }
            if let Some(parent) = parent_of_mut(entry) { set(parent, name, value_of(new)); }
        }
        Slot::Either(name) => match parent_of_mut(entry).filter(|parent| parent.contains_key(name)) {
            Some(parent) => set(parent, name, value_of(new)),
            None => set(entry, name, value_of(new)),
        },
        Slot::Serial(kind) => {
            let mut serial = match entry.get("serial-number") {
                Some(Value::Mapping(serial)) => serial.clone(),
                Some(other) => Mapping::from_iter([("serial".into(), other.clone())]),
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
        Slot::Publisher | Slot::Place if !entry.contains_key("publisher") && parent_of_mut(entry).is_some_and(|parent| parent.contains_key("publisher")) => {
            if let Some(parent) = parent_of_mut(entry) { write(parent, slot, new); }
        }
        Slot::Publisher => match entry.get_mut("publisher").and_then(Value::as_mapping_mut) {
            Some(publisher) if value_of(new).is_some() => set(publisher, "name", value_of(new)),
            _ => set(entry, "publisher", value_of(new)),
        },
        Slot::Place => match entry.get("publisher") {
            Some(Value::Mapping(publisher)) => {
                let mut publisher = publisher.clone();
                set(&mut publisher, "location", value_of(new));
                entry.insert("publisher".into(), Value::Mapping(publisher));
            }
            Some(name) if value_of(new).is_some() => {
                let publisher = Mapping::from_iter([("name".into(), name.clone()), ("location".into(), new.trim().into())]);
                entry.insert("publisher".into(), Value::Mapping(publisher));
            }
            _ => set(entry, "location", value_of(new)),
        },
        Slot::Url => {
            let accessed = read(&Value::Mapping(entry.clone()), Slot::Accessed);
            set_url(entry, new, &accessed);
        }
        Slot::Accessed => {
            let url = read(&Value::Mapping(entry.clone()), Slot::Url);
            set_url(entry, &url, new);
        }
    }
}
