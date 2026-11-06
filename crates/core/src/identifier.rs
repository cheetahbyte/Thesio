//! Recognises DOIs, arXiv IDs, PMIDs, and ISBNs in pasted text. Pure: looking
//! one up is `lookup`'s business.

/// The DOI prefix arXiv gives its papers, as lowercase text.
const ARXIV_DOI: &str = "10.48550/arxiv.";

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
fn arxiv_token(token: &str) -> Option<String> {
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
        return arxiv_token(rest.trim_end_matches(".pdf")).map(Identifier::Arxiv);
    }
    if let Some(rest) = strip_prefix_any(token, &["pmid:", "https://pubmed.ncbi.nlm.nih.gov/"]) {
        let digits = rest.trim().trim_end_matches('/');
        return (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| Identifier::Pmid(digits.to_string()));
    }
    if let Some(rest) = strip_prefix_any(token, &["isbn:"]) {
        return isbn(rest).map(Identifier::Isbn);
    }
    if token.starts_with("10.") && token.contains('/') { return Some(Identifier::Doi(token.to_string())); }
    if let Some(id) = arxiv_token(token) { return Some(Identifier::Arxiv(id)); }
    if let Some(number) = isbn(token) { return Some(Identifier::Isbn(number)); }
    (token.len() <= 8 && !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit())).then(|| Identifier::Pmid(token.to_string()))
}

/// Tokens split on whitespace and commas. An unrecognised one comes back as
/// its text, so the user can see what was not understood.
pub fn identifiers(text: &str) -> Vec<Result<Identifier, String>> {
    text.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.trim().is_empty()).map(|t| identify(t).ok_or_else(|| t.to_string())).collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperIds {
    pub doi: Option<String>,
    pub arxiv: Option<String>,
}

impl PaperIds {
    /// arXiv's own DOIs name the same paper.
    pub(crate) fn arxiv_id(&self) -> Option<String> {
        self.arxiv.clone().or_else(|| strip_prefix_any(self.doi.as_deref()?, &[ARXIV_DOI]).map(str::to_string))
    }
}

/// The DOI arXiv gives paper `id`, which doi.org resolves like any other.
pub(crate) fn arxiv_doi(id: &str) -> String {
    format!("10.48550/arXiv.{id}")
}

pub(crate) fn arxiv_of_link(url: &str) -> Option<String> {
    match identify(url) {
        Some(Identifier::Arxiv(id)) => Some(id),
        _ => None,
    }
}
