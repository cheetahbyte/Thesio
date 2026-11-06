package library

import (
	"strings"
	"unicode"
)

// Recognises DOIs, arXiv IDs, PMIDs, and ISBNs in pasted text. Pure: looking
// one up is lookup.go's business.

// The DOI prefix arXiv gives its papers, as lowercase text.
const arxivDOIPrefix = "10.48550/arxiv."

// IDKind is the kind of an Identifier.
type IDKind int

// The identifiers Identify recognises.
const (
	IDDOI IDKind = iota
	IDArxiv
	IDPMID
	IDISBN
)

// Identifier is a DOI, arXiv ID, PMID, or ISBN in its plain form.
type Identifier struct {
	Kind  IDKind
	Value string
}

func (id Identifier) String() string {
	return [...]string{"DOI", "arXiv", "PMID", "ISBN"}[id.Kind] + " " + id.Value
}

// IdentifierOrText is a recognised identifier (OK), or the token as it was.
type IdentifierOrText struct {
	ID   Identifier
	Text string
	OK   bool
}

func allDigits(text string) bool {
	for i := 0; i < len(text); i++ {
		if text[i] < '0' || text[i] > '9' {
			return false
		}
	}
	return true
}

// isbn is the ISBN digits of a token, when its check digit is right.
func isbn(token string) (string, bool) {
	digits := strings.ToUpper(strings.NewReplacer("-", "", " ", "").Replace(token))
	switch {
	case len(digits) == 10 && allDigits(digits[:9]):
		total := 0
		for i, c := range digits {
			d := 99
			if c == 'X' {
				d = 10
			} else if '0' <= c && c <= '9' {
				d = int(c - '0')
			}
			total += (10 - i) * d
		}
		return digits, total%11 == 0
	case len(digits) == 13 && allDigits(digits):
		total := 0
		for i, c := range digits {
			total += int(c-'0') * (1 + 2*(i%2))
		}
		return digits, total%10 == 0
	}
	return "", false
}

func isAreaChar(b byte) bool { return 'a' <= b && b <= 'z' || b == '-' || b == '.' }

// arxivToken is the ID in a token with the shape of an arXiv ID, with or without a version.
func arxivToken(token string) (string, bool) {
	bare := token
	if id, v, ok := strings.Cut(token, "v"); ok && v != "" && allDigits(v) {
		bare = id
	}
	modern := false
	if month, serial, ok := strings.Cut(bare, "."); ok {
		modern = len(month) == 4 && (len(serial) == 4 || len(serial) == 5) && allDigits(month) && allDigits(serial)
	}
	legacy := false
	if area, serial, ok := strings.Cut(bare, "/"); ok && area != "" && len(serial) == 7 && allDigits(serial) {
		legacy = true
		for i := 0; i < len(area); i++ {
			legacy = legacy && isAreaChar(area[i])
		}
	}
	return bare, modern || legacy
}

func stripPrefixAny(text string, prefixes ...string) (string, bool) {
	lower := strings.ToLower(text)
	for _, p := range prefixes {
		if strings.HasPrefix(lower, p) {
			return text[len(p):], true
		}
	}
	return "", false
}

// Identify reads one identifier from a token: a DOI (bare, doi:, or a doi.org
// link), an arXiv ID (bare, arxiv:, or an abs or pdf link), a PMID (pmid:, a
// PubMed link, or up to eight digits), or an ISBN with a valid check digit.
func Identify(token string) (Identifier, bool) {
	if id, ok := identify(token); ok {
		return id, true
	}
	return Identifier{}, false
}

func identify(token string) (Identifier, bool) {
	token = strings.TrimRight(strings.TrimSpace(token), ".,;)]")
	if rest, ok := stripPrefixAny(token, "https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "http://dx.doi.org/", "doi.org/", "doi:"); ok {
		return Identifier{IDDOI, strings.TrimSpace(rest)}, true
	}
	if rest, ok := stripPrefixAny(token, "https://arxiv.org/abs/", "http://arxiv.org/abs/", "https://arxiv.org/pdf/", "arxiv:"); ok {
		for strings.HasSuffix(rest, ".pdf") {
			rest = strings.TrimSuffix(rest, ".pdf")
		}
		id, ok := arxivToken(rest)
		return Identifier{IDArxiv, id}, ok
	}
	if rest, ok := stripPrefixAny(token, "pmid:", "https://pubmed.ncbi.nlm.nih.gov/"); ok {
		digits := strings.TrimRight(strings.TrimSpace(rest), "/")
		return Identifier{IDPMID, digits}, digits != "" && allDigits(digits)
	}
	if rest, ok := stripPrefixAny(token, "isbn:"); ok {
		number, ok := isbn(rest)
		return Identifier{IDISBN, number}, ok
	}
	if strings.HasPrefix(token, "10.") && strings.Contains(token, "/") {
		return Identifier{IDDOI, token}, true
	}
	if id, ok := arxivToken(token); ok {
		return Identifier{IDArxiv, id}, true
	}
	if number, ok := isbn(token); ok {
		return Identifier{IDISBN, number}, true
	}
	if len(token) <= 8 && token != "" && allDigits(token) {
		return Identifier{IDPMID, token}, true
	}
	return Identifier{}, false
}

// Identifiers are the tokens of text split on whitespace and commas. An
// unrecognised one comes back as its text, so the user can see what was not understood.
func Identifiers(text string) []IdentifierOrText {
	var list []IdentifierOrText
	for _, token := range strings.FieldsFunc(text, func(c rune) bool { return c == ',' || unicode.IsSpace(c) }) {
		id, ok := Identify(token)
		list = append(list, IdentifierOrText{ID: id, Text: token, OK: ok})
	}
	return list
}

// PaperIDs are what finds an open copy of a paper; "" for none.
type PaperIDs struct{ DOI, Arxiv string }

// ArxivID is the stated arXiv ID, or the one in arXiv's own DOI, which names the same paper.
func (p PaperIDs) ArxivID() string {
	if p.Arxiv != "" {
		return p.Arxiv
	}
	id, _ := stripPrefixAny(p.DOI, arxivDOIPrefix)
	return id
}

// ArxivDOI is the DOI arXiv gives paper id, which doi.org resolves like any other.
func ArxivDOI(id string) string { return "10.48550/arXiv." + id }

// ArxivOfLink is the arXiv ID a link points at, "" when it is not an arXiv link.
func ArxivOfLink(url string) string {
	if id, ok := Identify(url); ok && id.Kind == IDArxiv {
		return id.Value
	}
	return ""
}
