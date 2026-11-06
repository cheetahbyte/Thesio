//! BibLaTeX import and export. Hayagriva does the parsing; what it drops, and
//! every field name, comes from the table in `source_type.rs`.

use crate::paths::natural_cmp;
use crate::source_type::{self, ACCESSED, ARTICLE, ARXIV, EPRINT_TYPE, Field, KEYWORDS, LANGUAGE, MISC, REPOSITORY, Slot, TITLE, VERSION, WEB, biblatex_type, read, write};
use anyhow::{Context, Result};
use biblatex::ChunksExt;
use hayagriva::types::Date;
use serde_yaml::{Mapping, Value};

/// Hayagriva entries from BibLaTeX text.
fn parse(text: &str) -> Result<Mapping> {
    let library = hayagriva::io::from_biblatex_str(text).map_err(|errors| anyhow::anyhow!(errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")))?;
    Ok(serde_yaml::from_str(&hayagriva::io::to_yaml_str(&library)?)?)
}

/// A Hayagriva entry from one BibTeX record, as a web service sends it.
pub fn record(text: &str) -> Result<Mapping> {
    // Services use keys such as URLs that BibTeX parsers reject, and the key
    // is replaced anyway.
    let open = text.find('{').context("the service returned no record")?;
    let comma = open + text[open..].find(',').context("the service returned no record")?;
    let record = format!("{}{{entry{}", &text[..open], &text[comma..]);
    parse(&record)?.into_iter().next().and_then(|(_, entry)| entry.as_mapping().cloned()).context("the service returned no record")
}

/// Gives an imported entry a better type than BibLaTeX could: `@misc` holds
/// preprints and web pages alike, and Hayagriva reads `@software` as misc too.
fn refine(entry: &mut Mapping) {
    if !read(entry, Slot::Type).eq_ignore_ascii_case(MISC) { return; }
    let kind = if !read(entry, ARXIV.slot).is_empty() { ARTICLE }
        else if !read(entry, VERSION.slot).is_empty() { REPOSITORY }
        else if !read(entry, ACCESSED.slot).is_empty() { WEB }
        else { return };
    write(entry, Slot::Type, kind);
}

/// Entries by key from BibLaTeX text, with the fields Hayagriva has no place for.
pub fn import(text: &str) -> Result<Vec<(Value, Mapping)>> {
    let incoming = parse(text)?;
    // It read the same text, so this parses.
    let raw = biblatex::Bibliography::parse(text).ok();
    let verbatim = |key: &str, field: &str| raw.as_ref()?.get(key)?.get(field).map(|chunks| chunks.format_verbatim());
    let mut entries = vec![];
    for (key, mut value) in incoming {
        let Some(map) = value.as_mapping_mut() else { continue };
        refine(map);
        let name = key.as_str().unwrap_or_default();
        let keywords = verbatim(name, KEYWORDS).unwrap_or_default();
        let tags: Vec<String> = keywords.split([',', ';']).map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_string).collect();
        source_type::set_tags(map, tags);
        // `langid` names a language (`german`); only codes are valid here.
        let language = LANGUAGE.biblatex.and_then(|field| verbatim(name, field)).filter(|code| code.len() == 2 && code.chars().all(|c| c.is_ascii_lowercase()));
        if let Some(code) = language { write(map, LANGUAGE.slot, &code); }
        entries.push((key, map.clone()));
    }
    Ok(entries)
}

fn date_text(date: &Date) -> String {
    let mut text = format!("{:04}", date.year);
    if let Some(month) = date.month {
        text += &format!("-{:02}", month + 1);
        if let Some(day) = date.day { text += &format!("-{:02}", day + 1); }
    }
    text
}

/// What BibLaTeX gets for `field`. Hayagriva normalises titles and dates.
fn value(field: &Field, entry: &hayagriva::Entry, raw: &Mapping) -> String {
    match field.slot {
        slot if slot == TITLE.slot => entry.title().map(ToString::to_string).unwrap_or_default(),
        source_type::PARENT_TITLE => entry.parents().first().and_then(|parent| parent.title()).map(ToString::to_string).unwrap_or_default(),
        Slot::Date => entry.date_any().map(date_text).unwrap_or_default(),
        Slot::Persons(_) => source_type::persons(raw, field.slot).join(" and "),
        slot => read(raw, slot),
    }
}

fn export_entry(entry: &hayagriva::Entry, raw: &Mapping, schema: &[&'static Field]) -> String {
    let kind = biblatex_type(raw);
    let mut fields: Vec<(&str, String)> = vec![];
    for field in schema {
        let Some(name) = field.biblatex_name(kind) else { continue };
        let value = value(field, entry, raw);
        if value.is_empty() { continue; }
        // A serial number is the entry's own number, ahead of the issue it may share a name with.
        match fields.iter_mut().find(|(known, _)| *known == name) {
            Some(known) => if matches!(field.slot, Slot::Serial(_)) { known.1 = value; },
            None => fields.push((name, value)),
        }
    }
    if !read(raw, ARXIV.slot).is_empty() { fields.push((EPRINT_TYPE.0, EPRINT_TYPE.1.to_string())); }
    fields.push((KEYWORDS, source_type::tags(raw).join(", ")));
    let escape = |text: &str| text.replace('{', "\\{").replace('}', "\\}");
    let body = fields.iter().filter(|(_, value)| !value.is_empty()).map(|(name, value)| format!("  {name} = {{{}}},", escape(value))).collect::<Vec<_>>().join("\n");
    format!("@{kind}{{{},\n{body}\n}}\n", entry.key())
}

/// A whole library as BibLaTeX. Fields BibLaTeX has no place for, such as a
/// running time, are left out.
pub fn export(parsed: &hayagriva::Library, raw: &Mapping) -> String {
    let schema = source_type::all_fields();
    let empty = Mapping::new();
    let mut rows: Vec<&hayagriva::Entry> = parsed.iter().collect();
    rows.sort_by(|a, b| natural_cmp(a.key(), b.key()));
    rows.into_iter().map(|entry| export_entry(entry, raw.get(entry.key()).and_then(Value::as_mapping).unwrap_or(&empty), &schema)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exported(yaml: &str) -> String {
        let raw: Mapping = serde_yaml::from_str(yaml).unwrap();
        export(&hayagriva::io::from_yaml_str(yaml).unwrap(), &raw)
    }

    #[test]
    fn every_field_with_a_name_is_exported() {
        let text = exported("a:\n  type: Video\n  title: T\n  genre: Film\n  organization: Org\n  page-total: 3\n  serial-number:\n    serial: 7\n    version: 2\n");
        for line in ["title = {T}", "type = {Film}", "organization = {Org}", "pagetotal = {3}", "number = {7}", "version = {2}"] { assert!(text.contains(line), "{line} in {text}"); }
    }

    #[test]
    fn a_number_is_the_entrys_own_before_its_issue() {
        assert!(exported("a:\n  type: Report\n  title: T\n  issue: 4\n  serial-number:\n    serial: 7\n").contains("number = {7}"));
        assert!(exported("a:\n  type: Article\n  title: T\n  parent:\n    type: Periodical\n    title: J\n    issue: 4\n").contains("number = {4}"));
    }

    #[test]
    fn organizations_and_parents_take_the_name_their_type_uses() {
        let thesis = exported("a:\n  type: Thesis\n  title: T\n  organization: TU\n");
        assert!(thesis.starts_with("@thesis{a,") && thesis.contains("institution = {TU}"), "{thesis}");
        let journal = exported("a:\n  type: Article\n  title: T\n  parent:\n    type: Periodical\n    title: J\n");
        assert!(journal.starts_with("@article{a,") && journal.contains("journaltitle = {J}"), "{journal}");
        let paper = exported("a:\n  type: Article\n  title: T\n  parent:\n    type: Proceedings\n    title: P\n");
        assert!(paper.starts_with("@inproceedings{a,") && paper.contains("booktitle = {P}"), "{paper}");
    }
}
