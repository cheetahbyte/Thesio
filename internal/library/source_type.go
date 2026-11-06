package library

import (
	"strconv"
	"strings"

	"gopkg.in/yaml.v3"
)

// The kinds of source the Library knows, and which fields each one has. A kind
// is a Hayagriva type plus the type of its parent, so library.yaml stays plain
// Hayagriva and entries from any tool get a kind without a new field.

// SlotKind says where a field's value lives in an entry.
type SlotKind int

const (
	SlotType       SlotKind = iota
	SlotParentType          // the Hayagriva type of the work this one was published in
	SlotOwn
	SlotParent // on the work this one was published in
	SlotEither // on the entry or its parent, whichever has it; lookups put these on the parent
	SlotSerial
	SlotPersons
	SlotDate
	SlotPublisher
	SlotPlace // with the publisher, or on the entry without one
	SlotURL
	SlotAccessed
)

// Slot is where a field's value lives; Name is the key for the kinds that take one.
type Slot struct {
	Kind SlotKind
	Name string
}

// Field is one row of the entry editor. Biblatex is its name in BibLaTeX, ""
// where BibLaTeX has no place for it; export and import read it from here.
type Field struct {
	Label, Hint string
	Long        bool // wraps over several lines in the editor
	Slot        Slot
	Biblatex    string
}

// BiblatexName is the field's name in an entry of BibLaTeX type kind.
func (f Field) BiblatexName(kind string) string {
	switch {
	case f.Biblatex == "booktitle" && kind == "article":
		return "journaltitle"
	case f.Biblatex == "organization" && (kind == "thesis" || kind == "report"):
		return "institution"
	}
	return f.Biblatex
}

// BibLaTeX's field for tags, and the one that says what eprint holds.
const (
	keywordsField  = "keywords"
	eprintType     = "eprinttype"
	eprintTypeName = "arxiv"
)

func field(label, hint string, slot Slot, biblatex string) Field { return Field{Label: label, Hint: hint, Slot: slot, Biblatex: biblatex} }
func wrapped(label string, slot Slot, biblatex string) Field { return Field{Label: label, Long: true, Slot: slot, Biblatex: biblatex} }
func unexported(label, hint string, slot Slot) Field { return Field{Label: label, Hint: hint, Slot: slot} }

func own(name string) Slot { return Slot{SlotOwn, name} }
func serial(kind string) Slot { return Slot{SlotSerial, kind} }

var workIDKinds = []string{"doi", "arxiv", "pmid", "isbn"}

// A bare serial-number counts as this kind of number.
const plainSerial = "serial"

var (
	Title     = wrapped("Title", own("title"), "title")
	Authors   = wrapped("Authors", Slot{SlotPersons, "author"}, "author")
	editors   = wrapped("Editors", Slot{SlotPersons, "editor"}, "editor")
	Date      = field("Date", "2024 or 2024-03-15", Slot{Kind: SlotDate}, "date")
	Publisher = field("Publisher", "", Slot{Kind: SlotPublisher}, "publisher")
	place     = field("Place", "", Slot{Kind: SlotPlace}, "location")
	volume    = field("Volume", "", Slot{SlotEither, "volume"}, "volume")
	issue     = field("Issue", "", Slot{SlotEither, "issue"}, "number")
	edition   = field("Edition", "", own("edition"), "edition")
	Pages     = field("Pages", "891-921", own("page-range"), "pages")
	pageCount = field("Pages", "", own("page-total"), "pagetotal")
	number    = field("Number", "", serial(plainSerial), "number")
	runtime   = unexported("Running Time", "01:30:00", own("runtime"))
	DOI       = field("DOI", "", serial("doi"), "doi")
	ISBN      = field("ISBN", "", serial("isbn"), "isbn")
	issn      = field("ISSN", "", serial("issn"), "issn")
	Arxiv     = field("arXiv ID", "2112.12331", serial("arxiv"), "eprint")
	Version   = field("Version", "", serial("version"), "version")
	URL       = wrapped("URL", Slot{Kind: SlotURL}, "url")
	Accessed  = field("Accessed", "2024-03-15", Slot{Kind: SlotAccessed}, "urldate")
	Language  = field("Language", "en", own("language"), "language")
	abstract  = wrapped("Abstract", own("abstract"), "abstract")
	note      = wrapped("Note", own("note"), "note")
)

var (
	pmidSlot     = serial("pmid")
	parentTitle  = Slot{SlotParent, "title"}
	parentVolume = Slot{SlotParent, "volume"}
	parentIssue  = Slot{SlotParent, "issue"}
	archiveSlot  = own("archive")
	genreSlot    = own("genre")
	orgSlot      = own("organization")
)

func container(label string) Field { return wrapped(label, parentTitle, "booktitle") }
func genre(hint string) Field { return field("Type", hint, genreSlot, "type") }
func organization(label string) Field { return field(label, "", orgSlot, "organization") }
func person(label string) Field { return field(label, "", Authors.Slot, "author") }

// Hayagriva's names for the types this package writes.
const (
	typeArticle     = "Article"
	typeBook        = "Book"
	typeChapter     = "Chapter"
	typePeriodical  = "Periodical"
	typeRepository  = "Repository"
	typeWeb         = "Web"
	typeMisc        = "Misc"
	typeProceedings = "Proceedings"
)

// Each Hayagriva type, the type this table knows it by, and its BibLaTeX type.
// @incollection imports as an anthos in an anthology, which is a Book Section
// here. A type not listed stands for itself and exports as misc.
var typeNames = [][3]string{
	{typeArticle, typeArticle, "article"}, {typeBook, typeBook, "book"}, {typeChapter, typeChapter, "inbook"}, {"Anthos", typeChapter, "incollection"},
	{"Anthology", typeBook, "collection"}, {typeProceedings, typeProceedings, "proceedings"}, {"Conference", typeProceedings, "misc"},
	{"Report", "Report", "report"}, {"Thesis", "Thesis", "thesis"}, {typeWeb, typeWeb, "online"}, {"Manuscript", "Manuscript", "unpublished"},
	{"Patent", "Patent", "patent"}, {typeRepository, typeRepository, "software"}, {"Entry", "Entry", "inreference"}, {"Video", "Video", "video"}, {"Audio", "Audio", "audio"},
}

// SourceType is a kind of source in the type menu.
type SourceType struct {
	Label  string
	Fields []Field
	entry  string
	parent string // "" for none
	// The genre that marks an entry as this type, when nothing else tells it from its siblings.
	genre string
}

func kind(label, entry, parent string, fields ...Field) *SourceType {
	return &SourceType{Label: label, Fields: fields, entry: entry, parent: parent}
}

const preprintGenre = "Preprint"

var (
	JournalArticle = kind("Journal Article", typeArticle, typePeriodical, Title, Authors, container("Publication"), Date, volume, issue, Pages, DOI, issn, URL, Accessed, Language, abstract, note)
	Preprint       = &SourceType{Label: "Preprint", entry: typeArticle, genre: preprintGenre, Fields: []Field{Title, Authors, unexported("Repository", "arXiv", archiveSlot), Date, Arxiv, DOI, URL, Accessed, Language, abstract, note}}
	Other          = kind("Other", typeMisc, "", Title, Authors, Date, Publisher, place, Pages, DOI, URL, Accessed, Language, abstract, note)
)

// SourceTypes is in the order of the type menu. Among types sharing a
// Hayagriva type, an entry whose parent matches none of them gets the first.
var SourceTypes = []*SourceType{
	JournalArticle,
	kind("Book", typeBook, "", Title, Authors, editors, Date, Publisher, place, edition, volume, pageCount, ISBN, DOI, URL, Accessed, Language, abstract, note),
	kind("Book Section", typeChapter, typeBook, Title, Authors, container("Book Title"), editors, Date, Publisher, place, edition, volume, Pages, ISBN, DOI, URL, Accessed, Language, abstract, note),
	kind("Conference Paper", typeArticle, typeProceedings, Title, Authors, container("Proceedings"), Date, Publisher, place, volume, Pages, ISBN, DOI, URL, Accessed, Language, abstract, note),
	Preprint,
	kind("Thesis", "Thesis", "", Title, person("Author"), genre("PhD thesis"), organization("University"), place, Date, pageCount, DOI, URL, Accessed, Language, abstract, note),
	kind("Report", "Report", "", Title, Authors, genre("Technical report"), number, organization("Institution"), place, Date, pageCount, DOI, ISBN, URL, Accessed, Language, abstract, note),
	kind("Web Page", typeWeb, typeWeb, Title, Authors, container("Website"), Date, URL, Accessed, Language, abstract, note),
	kind("Blog Post", typeArticle, "Blog", Title, Authors, container("Blog"), Date, URL, Accessed, Language, abstract, note),
	kind("Newspaper Article", typeArticle, "Newspaper", Title, Authors, container("Newspaper"), Date, place, Pages, URL, Accessed, Language, abstract, note),
	kind("Encyclopedia Entry", "Entry", "Reference", Title, Authors, container("Encyclopedia"), editors, Date, Publisher, place, edition, volume, Pages, ISBN, URL, Accessed, Language, note),
	kind("Software, Data", typeRepository, "", Title, Authors, Version, Date, field("Publisher", "GitHub", Publisher.Slot, "publisher"), DOI, URL, Accessed, abstract, note),
	kind("Video", "Video", "", Title, person("Director"), Date, Publisher, runtime, DOI, URL, Accessed, Language, abstract, note),
	kind("Audio", "Audio", "", Title, Authors, Date, Publisher, runtime, DOI, URL, Accessed, Language, abstract, note),
	kind("Patent", "Patent", "", Title, person("Inventors"), field("Patent Number", "", number.Slot, "number"), organization("Assignee"), Date, URL, Accessed, abstract, note),
	kind("Legal Case", "Case", "", field("Case Name", "", Title.Slot, "title"), organization("Court"), Date, number, URL, Accessed, abstract, note),
	kind("Legislation", "Legislation", "", field("Name of Act", "", Title.Slot, "title"), Date, number, Pages, URL, Accessed, abstract, note),
	kind("Manuscript", "Manuscript", "", Title, Authors, genre(""), organization("Institution"), place, Date, pageCount, URL, Accessed, Language, abstract, note),
	Other,
}

// AllFields is every field of every type, each slot once.
func AllFields() []Field {
	var fields []Field
	seen := map[Slot]bool{}
	for _, kind := range SourceTypes {
		for _, f := range kind.Fields {
			if !seen[f.Slot] { seen[f.Slot] = true; fields = append(fields, f) }
		}
	}
	return fields
}

// same compares as Rust's eq_ignore_ascii_case.
func same(a, b string) bool {
	if len(a) != len(b) { return false }
	for i := 0; i < len(a); i++ {
		if lowerASCII(a[i]) != lowerASCII(b[i]) { return false }
	}
	return true
}

func lowerASCII(b byte) byte {
	if 'A' <= b && b <= 'Z' { return b + 'a' - 'A' }
	return b
}

// arxivID is the arXiv ID an entry states, or that its link holds.
func arxivID(entry *yaml.Node) string {
	if stated := read(entry, Arxiv.Slot); stated != "" { return stated }
	return ArxivOfLink(read(entry, URL.Slot))
}

// An article with no journal is a preprint only if something says so. Most
// are journal articles still missing their journal.
func isPreprint(entry *yaml.Node) bool {
	return arxivID(entry) != "" || read(entry, archiveSlot) != "" || read(entry, genreSlot) == preprintGenre
}

// knownAs is the type this table knows a Hayagriva type by.
func knownAs(kind string) string {
	for _, t := range typeNames {
		if same(t[0], kind) { return t[1] }
	}
	return kind
}

// biblatexType is an entry's BibLaTeX type. An article without a parent may be anything.
func biblatexType(entry *yaml.Node) string {
	kind := read(entry, Slot{Kind: SlotType})
	if same(kind, typeArticle) {
		parent := parentOf(entry)
		if parent == nil { return "misc" }
		if same(knownAs(text(get(parent, "type"))), typeProceedings) { return "inproceedings" }
		return "article"
	}
	for _, t := range typeNames {
		if same(t[0], kind) { return t[2] }
	}
	return "misc"
}

// parentOf is the parent an entry's fields go on. Hayagriva also allows a list of them.
func parentOf(entry *yaml.Node) *yaml.Node {
	parent := get(entry, "parent")
	if parent != nil && parent.Kind == yaml.SequenceNode {
		if len(parent.Content) == 0 { return nil }
		return asMap(parent.Content[0])
	}
	return asMap(parent)
}

// ensureParent is the entry's parent, made if it has none.
func ensureParent(entry *yaml.Node) *yaml.Node {
	if parentOf(entry) == nil { set(entry, "parent", newMap()) }
	return parentOf(entry)
}

// SourceTypeOf is the type of a raw entry. Any Hayagriva type this table lacks is Other.
func SourceTypeOf(e Entry) *SourceType { return sourceTypeOf(e.node) }

func sourceTypeOf(entry *yaml.Node) *SourceType {
	kind := knownAs(read(entry, Slot{Kind: SlotType}))
	parent, hasParent := "", false
	if p := parentOf(entry); p != nil {
		if t := get(p, "type"); t != nil && t.Kind == yaml.ScalarNode && t.ShortTag() == "!!str" { parent, hasParent = knownAs(t.Value), true }
	}
	var first *SourceType
	for _, t := range SourceTypes {
		if !same(t.entry, kind) { continue }
		if first == nil { first = t }
		switch {
		case t.parent != "" && hasParent:
			if same(t.parent, parent) { return t }
		case t.parent == "" && !hasParent:
			if t.genre == "" || isPreprint(entry) { return t }
		}
	}
	if first != nil { return first }
	return Other
}

// DropsParent: whether changing e to this type would lose what its parent holds.
func (t *SourceType) DropsParent(e Entry) bool { return t.dropsParent(e.node) }

func (t *SourceType) dropsParent(entry *yaml.Node) bool {
	parent := parentOf(entry)
	if t.parent != "" || parent == nil { return false }
	for _, name := range keys(parent) {
		if name != "type" { return true }
	}
	return false
}

// Apply makes e this type. A parent this type has no use for goes with the old
// type, or it would be cited but could not be edited.
func (t *SourceType) Apply(e Entry) { t.apply(e.node) }

func (t *SourceType) apply(entry *yaml.Node) {
	write(entry, Slot{Kind: SlotType}, t.entry)
	if read(entry, genreSlot) == preprintGenre { write(entry, genreSlot, "") }
	if t.genre != "" { write(entry, genreSlot, t.genre) }
	if t.parent != "" { write(entry, Slot{Kind: SlotParentType}, t.parent) } else { set(entry, "parent", nil) }
}

// publisherOf is the publisher, which a chapter's import leaves on its book.
func publisherOf(entry *yaml.Node) *yaml.Node {
	if p := get(entry, "publisher"); p != nil { return p }
	return get(parentOf(entry), "publisher")
}

func personName(n *yaml.Node) (string, bool) {
	n = deref(n)
	if n == nil { return "", false }
	if n.Kind == yaml.MappingNode {
		name := get(n, "name")
		if name == nil || name.Kind != yaml.ScalarNode || name.ShortTag() != "!!str" { return "", false }
		if given := get(n, "given-name"); given != nil && given.Kind == yaml.ScalarNode && given.ShortTag() == "!!str" { return name.Value + ", " + given.Value, true }
		return name.Value, true
	}
	if n.Kind == yaml.ScalarNode && n.ShortTag() == "!!str" { return n.Value, true }
	return "", false
}

// persons are the names in a Persons slot, each "Family, Given".
func persons(entry *yaml.Node, slot Slot) []string {
	if slot.Kind != SlotPersons { return nil }
	value := get(entry, slot.Name)
	if value == nil { return nil }
	items := []*yaml.Node{value}
	if value.Kind == yaml.SequenceNode { items = value.Content }
	var names []string
	for _, item := range items {
		if name, ok := personName(item); ok { names = append(names, name) }
	}
	return names
}

func firstSurname(entry *yaml.Node) string {
	names := persons(entry, Authors.Slot)
	if len(names) == 0 { return "" }
	words := strings.Fields(strings.SplitN(names[0], ",", 2)[0])
	if len(words) == 0 { return "" }
	return words[len(words)-1]
}

func entryYear(entry *yaml.Node) string {
	date := get(entry, "date")
	if isNumber(date) { return text(date) }
	runes := []rune(text(date))
	return string(runes[:min(4, len(runes))])
}

// tags are an entry's tags, which only Thesio reads. A hand-written `tags: reading` counts as one.
func tags(entry *yaml.Node) []string {
	value := get(entry, "tags")
	if value == nil { return nil }
	items := []*yaml.Node{value}
	if value.Kind == yaml.SequenceNode { items = value.Content }
	var names []string
	for _, item := range items {
		if name, ok := scalar(item); ok { names = append(names, name) }
	}
	return names
}

func setTags(entry *yaml.Node, names []string) {
	if len(names) == 0 { set(entry, "tags", nil); return }
	items := make([]*yaml.Node, len(names))
	for i, name := range names { items[i] = newStr(name) }
	set(entry, "tags", newSeq(items))
}

// workIDs are the identifiers that name one work, as kind and number, lowercased
// for comparing. An ISSN or a version number is shared by many. YAML reads a
// bare arXiv ID as a number.
func workIDs(entry *yaml.Node) [][2]string {
	serial := asMap(get(entry, "serial-number"))
	if serial == nil { return nil }
	var ids [][2]string
	for i := 0; i+1 < len(serial.Content); i += 2 {
		k := serial.Content[i]
		if k.Kind != yaml.ScalarNode || k.ShortTag() != "!!str" { continue }
		kind := strings.ToLower(k.Value)
		number, ok := scalar(serial.Content[i+1])
		if !contains(workIDKinds, kind) || !ok || number == "" { continue }
		ids = append(ids, [2]string{kind, strings.ToLower(number)})
	}
	return ids
}

func contains[T comparable](list []T, item T) bool {
	for _, x := range list {
		if x == item { return true }
	}
	return false
}

// read is a field's value as the editor shows it.
func read(entry *yaml.Node, slot Slot) string {
	switch slot.Kind {
	case SlotType:
		return text(get(entry, "type"))
	case SlotParentType:
		return text(get(parentOf(entry), "type"))
	case SlotOwn:
		return text(get(entry, slot.Name))
	case SlotParent:
		return text(get(parentOf(entry), slot.Name))
	case SlotEither:
		if v := get(entry, slot.Name); v != nil { return text(v) }
		return text(get(parentOf(entry), slot.Name))
	case SlotSerial:
		value := get(entry, "serial-number")
		if value != nil && value.Kind == yaml.MappingNode { return text(get(value, slot.Name)) }
		if slot.Name == plainSerial { return text(value) }
		return ""
	case SlotPersons:
		return strings.Join(persons(entry, slot), "; ")
	case SlotDate:
		return text(get(entry, "date"))
	case SlotPublisher:
		publisher := publisherOf(entry)
		if publisher != nil && publisher.Kind == yaml.MappingNode { return text(get(publisher, "name")) }
		return text(publisher)
	case SlotPlace:
		if location := get(publisherOf(entry), "location"); location != nil { return text(location) }
		return text(get(entry, "location"))
	case SlotURL:
		url := get(entry, "url")
		if url != nil && url.Kind == yaml.MappingNode { return text(get(url, "value")) }
		return text(url)
	case SlotAccessed:
		return text(get(get(entry, "url"), "date"))
	}
	return ""
}

func valueOf(new string) *yaml.Node {
	if new = strings.TrimSpace(new); new != "" { return newStr(new) }
	return nil
}

func setURL(entry *yaml.Node, url, accessed string) {
	value := valueOf(url)
	if value != nil {
		if date := valueOf(accessed); date != nil {
			m := newMap()
			set(m, "value", value)
			set(m, "date", date)
			value = m
		}
	}
	set(entry, "url", value)
}

// write writes one field. Validation is left to the commit, which runs Hayagriva.
func write(entry *yaml.Node, slot Slot, new string) {
	switch slot.Kind {
	case SlotType:
		set(entry, "type", valueOf(new))
	case SlotParentType:
		set(ensureParent(entry), "type", valueOf(new))
	case SlotOwn:
		set(entry, slot.Name, valueOf(new))
	case SlotParent:
		set(ensureParent(entry), slot.Name, valueOf(new))
	case SlotEither:
		if parent := parentOf(entry); parent != nil && has(parent, slot.Name) { set(parent, slot.Name, valueOf(new)) } else { set(entry, slot.Name, valueOf(new)) }
	case SlotSerial:
		var serial *yaml.Node
		switch old := get(entry, "serial-number"); {
		case old != nil && old.Kind == yaml.MappingNode:
			serial = clone(old)
		case old != nil:
			serial = newMap()
			set(serial, plainSerial, clone(old))
		default:
			serial = newMap()
		}
		set(serial, slot.Name, valueOf(new))
		if len(serial.Content) == 0 { serial = nil }
		set(entry, "serial-number", serial)
	case SlotPersons:
		var items []*yaml.Node
		for _, name := range strings.Split(new, ";") {
			if v := valueOf(name); v != nil { items = append(items, v) }
		}
		if len(items) == 0 { set(entry, slot.Name, nil) } else { set(entry, slot.Name, newSeq(items)) }
	case SlotDate:
		// A bare year is a number in Hayagriva files.
		value := valueOf(new)
		if value != nil {
			if year, err := strconv.ParseInt(value.Value, 10, 64); err == nil { value.Tag, value.Value = "!!int", strconv.FormatInt(year, 10) }
		}
		set(entry, "date", value)
	case SlotPublisher, SlotPlace:
		if parent := parentOf(entry); !has(entry, "publisher") && parent != nil && has(parent, "publisher") { write(parent, slot, new); return }
		publisher := get(entry, "publisher")
		if slot.Kind == SlotPublisher {
			if publisher != nil && publisher.Kind == yaml.MappingNode && valueOf(new) != nil { set(publisher, "name", valueOf(new)) } else { set(entry, "publisher", valueOf(new)) }
			return
		}
		switch {
		case publisher != nil && publisher.Kind == yaml.MappingNode:
			set(publisher, "location", valueOf(new))
		case publisher != nil && valueOf(new) != nil:
			m := newMap()
			set(m, "name", clone(publisher))
			set(m, "location", valueOf(new))
			set(entry, "publisher", m)
		default:
			set(entry, "location", valueOf(new))
		}
	case SlotURL:
		setURL(entry, new, read(entry, Slot{Kind: SlotAccessed}))
	case SlotAccessed:
		setURL(entry, read(entry, URL.Slot), new)
	}
}
