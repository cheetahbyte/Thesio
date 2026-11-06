package typst

/*
#include <stdlib.h>
#include "thesio_typst.h"
*/
import "C"

import (
	"encoding/json"
	"unsafe"
)

// Span is a highlighted leaf. Tag is the kebab-case name of Typst's highlight
// tag: comment, punctuation, escape, strong, emph, link, raw, label, ref,
// heading, list-marker, list-term, math-delimiter, math-operator,
// math-grouping-parens, keyword, operator, number, string, function,
// interpolated, error.
type Span struct {
	Start int    `json:"s"`
	End   int    `json:"e"`
	Tag   string `json:"t"`
}

// Highlight lists the highlighted leaves of text, sorted and not overlapping.
func Highlight(text string) []Span {
	var a arena
	defer a.free()
	var spans []Span
	if raw, err := result(C.thesio_highlight(a.str(text))); err == nil {
		_ = json.Unmarshal(raw, &spans)
	}
	return spans
}

// ProseWords counts the words text writes as prose, as a compile would draw them.
func ProseWords(text string) int {
	var a arena
	defer a.free()
	return int(C.thesio_prose_words(a.str(text)))
}

type Heading struct {
	Offset int    `json:"offset"`
	Level  int    `json:"level"` // 1 for `=`
	Title  string `json:"title"`
}

// Include is an `#include` with where its path is written and the path as written.
type Include struct {
	Offset  int    `json:"offset"`
	Literal string `json:"literal"`
}

type Parsed struct {
	Headings []Heading `json:"headings"`
	Includes []Include `json:"includes"`
}

func ParseDocument(text string) Parsed {
	var a arena
	defer a.free()
	var parsed Parsed
	if raw, err := result(C.thesio_parse_document(a.str(text))); err == nil {
		_ = json.Unmarshal(raw, &parsed)
	}
	return parsed
}

// NoteLink is a `#link` to a vault document: as written, the vault path it
// points at, and the bytes of the whole call.
type NoteLink struct {
	Literal string `json:"literal"`
	Target  string `json:"target"`
	Start   int    `json:"start"`
	End     int    `json:"end"`
}

// NoteLinks lists the note links in text, the document at vault path file.
func NoteLinks(text, file string) []NoteLink {
	var a arena
	defer a.free()
	var links []NoteLink
	if raw, err := result(C.thesio_note_links(a.str(text), a.str(file))); err == nil {
		_ = json.Unmarshal(raw, &links)
	}
	return links
}

// NoteLinkAt finds the `[[query` being written at caret, brackets included.
func NoteLinkAt(text string, caret int) (start, end int, ok bool) {
	var a arena
	defer a.free()
	var s, e C.size_t
	if !C.thesio_note_link_at(a.str(text), size(caret), &s, &e) {
		return 0, 0, false
	}
	return int(s), int(e), true
}

// CitationAt finds the `@key` being written at caret, `@` included.
func CitationAt(text string, caret int) (start, end int, ok bool) {
	var a arena
	defer a.free()
	var s, e C.size_t
	if !C.thesio_citation_at(a.str(text), size(caret), &s, &e) {
		return 0, 0, false
	}
	return int(s), int(e), true
}

// CitedKeys lists the keys text cites with `@key` and `#cite(<key>)`.
func CitedKeys(text string) []string {
	var a arena
	defer a.free()
	var keys []string
	if raw, err := result(C.thesio_cited_keys(a.str(text))); err == nil {
		_ = json.Unmarshal(raw, &keys)
	}
	return keys
}

// CitationsOf lists where key is cited: the byte after each `@` or `<`.
func CitationsOf(text, key string) []int {
	var a arena
	defer a.free()
	var starts []int
	if raw, err := result(C.thesio_citations_of(a.str(text), a.str(key))); err == nil {
		_ = json.Unmarshal(raw, &starts)
	}
	return starts
}

// RenameCitation changes every citation of old to new. With labelled, `@old`
// means a label and only `#cite(<old>)` changes. False when nothing changed.
func RenameCitation(text, old, new string, labelled bool) (string, bool) {
	var a arena
	defer a.free()
	return optionalText(C.thesio_rename_citation(a.str(text), a.str(old), a.str(new), C.bool(labelled)))
}

// DefinesLabel says whether text puts the label key on something.
func DefinesLabel(text, key string) bool {
	var a arena
	defer a.free()
	return bool(C.thesio_defines_label(a.str(text), a.str(key)))
}

// RewritePaths updates the path arguments of text after a move. The file moved
// from oldFile to newFile (equal when it stayed); main is its top-level document
// before the move; files are every file's vault path before the move; from→to
// is the move (a path inside from moves inside to). False when nothing changed.
func RewritePaths(text, oldFile, newFile, main string, files []string, from, to string) (string, bool) {
	if files == nil {
		files = []string{}
	}
	list, err := json.Marshal(files)
	if err != nil {
		return "", false
	}
	var a arena
	defer a.free()
	return optionalText(C.thesio_rewrite_paths(a.str(text), a.str(oldFile), a.str(newFile), a.str(main), a.str(string(list)), a.str(from), a.str(to)))
}

// BibMode is how a document gets its bibliography.
type BibMode int

const (
	BibNone     BibMode = iota // no `#bibliography`
	BibEmbedded                // the generated block
	BibLinked                  // `#bibliography` of library.yaml
	BibCustom                  // a `#bibliography` the user wrote
)

func BibliographyMode(text, file string) BibMode {
	var a arena
	defer a.free()
	return BibMode(C.thesio_bibliography_mode(a.str(text), a.str(file)))
}

func BibliographyStyle(text string) (string, bool) {
	var a arena
	defer a.free()
	return optionalText(C.thesio_bibliography_style(a.str(text)))
}

// SetBibliographyStyle sets the style of the first `#bibliography`; false without one.
func SetBibliographyStyle(text, style string) (string, bool) {
	var a arena
	defer a.free()
	return optionalText(C.thesio_set_bibliography_style(a.str(text), a.str(style)))
}

// BibliographyBlockRange is the generated block's bytes, with its final newline.
func BibliographyBlockRange(text string) (start, end int, ok bool) {
	var a arena
	defer a.free()
	var s, e C.size_t
	if !C.thesio_bibliography_block_range(a.str(text), &s, &e) {
		return 0, 0, false
	}
	return int(s), int(e), true
}

// plain returns the crate's text, or fallback when it failed.
func plain(p *C.char, fallback string) string {
	if text, ok := take(p); ok {
		return text
	}
	return fallback
}

func WithoutBibliographyBlock(text string) string {
	var a arena
	defer a.free()
	return plain(C.thesio_without_bibliography_block(a.str(text)), text)
}

// SyncBibliographyBlock puts yaml in the generated block; false when unchanged or without one.
func SyncBibliographyBlock(text, yaml string) (string, bool) {
	var a arena
	defer a.free()
	return optionalText(C.thesio_sync_bibliography_block(a.str(text), a.str(yaml)))
}

func EmbedBibliography(text, yaml string) string {
	var a arena
	defer a.free()
	return plain(C.thesio_embed_bibliography(a.str(text), a.str(yaml)), text)
}

// LinkBibliography appends a `#bibliography` of library.yaml, relative to file.
func LinkBibliography(text, file string) string {
	var a arena
	defer a.free()
	return plain(C.thesio_link_bibliography(a.str(text), a.str(file)), text)
}

// RemoveBibliography drops the generated block or the linked `#bibliography`; a written one stays.
func RemoveBibliography(text, file string) string {
	var a arena
	defer a.free()
	return plain(C.thesio_remove_bibliography(a.str(text), a.str(file)), text)
}

// FigurePlace is where a figure goes for a caret at at: past a figure around it.
func FigurePlace(text string, at int) int {
	var a arena
	defer a.free()
	if at < 0 {
		return at
	}
	return int(C.thesio_figure_place(a.str(text), C.size_t(at)))
}

// CodeOpens says whether text at start is code just begun: after a `#`, or the dot of a field access.
func CodeOpens(text string, start int) bool {
	var a arena
	defer a.free()
	return bool(C.thesio_code_opens(a.str(text), size(start)))
}

// ImageFormat names what Typst reads data as: png, jpg, gif, webp, svg or pdf.
func ImageFormat(data []byte) (ext string, ok bool) {
	if len(data) == 0 {
		return "", false
	}
	return take(C.thesio_image_format((*C.uint8_t)(unsafe.Pointer(&data[0])), C.size_t(len(data))))
}

// CheckLibrary reports why Typst cannot read yaml as a library, or nil.
func CheckLibrary(yaml string) error {
	var a arena
	defer a.free()
	_, err := result(C.thesio_library_check(a.str(yaml)))
	return err
}

// ParsedEntry is a library entry as Typst reads it. Authors are "Family, Given";
// Date is YYYY[-MM[-DD]], "" without one.
type ParsedEntry struct {
	Key         string   `json:"key"`
	Title       string   `json:"title"`
	Authors     []string `json:"authors"`
	Date        string   `json:"date"`
	ParentTitle string   `json:"parent_title"`
}

func LibraryEntries(yaml string) ([]ParsedEntry, error) {
	var a arena
	defer a.free()
	raw, err := result(C.thesio_library_entries(a.str(yaml)))
	if err != nil {
		return nil, err
	}
	var entries []ParsedEntry
	err = json.Unmarshal(raw, &entries)
	return entries, err
}

// BiblatexImport is BibLaTeX read by Hayagriva as a whole library, and per key
// the fields it drops, as written: keywords and language.
type BiblatexImport struct {
	YAML     string                       `json:"yaml"`
	Verbatim map[string]map[string]string `json:"verbatim"`
}

func ImportBiblatex(bibtex string) (BiblatexImport, error) {
	var a arena
	defer a.free()
	raw, err := result(C.thesio_biblatex_import(a.str(bibtex)))
	if err != nil {
		return BiblatexImport{}, err
	}
	var imported BiblatexImport
	if err := json.Unmarshal(raw, &imported); err != nil {
		return BiblatexImport{}, err
	}
	return imported, nil
}
