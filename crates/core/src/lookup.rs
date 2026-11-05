//! Looks up bibliographic metadata by identifier: DOIs and arXiv IDs through
//! doi.org, PMIDs through NCBI, ISBNs through Open Library. Also downloads
//! open-access papers from arXiv and the locations Unpaywall lists. Network
//! access happens only here, and only for sources the user added to the library.

use anyhow::{Context, Result};
use serde_json::Value as Json;
use serde_yaml::{Mapping, Value};
use std::path::Path;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str = "Thesio/0.1";
const PAPER_TIMEOUT: Duration = Duration::from_secs(120);
const PAPER_LIMIT: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identifier {
    Doi(String),
    Arxiv(String),
    Pmid(String),
    Isbn(String),
}

impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Doi(id) => write!(f, "DOI {id}"),
            Self::Arxiv(id) => write!(f, "arXiv {id}"),
            Self::Pmid(id) => write!(f, "PMID {id}"),
            Self::Isbn(id) => write!(f, "ISBN {id}"),
        }
    }
}

/// The ISBN digits of a token, when its check digit is right.
fn isbn(token: &str) -> Option<String> {
    let digits: String = token.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_uppercase();
    let valid = match digits.len() {
        10 if digits[..9].bytes().all(|b| b.is_ascii_digit()) => {
            let total: u32 = digits.chars().enumerate().map(|(i, c)| (10 - i as u32) * if c == 'X' { 10 } else { c.to_digit(10).unwrap_or(99) }).sum();
            total.is_multiple_of(11)
        }
        13 if digits.bytes().all(|b| b.is_ascii_digit()) => {
            let total: u32 = digits.chars().enumerate().map(|(i, c)| c.to_digit(10).unwrap_or(0) * if i % 2 == 0 { 1 } else { 3 }).sum();
            total.is_multiple_of(10)
        }
        _ => false,
    };
    valid.then_some(digits)
}

/// Whether a token has the shape of an arXiv ID, with or without a version.
fn arxiv_id(token: &str) -> Option<String> {
    let bare = token.split_once('v').filter(|(_, v)| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).map_or(token, |(id, _)| id);
    let modern = bare.split_once('.').is_some_and(|(month, serial)| {
        month.len() == 4 && matches!(serial.len(), 4 | 5) && month.bytes().chain(serial.bytes()).all(|b| b.is_ascii_digit())
    });
    let legacy = bare.split_once('/').is_some_and(|(area, serial)| {
        area.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'.') && !area.is_empty() && serial.len() == 7 && serial.bytes().all(|b| b.is_ascii_digit())
    });
    (modern || legacy).then(|| bare.to_string())
}

fn strip_prefix_any<'a>(text: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    prefixes.iter().find(|p| lower.starts_with(**p)).map(|p| &text[p.len()..])
}

/// One identifier from a token, or `None` when it is not recognised.
pub fn identify(token: &str) -> Option<Identifier> {
    let token = token.trim().trim_end_matches(['.', ',', ';', ')', ']']);
    if let Some(rest) = strip_prefix_any(token, &["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "http://dx.doi.org/", "doi.org/", "doi:"]) {
        return Some(Identifier::Doi(rest.trim().to_string()));
    }
    if let Some(rest) = strip_prefix_any(token, &["https://arxiv.org/abs/", "http://arxiv.org/abs/", "https://arxiv.org/pdf/", "arxiv:"]) {
        return arxiv_id(rest.trim_end_matches(".pdf")).map(Identifier::Arxiv);
    }
    if let Some(rest) = strip_prefix_any(token, &["pmid:", "https://pubmed.ncbi.nlm.nih.gov/"]) {
        let digits = rest.trim().trim_end_matches('/');
        return (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| Identifier::Pmid(digits.to_string()));
    }
    if let Some(rest) = strip_prefix_any(token, &["isbn:"]) {
        return isbn(rest).map(Identifier::Isbn);
    }
    if token.starts_with("10.") && token.contains('/') { return Some(Identifier::Doi(token.to_string())); }
    if let Some(id) = arxiv_id(token) { return Some(Identifier::Arxiv(id)); }
    if let Some(number) = isbn(token) { return Some(Identifier::Isbn(number)); }
    (token.len() <= 8 && !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit())).then(|| Identifier::Pmid(token.to_string()))
}

/// Tokens split on whitespace and commas. An unrecognised one comes back as
/// its text, so the user can see what was not understood.
pub fn identifiers(text: &str) -> Vec<Result<Identifier, String>> {
    text.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.trim().is_empty()).map(|t| identify(t).ok_or_else(|| t.to_string())).collect()
}

fn get(url: &str, accept: &str) -> Result<String> {
    let response = ureq::get(url).header("Accept", accept).header("User-Agent", USER_AGENT)
        .config().timeout_global(Some(TIMEOUT)).build().call();
    match response {
        Ok(mut response) => Ok(response.body_mut().read_to_string()?),
        Err(ureq::Error::StatusCode(404)) => anyhow::bail!("not found"),
        Err(ureq::Error::StatusCode(code)) => anyhow::bail!("the service answered with status {code}"),
        Err(e) => Err(e.into()),
    }
}

/// Characters a path segment of a DOI cannot carry as they are.
fn encode(id: &str) -> String {
    id.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b'(' | b')' | b':' | b';' => (b as char).to_string(),
        other => format!("%{other:02X}"),
    }).collect()
}

/// A Hayagriva entry from one BibTeX record.
fn from_bibtex(text: &str) -> Result<Mapping> {
    // Services use keys such as URLs that BibTeX parsers reject, and the key
    // is replaced anyway.
    let open = text.find('{').context("the service returned no record")?;
    let comma = open + text[open..].find(',').context("the service returned no record")?;
    let record = format!("{}{{entry{}", &text[..open], &text[comma..]);
    let library = hayagriva::io::from_biblatex_str(&record).map_err(|errors| anyhow::anyhow!(errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")))?;
    let entries: Mapping = serde_yaml::from_str(&hayagriva::io::to_yaml_str(&library)?)?;
    entries.into_iter().next().and_then(|(_, entry)| entry.as_mapping().cloned()).context("the service returned no record")
}

fn serial(entry: &mut Mapping, kind: &str, value: &str) {
    let numbers = entry.entry("serial-number".into()).or_insert_with(|| Value::Mapping(Mapping::new()));
    if let Some(numbers) = numbers.as_mapping_mut() { numbers.insert(kind.into(), value.into()); }
}

fn fetch_doi(doi: &str) -> Result<Mapping> {
    let mut entry = from_bibtex(&get(&format!("https://doi.org/{}", encode(doi)), "application/x-bibtex")?)?;
    // The URL only restates the DOI.
    if entry.get("url").and_then(url_text).is_some_and(|u| u.contains("doi.org/")) { entry.remove("url"); }
    serial(&mut entry, "doi", doi);
    Ok(entry)
}

fn url_text(url: &Value) -> Option<&str> {
    url.as_str().or_else(|| url.get("value")?.as_str())
}

fn fetch_arxiv(id: &str) -> Result<Mapping> {
    let mut entry = fetch_doi(&format!("10.48550/arXiv.{id}"))?;
    serial(&mut entry, "arxiv", id);
    Ok(entry)
}

fn text<'a>(json: &'a Json, key: &str) -> Option<&'a str> {
    json.get(key)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn fetch_pmid(id: &str) -> Result<Mapping> {
    let body = get(&format!("https://pmc.ncbi.nlm.nih.gov/api/ctxp/v1/pubmed/?format=csl&id={id}"), "*/*")?;
    let json: Json = serde_json::from_str(&body).context("the service returned no record")?;
    let title = text(&json, "title").context("not found")?;
    let (kind, parent_kind) = match text(&json, "type") {
        Some("book") => ("Book", "Book"),
        Some("chapter") => ("Chapter", "Book"),
        _ => ("Article", "Periodical"),
    };
    let mut entry = Mapping::new();
    entry.insert("type".into(), kind.into());
    entry.insert("title".into(), title.into());
    let authors: Vec<Value> = json.get("author").and_then(Json::as_array).map_or(vec![], |list| list.iter().filter_map(|person| {
        let family = text(person, "family")?;
        Some(text(person, "given").map_or(family.to_string(), |given| format!("{family}, {given}")).into())
    }).collect());
    if !authors.is_empty() { entry.insert("author".into(), Value::Sequence(authors)); }
    let year = json.pointer("/issued/date-parts/0/0").and_then(|y| y.as_i64().or_else(|| y.as_str()?.parse().ok()));
    if let Some(year) = year { entry.insert("date".into(), year.into()); }
    if let Some(container) = text(&json, "container-title") {
        let mut parent = Mapping::new();
        parent.insert("type".into(), parent_kind.into());
        parent.insert("title".into(), container.into());
        if let Some(volume) = text(&json, "volume") { parent.insert("volume".into(), volume.into()); }
        if let Some(issue) = text(&json, "issue") { parent.insert("issue".into(), issue.into()); }
        entry.insert("parent".into(), Value::Mapping(parent));
    }
    if let Some(pages) = text(&json, "page") { entry.insert("page-range".into(), pages.into()); }
    serial(&mut entry, "pmid", id);
    if let Some(doi) = text(&json, "DOI") { serial(&mut entry, "doi", doi); }
    Ok(entry)
}

/// `Family, Given` from a name written in reading order.
fn sort_name(name: &str) -> String {
    match name.trim().rsplit_once(' ') {
        Some((given, family)) if !name.contains(',') => format!("{family}, {given}"),
        _ => name.trim().to_string(),
    }
}

fn fetch_isbn(id: &str) -> Result<Mapping> {
    let key = format!("ISBN:{id}");
    let body = get(&format!("https://openlibrary.org/api/books?bibkeys={key}&format=json&jscmd=data"), "application/json")?;
    let json: Json = serde_json::from_str(&body).context("the service returned no record")?;
    let book = json.get(&key).context("not found")?;
    let title = text(book, "title").context("not found")?;
    let title = text(book, "subtitle").map_or(title.to_string(), |subtitle| format!("{title}: {subtitle}"));
    let mut entry = Mapping::new();
    entry.insert("type".into(), "Book".into());
    entry.insert("title".into(), title.into());
    let authors: Vec<Value> = book.get("authors").and_then(Json::as_array).map_or(vec![], |list| list.iter().filter_map(|a| Some(sort_name(text(a, "name")?).into())).collect());
    if !authors.is_empty() { entry.insert("author".into(), Value::Sequence(authors)); }
    let year = text(book, "publish_date").and_then(|date| date.as_bytes().windows(4).find(|w| w.iter().all(u8::is_ascii_digit)).and_then(|w| std::str::from_utf8(w).ok()?.parse::<i64>().ok()));
    if let Some(year) = year { entry.insert("date".into(), year.into()); }
    if let Some(publisher) = book.pointer("/publishers/0").and_then(|p| text(p, "name")) { entry.insert("publisher".into(), publisher.into()); }
    serial(&mut entry, "isbn", id);
    Ok(entry)
}

/// What a paper can be found by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperIds {
    pub doi: Option<String>,
    pub arxiv: Option<String>,
}

impl PaperIds {
    /// arXiv's own DOIs name the same paper.
    fn arxiv_id(&self) -> Option<String> {
        self.arxiv.clone().or_else(|| strip_prefix_any(self.doi.as_deref()?, &["10.48550/arxiv."]).map(str::to_string))
    }
}

/// Unpaywall's PDF links for `doi`, best first. None when it has no record.
fn unpaywall_links(doi: &str, email: &str) -> Result<Vec<String>> {
    let body = match get(&format!("https://api.unpaywall.org/v2/{}?email={}", encode(doi), encode(email)), "application/json") {
        Ok(body) => body,
        Err(e) if e.to_string() == "not found" => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let json: Json = serde_json::from_str(&body).context("Unpaywall returned no record")?;
    let others = json.get("oa_locations").and_then(Json::as_array).into_iter().flatten();
    let mut links = vec![];
    for location in json.get("best_oa_location").into_iter().chain(others) {
        if let Some(link) = text(location, "url_for_pdf") && !links.iter().any(|known| known == link) { links.push(link.to_string()); }
    }
    Ok(links)
}

/// Saves the PDF at `url` to `dest`, whole or not at all.
fn download(url: &str, dest: &Path) -> Result<()> {
    anyhow::ensure!(url.starts_with("https://") || url.starts_with("http://"), "not a web link");
    let mut response = ureq::get(url).header("Accept", "application/pdf").header("User-Agent", USER_AGENT)
        .config().timeout_global(Some(PAPER_TIMEOUT)).build().call()?;
    let bytes = response.body_mut().with_config().limit(PAPER_LIMIT).read_to_vec()?;
    // Publishers often answer a PDF link with a login page.
    anyhow::ensure!(bytes.starts_with(b"%PDF-"), "the link did not lead to a PDF");
    let name = dest.file_name().context("no file name")?.to_string_lossy();
    let partial = dest.with_file_name(format!(".{name}.part"));
    if let Some(dir) = dest.parent() { std::fs::create_dir_all(dir)?; }
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, dest)?;
    Ok(())
}

/// Downloads an open-access copy to `dest`: arXiv first, then Unpaywall when
/// there is an `email` to ask with. False when neither knows a copy.
pub fn fetch_paper(ids: &PaperIds, email: Option<&str>, dest: &Path) -> Result<bool> {
    let mut links: Vec<String> = ids.arxiv_id().map(|id| format!("https://arxiv.org/pdf/{}", encode(&id))).into_iter().collect();
    let mut problem = None;
    if let (Some(doi), Some(email)) = (&ids.doi, email) {
        match unpaywall_links(doi, email) {
            Ok(found) => links.extend(found),
            Err(e) => problem = Some(e),
        }
    }
    for link in links {
        match download(&link, dest) {
            Ok(()) => return Ok(true),
            Err(e) => problem = Some(e),
        }
    }
    problem.map_or(Ok(false), Err)
}

/// Fetches one identifier as a Hayagriva entry, without its key.
pub fn fetch(id: &Identifier) -> Result<Mapping> {
    match id {
        Identifier::Doi(doi) => fetch_doi(doi),
        Identifier::Arxiv(arxiv) => fetch_arxiv(arxiv),
        Identifier::Pmid(pmid) => fetch_pmid(pmid),
        Identifier::Isbn(isbn) => fetch_isbn(isbn),
    }
}
