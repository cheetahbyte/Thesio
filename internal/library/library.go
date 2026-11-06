// Package library is the citation library: Hayagriva entries in library.yaml at
// the vault root. The YAML mapping is the source of truth, so editing an entry
// keeps every field the form does not show. A file that does not parse is never
// overwritten.
package library

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/cheetahbyte/thesio/internal/vault"
	"gopkg.in/yaml.v3"
)

// ParsedEntry is an entry as Hayagriva reads it, as typst.ParsedEntry. Authors
// are "Family, Given"; Date is "YYYY[-MM[-DD]]", "" when it has none.
type ParsedEntry struct {
	Key, Title  string
	Authors     []string
	Date        string
	ParentTitle string
}

// BiblatexImport is Hayagriva YAML read from BibLaTeX, with the raw text of the
// fields Hayagriva drops (keywords, language) per key, as typst.BiblatexImport.
type BiblatexImport struct {
	YAML     string
	Verbatim map[string]map[string]string
}

// Engine is Hayagriva, which lives in the Typst library.
type Engine interface {
	Check(yaml string) error
	Entries(yaml string) ([]ParsedEntry, error)
	ImportBiblatex(bibtex string) (BiblatexImport, error)
}

// Hayagriva is set by the app before Load.
var Hayagriva Engine

// PapersDir is the vault folder that holds downloaded papers.
const PapersDir = "sources"

// NotesDir is the vault folder that holds notes on sources.
const NotesDir = "notes"

// CitationStyles are offered per document: a label and Typst's name for it.
var CitationStyles = [][2]string{
	{"IEEE", "ieee"},
	{"APA", "apa"},
	{"Chicago (Author-Date)", "chicago-author-date"},
	{"Chicago (Notes)", "chicago-notes"},
	{"MLA", "mla"},
	{"Harvard", "harvard-cite-them-right"},
	{"Vancouver", "vancouver"},
	{"Nature", "nature"},
	{"ACM", "association-for-computing-machinery"},
	{"Springer", "springer-basic"},
}

// DefaultStyle is what Typst uses for a #bibliography without a style.
const DefaultStyle = "ieee"

// NoteFile is the notes document that goes with paper id, or with PaperFile(key).
func NoteFile(paper string) string {
	name := vault.BaseName(paper)
	return NotesDir + "/" + vault.DocumentName(strings.TrimSuffix(name, ".pdf"))
}

// PaperFile is the vault path of the paper for key. Keys may hold characters a file name should not.
func PaperFile(key string) string {
	stem := strings.Map(func(c rune) rune {
		if unicode.IsLetter(c) || unicode.IsNumber(c) || c == '-' || c == '_' || c == '.' { return c }
		return '-'
	}, key)
	return PaperNamed(strings.TrimLeft(stem, "."))
}

// PaperNamed is the vault path of a paper with no source, under the name it came with.
func PaperNamed(stem string) string { return PapersDir + "/" + stem + ".pdf" }

// Row is a table row.
type Row struct {
	Key, Kind, Title, Authors, Year string
	Tags                            []string
}

// Label is what to call it: its title, or its key without one.
func (r Row) Label() string {
	if r.Title == "" { return r.Key }
	return r.Title
}

// Matches reports whether a lowercase search query finds this row.
func (r Row) Matches(query string) bool {
	for _, text := range append([]string{r.Title, r.Authors, r.Year, r.Key}, r.Tags...) {
		if strings.Contains(strings.ToLower(text), query) { return true }
	}
	return false
}

func RowOfPaper(rows []Row, id string) (Row, bool) {
	for _, row := range rows {
		if PaperFile(row.Key) == id { return row, true }
	}
	return Row{}, false
}

// Added is the outcome of adding a looked-up entry. Duplicate: it was in the
// library already, under Key.
type Added struct {
	Key       string
	Duplicate bool
}

// TagCount is a tag in use and its number of entries.
type TagCount struct {
	Name  string
	Count int
}

// FieldValue is a field as the editor holds it.
type FieldValue struct {
	Field Field
	Text  string
}

// InvalidLibrary is text Hayagriva or YAML refused as a library. Reason holds
// no path, so it can be shown beside the field that caused it.
type InvalidLibrary struct{ Path, Reason string }

func (e *InvalidLibrary) Error() string { return e.Path + " is not a valid library: " + e.Reason }

// Entry is one raw Hayagriva entry, without its key.
type Entry struct{ node *yaml.Node }

func NewEntry() Entry { return Entry{newMap()} }

// ParseEntry reads one entry's YAML mapping.
func ParseEntry(text string) (Entry, error) {
	node, err := parseMapping(text)
	if err != nil { return Entry{}, err }
	return Entry{node}, nil
}

func (e Entry) Node() *yaml.Node { return e.node }
func (e Entry) Read(slot Slot) string { return read(e.node, slot) }
func (e Entry) Write(slot Slot, text string) { write(e.node, slot, text) }

func (e Entry) YAML() string {
	text, _ := encode(e.node)
	return text
}

type Library struct {
	root, path string
	entries    *yaml.Node
	parsed     []ParsedEntry
}

// Load reads the vault's library. A file that does not parse is an error, so
// that nothing overwrites it.
func Load(root string) (*Library, error) {
	path := filepath.Join(root, vault.LibraryFile)
	text, err := readText(path)
	if err != nil { return nil, err }
	return parse(root, path, text)
}

// readText is "" for a missing file and an error for one that is not UTF-8.
func readText(path string) (string, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) { return "", nil }
	if err == nil && !utf8.Valid(data) { err = errors.New("stream did not contain valid UTF-8") }
	if err != nil { return "", fmt.Errorf("could not read %s: %w", path, err) }
	return string(data), nil
}

func parse(root, path, text string) (*Library, error) {
	invalid := func(err error) error { return &InvalidLibrary{Path: path, Reason: err.Error()} }
	entries, err := parseMapping(text)
	if err != nil { return nil, invalid(err) }
	var parsed []ParsedEntry
	if len(entries.Content) > 0 {
		if Hayagriva == nil { return nil, errors.New("library: Hayagriva is not set") }
		if err := Hayagriva.Check(text); err != nil { return nil, invalid(err) }
		if parsed, err = Hayagriva.Entries(text); err != nil { return nil, invalid(err) }
	}
	return &Library{root: root, path: path, entries: entries, parsed: parsed}, nil
}

func (l *Library) Versions() *vault.Versions { return vault.VersionsOf(l.root, vault.LibraryFile) }

// Text is the file as it stands.
func (l *Library) Text() string {
	data, _ := os.ReadFile(l.path)
	return string(data)
}

// keepVersion keeps the file as it stands before a change. A run of quick
// edits keeps the state before the first, unless now. A file that cannot be
// read cannot be kept, which stops the change.
func (l *Library) keepVersion(now bool) error {
	text, err := readText(l.path)
	if err != nil || text == "" { return err }
	if now { _, err = l.Versions().Keep(text) } else { _, err = l.Versions().KeepPaced(text) }
	return err
}

// Restore puts a version back, keeping what it replaces. It is checked first,
// so a version that does not parse changes nothing.
func (l *Library) Restore(text string) error { return l.replace(text, true) }

// commit validates entries with Hayagriva, then writes them.
func (l *Library) commit(entries *yaml.Node) error {
	text, err := encode(entries)
	if err != nil { return err }
	return l.replace(text, false)
}

// replace writes text over the file if it parses, keeping a version first.
func (l *Library) replace(text string, now bool) error {
	next, err := parse(l.root, l.path, text)
	if err != nil { return err }
	if err := l.keepVersion(now); err != nil { return err }
	if err := vault.WriteAtomic(l.path, []byte(text)); err != nil { return err }
	*l = *next
	return nil
}

func (l *Library) Len() int { return len(l.parsed) }

func (l *Library) Contains(key string) bool {
	for _, entry := range l.parsed {
		if entry.Key == key { return true }
	}
	return false
}

func (l *Library) entry(key string) *yaml.Node { return asMap(get(l.entries, key)) }

func (l *Library) PaperIDs(key string) PaperIDs {
	entry := l.entry(key)
	if entry == nil { return PaperIDs{} }
	return PaperIDs{DOI: read(entry, DOI.Slot), Arxiv: arxivID(entry)}
}

func (l *Library) Rows() []Row {
	rows := make([]Row, 0, len(l.parsed))
	for _, parsed := range l.parsed {
		row := Row{Key: parsed.Key, Title: parsed.Title, Year: year(parsed.Date)}
		if kind, ok := l.SourceType(parsed.Key); ok { row.Kind = kind.Label }
		names := make([]string, len(parsed.Authors))
		for i, name := range parsed.Authors { names[i], _, _ = strings.Cut(name, ",") }
		row.Authors = strings.Join(names, ", ")
		if entry := l.entry(parsed.Key); entry != nil { row.Tags = tags(entry) }
		rows = append(rows, row)
	}
	return rows
}

// year of a "YYYY[-MM[-DD]]" date, as a number prints.
func year(date string) string {
	if date == "" { return "" }
	end := strings.IndexByte(date[1:], '-') + 1
	if end == 0 { end = len(date) }
	n, err := strconv.Atoi(date[:end])
	if err != nil { return date[:end] }
	return strconv.Itoa(n)
}

// Tags is every tag in use with its number of entries, in natural order.
func (l *Library) Tags() []TagCount {
	counts := map[string]int{}
	for i := 1; i < len(l.entries.Content); i += 2 {
		if entry := asMap(l.entries.Content[i]); entry != nil {
			for _, tag := range tags(entry) { counts[tag]++ }
		}
	}
	list := make([]TagCount, 0, len(counts))
	for name, count := range counts { list = append(list, TagCount{name, count}) }
	sort.Slice(list, func(i, j int) bool { return vault.NaturalLess(list[i].Name, list[j].Name) })
	return list
}

// TagLike is the tag in use that name differs from only in case.
func (l *Library) TagLike(name string) (string, bool) {
	for _, tag := range l.Tags() {
		if same(tag.Name, name) { return tag.Name, true }
	}
	return "", false
}

// tagName is tag as it should be stored. A name that differs only in case from
// a tag in use, other than old, becomes that tag.
func (l *Library) tagName(tag string, old *string) (string, error) {
	tag = strings.TrimSpace(tag)
	if tag == "" { return "", errors.New("Enter a tag") }
	if strings.Contains(tag, ",") { return "", errors.New("Tags cannot contain commas") }
	for _, known := range l.Tags() {
		if (old == nil || known.Name != *old) && same(known.Name, tag) { return known.Name, nil }
	}
	return tag, nil
}

// SetTag adds tag to the entries in keys, or removes it.
func (l *Library) SetTag(keys []string, tag string, on bool) error {
	tag, err := l.tagName(tag, nil)
	if err != nil { return err }
	entries := clone(l.entries)
	for _, key := range keys {
		entry := asMap(get(entries, key))
		if entry == nil { continue }
		var next []string
		for _, name := range tags(entry) {
			if name != tag { next = append(next, name) }
		}
		if on { next = append(next, tag) }
		setTags(entry, next)
	}
	return l.commit(entries)
}

func (l *Library) SourceType(key string) (*SourceType, bool) {
	entry := l.entry(key)
	if entry == nil { return nil, false }
	return sourceTypeOf(entry), true
}

// FieldText is what the editor shows for a field of key.
func (l *Library) FieldText(key string, f Field) string {
	entry := l.entry(key)
	if entry == nil { return "" }
	return read(entry, f.Slot)
}

// AddBlank adds an empty entry of kind for the editor to fill in.
func (l *Library) AddBlank(kind *SourceType) (string, error) {
	entry := newMap()
	kind.apply(entry)
	key := l.freshKey(entry)
	entries := clone(l.entries)
	set(entries, key, entry)
	if err := l.commit(entries); err != nil { return "", err }
	return key, nil
}

// TypeChangeDropsParent: whether changing key to kind would lose the work it was published in.
func (l *Library) TypeChangeDropsParent(key string, kind *SourceType) bool {
	entry := l.entry(key)
	return entry != nil && kind.dropsParent(entry)
}

// SetSourceType changes key to kind, keeping every field the types share.
func (l *Library) SetSourceType(key string, kind *SourceType) error {
	entries := clone(l.entries)
	entry := asMap(get(entries, key))
	if entry == nil { return nil }
	kind.apply(entry)
	return l.commit(entries)
}

// SaveFields writes the fields that changed and renames the entry to newKey.
// Fields left alone keep their exact form, such as a person's prefix. An entry
// that would not parse is rejected unwritten. True if anything changed.
func (l *Library) SaveFields(key, newKey string, fields []FieldValue) (bool, error) {
	newKey = strings.TrimSpace(newKey)
	if newKey == "" { return false, errors.New("Enter a citation key") }
	if strings.ContainsFunc(newKey, unicode.IsSpace) { return false, errors.New("Citation keys cannot contain spaces") }
	if key != newKey && l.Contains(newKey) { return false, fmt.Errorf("%s is already in the library", newKey) }
	original := l.entry(key)
	if original == nil { return false, nil }
	entry := clone(original)
	for _, f := range fields {
		if read(original, f.Field.Slot) != strings.TrimSpace(f.Text) { write(entry, f.Field.Slot, f.Text) }
	}
	if key == newKey && equal(entry, original) { return false, nil }
	entries := clone(l.entries)
	for i := 0; i+1 < len(entries.Content); i += 2 {
		if entries.Content[i].Value != key { continue }
		if key != newKey { entries.Content[i] = newStr(newKey) }
		entries.Content[i+1] = entry
	}
	if err := l.commit(entries); err != nil { return false, err }
	return true, nil
}

// RenameTag renames a tag on every entry. Renaming to a tag in use merges the two.
func (l *Library) RenameTag(old, new string) error {
	new, err := l.tagName(new, &old)
	if err != nil { return err }
	return l.changeTag(old, &new)
}

func (l *Library) DeleteTag(tag string) error { return l.changeTag(tag, nil) }

func (l *Library) changeTag(old string, new *string) error {
	entries := clone(l.entries)
	for i := 1; i < len(entries.Content); i += 2 {
		entry := asMap(entries.Content[i])
		if entry == nil || !contains(tags(entry), old) { continue }
		var next []string
		for _, tag := range tags(entry) {
			if tag == old {
				if new == nil { continue }
				tag = *new
			}
			if !contains(next, tag) { next = append(next, tag) }
		}
		setTags(entry, next)
	}
	return l.commit(entries)
}

func (l *Library) DeleteEntries(keys []string) error {
	entries := clone(l.entries)
	for _, key := range keys { set(entries, key, nil) }
	return l.commit(entries)
}

// ImportBiblatex imports BibLaTeX text. Keys already in the library are kept as
// they are. Returns the number of entries added and skipped.
func (l *Library) ImportBiblatex(text string) (added, skipped int, err error) {
	incoming, err := importBiblatex(text)
	if err != nil { return 0, 0, err }
	entries := clone(l.entries)
	for _, in := range incoming {
		if has(entries, in.key) || l.findDuplicate(in.entry) != "" { skipped++; continue }
		set(entries, in.key, in.entry)
		added++
	}
	if added > 0 {
		if err := l.commit(entries); err != nil { return 0, 0, err }
	}
	return added, skipped, nil
}

// AddFetched adds a looked-up entry under a generated key, unless an entry with
// the same DOI, PMID, ISBN, or arXiv ID is here already.
func (l *Library) AddFetched(e Entry) (Added, error) {
	if key := l.findDuplicate(e.node); key != "" { return Added{Key: key, Duplicate: true}, nil }
	entry := clone(e.node)
	key := l.freshKey(entry)
	if read(entry, Slot{Kind: SlotType}) == "" { write(entry, Slot{Kind: SlotType}, typeMisc) }
	entries := clone(l.entries)
	set(entries, key, entry)
	if err := l.commit(entries); err != nil { return Added{}, err }
	return Added{Key: key}, nil
}

func (l *Library) findDuplicate(entry *yaml.Node) string {
	wanted := workIDs(entry)
	for i := 0; i+1 < len(l.entries.Content); i += 2 {
		existing := asMap(l.entries.Content[i+1])
		if existing == nil { continue }
		for _, id := range workIDs(existing) {
			if contains(wanted, id) { return l.entries.Content[i].Value }
		}
	}
	return ""
}

// freshKey is surname + year, with a letter added when the key is taken.
func (l *Library) freshKey(entry *yaml.Node) string {
	surname, year := asciiKey(firstSurname(entry)), entryYear(entry)
	base := surname + year
	if surname == "" { base = "entry" + year }
	if !l.Contains(base) { return base }
	for letter := 'b'; letter <= 'z'; letter++ {
		if key := base + string(letter); !l.Contains(key) { return key }
	}
	for n := 2; ; n++ {
		if key := fmt.Sprintf("%s-%d", base, n); !l.Contains(key) { return key }
	}
}

func (l *Library) ExportBiblatex() string { return export(l.parsed, l.entries) }

// Subset is YAML for the entries in keys, in library order. Tags stay out: this
// goes into documents, and a new tag should not rewrite them.
func (l *Library) Subset(keys []string) (string, error) {
	entries := newMap()
	for i := 0; i+1 < len(l.entries.Content); i += 2 {
		if !contains(keys, l.entries.Content[i].Value) { continue }
		entry := clone(l.entries.Content[i+1])
		if m := asMap(entry); m != nil { setTags(m, nil) }
		entries.Content = append(entries.Content, clone(l.entries.Content[i]), entry)
	}
	return encode(entries)
}

var keyFolds = map[rune]string{
	'ä': "ae", 'æ': "ae", 'ö': "oe", 'œ': "oe", 'ü': "ue", 'ß': "ss",
	'à': "a", 'á': "a", 'â': "a", 'ã': "a", 'å': "a", 'ç': "c", 'č': "c", 'ć': "c",
	'è': "e", 'é': "e", 'ê': "e", 'ë': "e", 'ì': "i", 'í': "i", 'î': "i", 'ï': "i",
	'ñ': "n", 'ń': "n", 'ò': "o", 'ó': "o", 'ô': "o", 'õ': "o", 'ø': "o",
	'š': "s", 'ś': "s", 'ù': "u", 'ú': "u", 'û': "u", 'ý': "y", 'ÿ': "y", 'ž': "z", 'ź': "z",
}

// asciiKey is lowercase ASCII for a citation key, folding common accented letters.
func asciiKey(text string) string {
	var key strings.Builder
	for _, c := range strings.ToLower(text) {
		if fold, ok := keyFolds[c]; ok {
			key.WriteString(fold)
		} else if c < utf8.RuneSelf && (unicode.IsLetter(c) || unicode.IsDigit(c)) {
			key.WriteRune(c)
		}
	}
	return key.String()
}
