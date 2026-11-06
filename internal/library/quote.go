package library

import (
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/cheetahbyte/thesio/internal/vault"
)

// Turns a PDF highlight into a Typst quotation with a page citation.

// Quotes of this many words or more stand as a block.
const blockWords = 40

// BlockGlue is what separates a block from the text before it, so it stands as
// a paragraph. vault.BlockGlue is a copy; keep the two identical.
func BlockGlue(before string) string {
	switch {
	case before == "" || strings.HasSuffix(before, "\n\n"):
		return ""
	case strings.HasSuffix(before, "\n"):
		return "\n"
	}
	return "\n\n"
}

var ligatures = strings.NewReplacer("­", "", "ﬀ", "ff", "ﬁ", "fi", "ﬂ", "fl", "ﬃ", "ffi", "ﬄ", "ffl", "ﬅ", "st", "ﬆ", "st")

// CleanQuote is raw as read from a PDF: line-end hyphens joined, ligatures
// spelled out, whitespace collapsed. PDFKit turns line breaks into spaces, so
// "exam- ple" is taken for a break and "well- known" needs the user's eye.
// Suspended hyphens ("pre- and post-war") stay.
func CleanQuote(raw string) string {
	text := ""
	for _, word := range strings.Fields(ligatures.Replace(raw)) {
		head, hyphen := strings.CutSuffix(text, "-")
		last, _ := utf8.DecodeLastRuneInString(head)
		first, _ := utf8.DecodeRuneInString(word)
		if hyphen && unicode.IsLetter(last) && unicode.IsLower(first) && word != "and" && word != "or" {
			text = head
		} else if text != "" {
			text += " "
		}
		text += word
	}
	return text
}

// basePage is the numeric page a label stands for: PDFKit's label, or the
// position in the file when there is none. False for labels like iv.
func basePage(label string, index int) (int, bool) {
	if label == "" { return index + 1, true }
	page, err := strconv.ParseInt(label, 10, 32)
	return int(page), err == nil
}

// PrintedPage is the page as printed. offset corrects PDFs whose numbering
// starts elsewhere; empty when that lands before the first page, as on a cover.
func PrintedPage(label string, index int, offset int) string {
	page, ok := basePage(label, index)
	switch {
	case !ok:
		return label
	case page+offset > 0:
		return strconv.Itoa(page + offset)
	}
	return ""
}

// PageOffset is the offset that makes this page read entered, when both are numbers.
func PageOffset(label string, index int, entered string) (int, bool) {
	want, err := strconv.ParseInt(strings.TrimSpace(entered), 10, 32)
	page, ok := basePage(label, index)
	if err != nil || !ok { return 0, false }
	return int(want) - page, true
}

// QuoteMarkup is the quotation as markup, and whether it stands as a block.
func QuoteMarkup(text, key, page string) (string, bool) {
	words := strings.Fields(text)
	body := vault.EscapeMarkup(strings.Join(words, " "))
	page = strings.TrimSpace(page)
	cite := "@" + key
	if page != "" {
		unit := "p."
		if strings.ContainsAny(page, "-–,") { unit = "pp." }
		cite += "[" + unit + " " + vault.EscapeMarkup(page) + "]"
	}
	if len(words) >= blockWords { return "#quote(block: true, attribution: [" + cite + "])[" + body + "]", true }
	return "#quote[" + body + "] " + cite, false
}
