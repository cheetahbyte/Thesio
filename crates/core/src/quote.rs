//! Turns a PDF highlight into a Typst quotation with a page citation.

/// Quotes of this many words or more stand as a block.
const BLOCK_WORDS: usize = 40;

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

/// `text` as Typst markup that renders literally.
fn escape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let special = match c {
            '\\' | '#' | '$' | '*' | '_' | '`' | '@' | '<' | '>' | '[' | ']' | '~' => true,
            // Comments, and a term list at the start.
            '/' => i == 0 || matches!(next, Some('/' | '*')),
            // Lists, dashes, soft hyphens, and minus signs.
            '-' => i == 0 || i > 0 && chars[i - 1] == '-' || next.is_some_and(|n| matches!(n, '-' | '?') || n.is_ascii_digit()),
            '+' | '=' => i == 0,
            // A numbered list at the start, and the ellipsis shorthand.
            '.' => (i > 0 && chars[..i].iter().all(char::is_ascii_digit)) || chars[i..].starts_with(&['.', '.', '.']),
            _ => false,
        };
        if special { out.push('\\'); }
        out.push(c);
    }
    out
}

/// The quotation as markup, and whether it stands as a block.
pub fn quote_markup(text: &str, key: &str, page: &str) -> (String, bool) {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let body = escape(&words.join(" "));
    let page = page.trim();
    let cite = if page.is_empty() {
        format!("@{key}")
    } else {
        let unit = if page.contains(['-', '–', ',']) { "pp." } else { "p." };
        format!("@{key}[{unit} {}]", escape(page))
    };
    if words.len() >= BLOCK_WORDS { (format!("#quote(block: true, attribution: [{cite}])[{body}]"), true) } else { (format!("#quote[{body}] {cite}"), false) }
}
