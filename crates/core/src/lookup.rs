//! Looks up bibliographic metadata by identifier: DOIs and arXiv IDs through
//! doi.org, PMIDs through NCBI, ISBNs through Open Library. Also downloads
//! open-access papers from arXiv and the locations Unpaywall lists. Network
//! access happens only here, and only for sources the user added to the library.

use crate::source_type::{ARTICLE, ARXIV, AUTHORS, BOOK, CHAPTER, DATE, DOI, ISBN, PAGES, PARENT_ISSUE, PARENT_TITLE, PARENT_VOLUME, PERIODICAL, PMID, PUBLISHER, Slot, TITLE, URL, read, write};
use crate::identifier::{Identifier, PaperIds, arxiv_doi};
use anyhow::{Context, Result};
use serde_json::Value as Json;
use serde_yaml::Mapping;
use std::path::Path;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str = concat!("Thesio/", env!("CARGO_PKG_VERSION"));
const PAPER_TIMEOUT: Duration = Duration::from_secs(120);
const PAPER_LIMIT: u64 = 200 * 1024 * 1024;

/// The service has no such record.
#[derive(Debug)]
struct NotFound;

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str("not found") }
}

impl std::error::Error for NotFound {}

fn request(url: &str, accept: &str, timeout: Duration) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    ureq::get(url).header("Accept", accept).header("User-Agent", USER_AGENT).config().timeout_global(Some(timeout)).build().call()
}

fn get(url: &str, accept: &str) -> Result<String> {
    match request(url, accept, TIMEOUT) {
        Ok(mut response) => Ok(response.body_mut().read_to_string()?),
        Err(ureq::Error::StatusCode(404)) => Err(NotFound.into()),
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

fn fetch_doi(doi: &str) -> Result<Mapping> {
    let mut entry = crate::biblatex::record(&get(&format!("https://doi.org/{}", encode(doi)), "application/x-bibtex")?)?;
    // The URL only restates the DOI.
    if read(&entry, URL.slot).contains("doi.org/") { write(&mut entry, URL.slot, ""); }
    write(&mut entry, DOI.slot, doi);
    Ok(entry)
}

fn fetch_arxiv(id: &str) -> Result<Mapping> {
    let mut entry = fetch_doi(&arxiv_doi(id))?;
    write(&mut entry, ARXIV.slot, id);
    Ok(entry)
}

fn text<'a>(json: &'a Json, key: &str) -> Option<&'a str> {
    json.get(key)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn fetch_pmid(id: &str) -> Result<Mapping> {
    let body = get(&format!("https://pmc.ncbi.nlm.nih.gov/api/ctxp/v1/pubmed/?format=csl&id={id}"), "*/*")?;
    let json: Json = serde_json::from_str(&body).context("the service returned no record")?;
    let title = text(&json, "title").ok_or(NotFound)?;
    let (kind, parent_kind) = match text(&json, "type") {
        Some("book") => (BOOK, BOOK),
        Some("chapter") => (CHAPTER, BOOK),
        _ => (ARTICLE, PERIODICAL),
    };
    let mut entry = Mapping::new();
    write(&mut entry, Slot::Type, kind);
    write(&mut entry, TITLE.slot, title);
    let authors: Vec<String> = json.get("author").and_then(Json::as_array).map_or(vec![], |list| list.iter().filter_map(|person| {
        let family = text(person, "family")?;
        Some(text(person, "given").map_or(family.to_string(), |given| format!("{family}, {given}")))
    }).collect());
    write(&mut entry, AUTHORS.slot, &authors.join("; "));
    let year = json.pointer("/issued/date-parts/0/0").and_then(|y| y.as_i64().or_else(|| y.as_str()?.parse().ok()));
    if let Some(year) = year { write(&mut entry, DATE.slot, &year.to_string()); }
    if let Some(container) = text(&json, "container-title") {
        write(&mut entry, Slot::ParentType, parent_kind);
        write(&mut entry, PARENT_TITLE, container);
        write(&mut entry, PARENT_VOLUME, text(&json, "volume").unwrap_or_default());
        write(&mut entry, PARENT_ISSUE, text(&json, "issue").unwrap_or_default());
    }
    write(&mut entry, PAGES.slot, text(&json, "page").unwrap_or_default());
    write(&mut entry, PMID, id);
    write(&mut entry, DOI.slot, text(&json, "DOI").unwrap_or_default());
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
    let book = json.get(&key).ok_or(NotFound)?;
    let title = text(book, "title").ok_or(NotFound)?;
    let title = text(book, "subtitle").map_or(title.to_string(), |subtitle| format!("{title}: {subtitle}"));
    let mut entry = Mapping::new();
    write(&mut entry, Slot::Type, BOOK);
    write(&mut entry, TITLE.slot, &title);
    let authors: Vec<String> = book.get("authors").and_then(Json::as_array).map_or(vec![], |list| list.iter().filter_map(|a| Some(sort_name(text(a, "name")?))).collect());
    write(&mut entry, AUTHORS.slot, &authors.join("; "));
    let year = text(book, "publish_date").and_then(|date| date.as_bytes().windows(4).find(|w| w.iter().all(u8::is_ascii_digit)).and_then(|w| std::str::from_utf8(w).ok()?.parse::<i64>().ok()));
    if let Some(year) = year { write(&mut entry, DATE.slot, &year.to_string()); }
    write(&mut entry, PUBLISHER.slot, book.pointer("/publishers/0").and_then(|p| text(p, "name")).unwrap_or_default());
    write(&mut entry, ISBN.slot, id);
    Ok(entry)
}

/// Unpaywall's PDF links for `doi`, best first. Empty when it has no record.
fn unpaywall_links(doi: &str, email: &str) -> Result<Vec<String>> {
    let body = match get(&format!("https://api.unpaywall.org/v2/{}?email={}", encode(doi), encode(email)), "application/json") {
        Ok(body) => body,
        Err(e) if e.is::<NotFound>() => return Ok(vec![]),
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
    let mut response = request(url, "application/pdf", PAPER_TIMEOUT)?;
    let bytes = response.body_mut().with_config().limit(PAPER_LIMIT).read_to_vec()?;
    // Publishers often answer a PDF link with a login page.
    anyhow::ensure!(bytes.starts_with(crate::paths::PDF_HEADER), "the link did not lead to a PDF");
    if let Some(dir) = dest.parent() { std::fs::create_dir_all(dir)?; }
    crate::paths::write_atomic(dest, &bytes)
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
