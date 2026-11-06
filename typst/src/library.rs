//! The citation library as Hayagriva reads it: whether `library.yaml` parses,
//! its entries as Typst sees them, and BibLaTeX read into Hayagriva YAML.

use anyhow::Result;
use biblatex::ChunksExt;
use hayagriva::types::Date;
use serde::Serialize;
use serde_yaml::Mapping;
use std::collections::BTreeMap;

/// BibLaTeX fields Hayagriva drops, read as written.
const VERBATIM: [&str; 2] = ["keywords", "language"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParsedEntry {
    pub key: String,
    pub title: String,
    /// "Family, Given", or the family name alone.
    pub authors: Vec<String>,
    /// YYYY[-MM[-DD]], empty without a date.
    pub date: String,
    pub parent_title: String,
}

/// The library Typst would read from `text`. An empty file holds no entries.
fn parse(text: &str) -> Result<hayagriva::Library> {
    let entries = serde_yaml::from_str::<Option<Mapping>>(text)?.unwrap_or_default();
    if entries.is_empty() { return Ok(hayagriva::Library::new()); }
    Ok(hayagriva::io::from_yaml_str(text)?)
}

pub fn check(text: &str) -> Result<()> { parse(text).map(|_| ()) }

fn date_text(date: &Date) -> String {
    let mut text = format!("{:04}", date.year);
    if let Some(month) = date.month {
        text += &format!("-{:02}", month + 1);
        if let Some(day) = date.day { text += &format!("-{:02}", day + 1); }
    }
    text
}

pub fn entries(text: &str) -> Result<Vec<ParsedEntry>> {
    Ok(parse(text)?.iter().map(|entry| ParsedEntry {
        key: entry.key().to_string(),
        title: entry.title().map(ToString::to_string).unwrap_or_default(),
        authors: entry.authors().unwrap_or_default().iter().map(|person| match &person.given_name {
            Some(given) => format!("{}, {given}", person.name),
            None => person.name.clone(),
        }).collect(),
        date: entry.date_any().map(date_text).unwrap_or_default(),
        parent_title: entry.parents().first().and_then(|parent| parent.title()).map(ToString::to_string).unwrap_or_default(),
    }).collect())
}

/// BibLaTeX as a Hayagriva library, and by key the fields of `VERBATIM` it has.
pub fn import_biblatex(text: &str) -> Result<(String, BTreeMap<String, BTreeMap<String, String>>)> {
    let library = hayagriva::io::from_biblatex_str(text).map_err(|errors| anyhow::anyhow!(errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")))?;
    let yaml = hayagriva::io::to_yaml_str(&library)?;
    // Hayagriva read the same text, so this parses.
    let raw = biblatex::Bibliography::parse(text).ok();
    let mut verbatim = BTreeMap::new();
    for entry in library.iter() {
        let Some(found) = raw.as_ref().and_then(|raw| raw.get(entry.key())) else { continue };
        let fields: BTreeMap<String, String> = VERBATIM.iter().filter_map(|field| Some((field.to_string(), found.get(field)?.format_verbatim()))).collect();
        if !fields.is_empty() { verbatim.insert(entry.key().to_string(), fields); }
    }
    Ok((yaml, verbatim))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZOTERO: &str = r#"
@article{parry2022, title = {Flaky}, author = {Parry, Owain}, journal = {TOSEM}, volume = {31}, number = {1}, pages = {1--74},
  year = {2022}, doi = {10.1145/3476105}, issn = {1049-331X}, language = {en}, keywords = {testing, flaky tests}}
@book{knuth1984, title = {TeXbook}, author = {Knuth, Donald}, editor = {Doe, Jane}, publisher = {AW}, address = {Reading},
  edition = {2}, isbn = {0-201-13447-0}, year = {1984}, note = {A note}}
@incollection{chap2020, title = {Chapter}, booktitle = {Big Book}, publisher = {Pub}, author = {Doe, A.}, year = {2020}}
"#;

    #[test]
    fn an_empty_library_parses_and_a_broken_one_does_not() {
        assert!(check("").is_ok() && check("# nothing yet\n").is_ok());
        assert!(check("a: [unclosed\n").is_err());
        assert!(check("a:\n  type: Book\n  date: not a date\n").is_err());
    }

    #[test]
    fn entries_read_as_typst_reads_them() {
        let found = entries("a:\n  type: Article\n  title: T\n  author: [\"Doe, Jane\", Plato]\n  date: 2020-03\n  parent:\n    type: Periodical\n    title: J\n").unwrap();
        assert_eq!(found, [ParsedEntry { key: "a".into(), title: "T".into(), authors: vec!["Doe, Jane".into(), "Plato".into()], date: "2020-03".into(), parent_title: "J".into() }]);
    }

    #[test]
    fn import_keeps_what_hayagriva_drops() {
        let (yaml, verbatim) = import_biblatex(ZOTERO).unwrap();
        assert_eq!(entries(&yaml).unwrap().len(), 3);
        assert_eq!(verbatim["parry2022"]["keywords"], "testing, flaky tests");
        assert_eq!(verbatim["parry2022"]["language"], "en");
        assert!(!verbatim.contains_key("knuth1984"));
    }
}
