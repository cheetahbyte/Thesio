//! Turns a PDF highlight into a Typst quotation with a page citation.

use crate::typst_source::escape_markup;

/// Quotes of this many words or more stand as a block.
const BLOCK_WORDS: usize = 40;

/// What separates a block from the text `before` it, so it stands as a paragraph.
pub fn block_glue(before: &str) -> &'static str {
    if before.is_empty() || before.ends_with("\n\n") { "" } else if before.ends_with('\n') { "\n" } else { "\n\n" }
}

/// `raw` as read from a PDF: line-end hyphens joined, ligatures spelled out,
/// whitespace collapsed. PDFKit turns line breaks into spaces, so "exam- ple"
/// is taken for a break and "well- known" needs the user's eye. Suspended
/// hyphens ("pre- and post-war") stay.
pub fn clean_quote(raw: &str) -> String {
    let spelled = raw.replace('\u{ad}', "").replace('ﬀ', "ff").replace('ﬁ', "fi").replace('ﬂ', "fl").replace('ﬃ', "ffi").replace('ﬄ', "ffl").replace(['ﬅ', 'ﬆ'], "st");
    let mut text = String::with_capacity(spelled.len());
    for word in spelled.split_whitespace() {
        let broken = text.strip_suffix('-').is_some_and(|head| head.chars().next_back().is_some_and(char::is_alphabetic));
        if broken && word.chars().next().is_some_and(char::is_lowercase) && !matches!(word, "and" | "or") {
            text.pop();
        } else if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(word);
    }
    text
}

/// The numeric page a label stands for: PDFKit's label, or the position in the
/// file when there is none. None for labels like `iv`.
fn base_page(label: &str, index: usize) -> Option<i32> {
    if label.is_empty() { Some(index as i32 + 1) } else { label.parse().ok() }
}

/// The page as printed. `offset` corrects PDFs whose numbering starts elsewhere;
/// empty when that lands before the first page, as on a cover.
pub fn printed_page(label: &str, index: usize, offset: i32) -> String {
    match base_page(label, index) {
        Some(page) if page + offset > 0 => (page + offset).to_string(),
        Some(_) => String::new(),
        None => label.to_string(),
    }
}

/// The offset that makes this page read `entered`, when both are numbers.
pub fn page_offset(label: &str, index: usize, entered: &str) -> Option<i32> {
    Some(entered.trim().parse::<i32>().ok()? - base_page(label, index)?)
}

/// The quotation as markup, and whether it stands as a block.
pub fn quote_markup(text: &str, key: &str, page: &str) -> (String, bool) {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let body = escape_markup(&words.join(" "));
    let page = page.trim();
    let cite = if page.is_empty() {
        format!("@{key}")
    } else {
        let unit = if page.contains(['-', '–', ',']) { "pp." } else { "p." };
        format!("@{key}[{unit} {}]", escape_markup(page))
    };
    if words.len() >= BLOCK_WORDS { (format!("#quote(block: true, attribution: [{cite}])[{body}]"), true) } else { (format!("#quote[{body}] {cite}"), false) }
}
