package library

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"sort"
	"strings"
	"testing"

	"github.com/cheetahbyte/thesio/internal/vault"
	"gopkg.in/yaml.v3"
)

// fakeHayagriva stands in for the Typst library: just enough Hayagriva for these tests.
type fakeHayagriva struct{}

var dateShape = regexp.MustCompile(`^\d{4}(-\d{2}(-\d{2})?)?$`)

func (fakeHayagriva) Check(yamlText string) error {
	entries, err := parseMapping(yamlText)
	if err != nil { return err }
	for i := 0; i+1 < len(entries.Content); i += 2 {
		entry := asMap(entries.Content[i+1])
		if entry == nil { return fmt.Errorf("%s: an entry must be a mapping", entries.Content[i].Value) }
		if date := get(entry, "date"); date != nil && !dateShape.MatchString(text(date)) { return fmt.Errorf("%s: invalid date %q", entries.Content[i].Value, text(date)) }
	}
	return nil
}

func (fakeHayagriva) Entries(text string) ([]ParsedEntry, error) {
	entries, err := parseMapping(text)
	if err != nil { return nil, err }
	var list []ParsedEntry
	for i := 0; i+1 < len(entries.Content); i += 2 {
		entry := asMap(entries.Content[i+1])
		list = append(list, ParsedEntry{Key: entries.Content[i].Value, Title: read(entry, Title.Slot), Authors: persons(entry, Authors.Slot), Date: read(entry, Date.Slot), ParentTitle: read(entry, parentTitle)})
	}
	return list, nil
}

type bibRecord struct {
	kind, key string
	fields    map[string]string
}

// scanBibtex reads @kind{key, name = {value}, ...} records, nothing more.
func scanBibtex(text string) []bibRecord {
	var records []bibRecord
	for {
		at := strings.IndexByte(text, '@')
		if at < 0 { return records }
		text = text[at+1:]
		open := strings.IndexByte(text, '{')
		comma := strings.IndexByte(text, ',')
		record := bibRecord{kind: strings.ToLower(strings.TrimSpace(text[:open])), key: strings.TrimSpace(text[open+1 : comma]), fields: map[string]string{}}
		text = text[comma+1:]
		for {
			text = strings.TrimLeft(text, " \t\r\n,")
			if text == "" || text[0] == '}' { break }
			eq := strings.IndexByte(text, '=')
			name := strings.ToLower(strings.TrimSpace(text[:eq]))
			text = strings.TrimLeft(text[eq+1:], " \t\r\n")
			var value string
			switch text[0] {
			case '{':
				depth, i := 0, 0
				for ; i < len(text); i++ {
					if text[i] == '\\' { i++; continue }
					if text[i] == '{' { depth++ } else if text[i] == '}' { depth--; if depth == 0 { break } }
				}
				value, text = text[1:i], text[i+1:]
			case '"':
				end := strings.IndexByte(text[1:], '"') + 1
				value, text = text[1:end], text[end+1:]
			default:
				end := strings.IndexAny(text, ",}")
				value, text = strings.TrimSpace(text[:end]), text[end:]
			}
			record.fields[name] = strings.NewReplacer("\\{", "{", "\\}", "}").Replace(value)
		}
		records = append(records, record)
	}
}

func personNode(name string) *yaml.Node {
	family, given, ok := strings.Cut(name, ",")
	if !ok {
		words := strings.Fields(name)
		family, given = words[len(words)-1], strings.Join(words[:len(words)-1], " ")
	}
	person := newMap()
	set(person, "name", newStr(strings.TrimSpace(family)))
	if given = strings.TrimSpace(given); given != "" { set(person, "given-name", newStr(given)) }
	return person
}

var bibTypes = map[string][2]string{
	"article": {typeArticle, ""}, "inproceedings": {typeArticle, typeProceedings}, "book": {typeBook, ""}, "collection": {"Anthology", ""},
	"inbook": {typeChapter, typeBook}, "incollection": {"Anthos", "Anthology"}, "proceedings": {typeProceedings, ""}, "report": {"Report", ""},
	"techreport": {"Report", ""}, "thesis": {"Thesis", ""}, "phdthesis": {"Thesis", ""}, "mastersthesis": {"Thesis", ""}, "online": {typeWeb, ""},
	"unpublished": {"Manuscript", ""}, "patent": {"Patent", ""}, "inreference": {"Entry", "Reference"}, "video": {"Video", ""}, "audio": {"Audio", ""},
}

func (fakeHayagriva) ImportBiblatex(bibtex string) (BiblatexImport, error) {
	out := BiblatexImport{Verbatim: map[string]map[string]string{}}
	entries := newMap()
	for _, r := range scanBibtex(bibtex) {
		f := r.fields
		kinds, ok := bibTypes[r.kind]
		if !ok { kinds = [2]string{typeMisc, ""} }
		journal := f["journaltitle"] + f["journal"]
		if r.kind == "article" && journal != "" { kinds[1] = typePeriodical }
		entry := newMap()
		write(entry, Slot{Kind: SlotType}, kinds[0])
		write(entry, Title.Slot, f["title"])
		for _, role := range []string{"author", "editor"} {
			if f[role] == "" { continue }
			var people []*yaml.Node
			for _, name := range strings.Split(f[role], " and ") { people = append(people, personNode(name)) }
			set(entry, role, newSeq(people))
		}
		write(entry, Date.Slot, f["date"]+f["year"])
		parent := entry
		if kinds[1] != "" {
			write(entry, Slot{Kind: SlotParentType}, kinds[1])
			write(entry, parentTitle, journal+f["booktitle"])
			parent = parentOf(entry)
		}
		on := func(target *yaml.Node, name, value string) { if value != "" { set(target, name, newStr(value)) } }
		periodical := kinds[1] == typePeriodical
		if periodical { on(parent, "volume", f["volume"]); on(parent, "issue", f["number"]) } else { on(entry, "volume", f["volume"]); write(entry, number.Slot, f["number"]) }
		publisher := entry
		if r.kind == "inbook" || r.kind == "incollection" { publisher = parent }
		if f["publisher"] != "" && f["address"] != "" {
			m := newMap()
			on(m, "name", f["publisher"])
			on(m, "location", f["address"])
			set(publisher, "publisher", m)
		} else {
			on(publisher, "publisher", f["publisher"])
			on(entry, "location", f["address"]+f["location"])
		}
		on(entry, "page-range", strings.ReplaceAll(f["pages"], "--", "-"))
		on(entry, "page-total", f["pagetotal"])
		on(entry, "edition", f["edition"])
		on(entry, "genre", f["type"])
		on(entry, "organization", f["school"]+f["institution"]+f["organization"])
		on(entry, "note", f["note"])
		on(entry, "abstract", f["abstract"])
		for _, id := range [][2]string{{"doi", "doi"}, {"isbn", "isbn"}, {"issn", "issn"}, {"eprint", "arxiv"}, {"version", "version"}} { write(entry, serial(id[1]), f[id[0]]) }
		write(entry, URL.Slot, f["url"])
		write(entry, Accessed.Slot, f["urldate"])
		set(entries, r.key, entry)
		out.Verbatim[r.key] = map[string]string{"keywords": f["keywords"], "language": f["language"]}
	}
	text, err := encode(entries)
	out.YAML = text
	return out, err
}

func TestMain(m *testing.M) {
	Hayagriva = fakeHayagriva{}
	os.Exit(m.Run())
}

func library(t *testing.T) *Library {
	l, err := Load(t.TempDir())
	if err != nil { t.Fatal(err) }
	return l
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil { t.Fatal(err) }
}

func importing(t *testing.T, l *Library, text string) (int, int) {
	t.Helper()
	added, skipped, err := l.ImportBiblatex(text)
	must(t, err)
	return added, skipped
}

func tagsOf(t *testing.T, l *Library, key string) []string {
	if row, ok := rowOf(l, key); ok { return row.Tags }
	t.Fatalf("no row %s", key)
	return nil
}

func rowOf(l *Library, key string) (Row, bool) {
	for _, row := range l.Rows() {
		if row.Key == key { return row, true }
	}
	return Row{}, false
}

const zoteroExport = `
@article{parry2022, title = {Flaky}, author = {Parry, Owain}, journal = {TOSEM}, volume = {31}, number = {1}, pages = {1--74},
  year = {2022}, doi = {10.1145/3476105}, issn = {1049-331X}, language = {en}, keywords = {testing, flaky tests}}
@book{knuth1984, title = {TeXbook}, author = {Knuth, Donald}, editor = {Doe, Jane}, publisher = {AW}, address = {Reading},
  edition = {2}, isbn = {0-201-13447-0}, year = {1984}, note = {A note}}
@incollection{chap2020, title = {Chapter}, booktitle = {Big Book}, publisher = {Pub}, author = {Doe, A.}, year = {2020}}
@phdthesis{phd2020, title = {On Tests}, type = {PhD thesis}, school = {TU Berlin}, author = {Doe, Jane}, year = {2020}}
@misc{arx2021, title = {Preprint}, author = {Doe, A.}, year = {2021}, eprint = {2112.12331}, eprinttype = {arxiv}}
@software{sw2022, title = {Tool}, author = {Doe, A.}, version = {1.2}, year = {2022}}
@misc{web2024, title = {Page}, url = {https://example.com}, urldate = {2024-03-15}, year = {2024}}
`

func zotero(t *testing.T) *Library {
	l := library(t)
	if added, skipped := importing(t, l, zoteroExport); added != 7 || skipped != 0 { t.Fatalf("imported %d, skipped %d", added, skipped) }
	return l
}

func kinds(l *Library) [][2]string {
	var list [][2]string
	for _, row := range l.Rows() { list = append(list, [2]string{row.Key, row.Kind}) }
	sort.Slice(list, func(i, j int) bool { return list[i][0] < list[j][0] })
	return list
}

func TestAZoteroExportKeepsItsTypes(t *testing.T) {
	want := [][2]string{{"arx2021", "Preprint"}, {"chap2020", "Book Section"}, {"knuth1984", "Book"}, {"parry2022", "Journal Article"}, {"phd2020", "Thesis"}, {"sw2022", "Software, Data"}, {"web2024", "Web Page"}}
	if got := kinds(zotero(t)); !reflect.DeepEqual(got, want) { t.Fatal(got) }
}

func TestKeywordsBecomeTags(t *testing.T) {
	var names []string
	for _, tag := range zotero(t).Tags() { names = append(names, tag.Name) }
	if !reflect.DeepEqual(names, []string{"flaky tests", "testing"}) { t.Fatal(names) }
}

func TestImportReadsWhatHayagrivaDropsAndWhatItFilesOnTheParent(t *testing.T) {
	l := zotero(t)
	if got := l.FieldText("parry2022", Language); got != "en" { t.Fatal(got) }
	if got := l.FieldText("chap2020", Publisher); got != "Pub" { t.Fatal(got) }
}

func TestImportingTheSameFileAgainAddsNothing(t *testing.T) {
	if added, skipped := importing(t, zotero(t), zoteroExport); added != 0 || skipped != 7 { t.Fatal(added, skipped) }
}

func tagged(t *testing.T) *Library {
	l := library(t)
	importing(t, l, "@book{a, title = {A}, keywords = {old, keep}}\n@book{b, title = {B}, keywords = {old, merged}}\n@book{c, title = {C}, keywords = {merged}}")
	return l
}

func TestARenamedTagChangesOnEveryEntry(t *testing.T) {
	l := tagged(t)
	must(t, l.RenameTag("old", "fresh"))
	if got := tagsOf(t, l, "a"); !reflect.DeepEqual(got, []string{"fresh", "keep"}) { t.Fatal(got) }
}

func TestARenamedTagMergesIntoOneInUse(t *testing.T) {
	l := tagged(t)
	must(t, l.RenameTag("old", "MERGED"))
	if got := l.Tags(); !reflect.DeepEqual(got, []TagCount{{"keep", 1}, {"merged", 3}}) { t.Fatal(got) }
	if got := tagsOf(t, l, "b"); !reflect.DeepEqual(got, []string{"merged"}) { t.Fatal(got) }
}

func TestADeletedTagLeavesNoEmptyListBehind(t *testing.T) {
	l := library(t)
	importing(t, l, "@book{a, title = {A}, keywords = {only}}")
	must(t, l.DeleteTag("only"))
	if len(l.Tags()) != 0 || strings.Contains(l.Text(), "tags") { t.Fatal(l.Text()) }
}

func TestChoosingPreprintSticksAndChoosingAgainLeavesNoTrace(t *testing.T) {
	l := library(t)
	importing(t, l, "@article{a, title = {A}}")
	must(t, l.SetSourceType("a", Preprint))
	if kind, _ := l.SourceType("a"); kind.Label != "Preprint" { t.Fatal(kind.Label) }
	must(t, l.SetSourceType("a", JournalArticle))
	if kind, _ := l.SourceType("a"); kind.Label != "Journal Article" { t.Fatal(kind.Label) }
	if strings.Contains(l.Text(), "genre") { t.Fatal(l.Text()) }
}

func TestADeletedEntryCanComeBackFromAVersion(t *testing.T) {
	l := zotero(t)
	before := l.Text()
	must(t, l.DeleteEntries([]string{"knuth1984"}))
	if l.Contains("knuth1984") { t.Fatal("still there") }
	versions := l.Versions().List()
	if kept, err := l.Versions().Read(versions[0]); err != nil || kept != before { t.Fatal(kept, err) }
	must(t, l.Restore(before))
	if !l.Contains("knuth1984") { t.Fatal("not restored") }
}

func TestAVersionThatDoesNotParseChangesNothing(t *testing.T) {
	l := zotero(t)
	before := l.Text()
	if l.Restore("not: [valid") == nil { t.Fatal("restored") }
	if l.Text() != before { t.Fatal(l.Text()) }
}

func TestANewKeyForAKnownDOIIsADuplicate(t *testing.T) {
	if added, skipped := importing(t, zotero(t), "@article{other, title = {Flaky}, doi = {10.1145/3476105}}"); added != 0 || skipped != 1 { t.Fatal(added, skipped) }
}

func entryOf(t *testing.T, text string) Entry {
	entry, err := ParseEntry(text)
	must(t, err)
	return entry
}

func TestANumericArxivIDIsADuplicate(t *testing.T) {
	l := library(t)
	first, err := l.AddFetched(entryOf(t, "type: Article\ntitle: A\nserial-number:\n  arxiv: 2112.12331\n"))
	if err != nil || first != (Added{Key: "entry"}) { t.Fatal(first, err) }
	second, err := l.AddFetched(entryOf(t, "type: Article\ntitle: B\nserial-number:\n  arxiv: 2112.12331\n"))
	if err != nil || second != (Added{Key: "entry", Duplicate: true}) { t.Fatal(second, err) }
}

func TestNumbersManyWorksShareDoNotMakeADuplicate(t *testing.T) {
	l := library(t)
	entry := func(doi string) Entry { return entryOf(t, "type: Article\ntitle: T\nserial-number:\n  doi: "+doi+"\n  issn: 1049-331X\n  version: 1\n") }
	_, err := l.AddFetched(entry("10.1/a"))
	must(t, err)
	if added, err := l.AddFetched(entry("10.1/b")); err != nil || added.Duplicate { t.Fatal(added, err) }
}

func TestALibraryThatCannotBeReadBackIsNotReplaced(t *testing.T) {
	root := t.TempDir()
	l, err := Load(root)
	must(t, err)
	importing(t, l, "@book{a, title = {A}}")
	unreadable := []byte{0xff, 0xfe, 'a'}
	must(t, os.WriteFile(filepath.Join(root, vault.LibraryFile), unreadable, 0o644))
	if l.DeleteEntries([]string{"a"}) == nil { t.Fatal("replaced") }
	if data, _ := os.ReadFile(filepath.Join(root, vault.LibraryFile)); string(data) != string(unreadable) { t.Fatal(data) }
}

func TestARefusedEditGivesItsReasonApartFromThePath(t *testing.T) {
	l := library(t)
	importing(t, l, "@book{a, title = {A}}")
	_, err := l.SaveFields("a", "a", []FieldValue{{Date, "not a date"}})
	var invalid *InvalidLibrary
	if !errors.As(err, &invalid) { t.Fatal(err) }
	if invalid.Reason == "" || strings.Contains(invalid.Reason, vault.LibraryFile) { t.Fatal(invalid.Reason) }
}

func TestExportThenImportLosesNothingItCanHold(t *testing.T) {
	first, second := zotero(t), library(t)
	if added, skipped := importing(t, second, first.ExportBiblatex()); added != 7 || skipped != 0 { t.Fatal(added, skipped) }
	if !reflect.DeepEqual(kinds(second), kinds(first)) { t.Fatal(kinds(second)) }
	for _, row := range first.Rows() {
		kind, _ := first.SourceType(row.Key)
		for _, f := range kind.Fields {
			if a, b := second.FieldText(row.Key, f), first.FieldText(row.Key, f); a != b { t.Errorf("%s: %s: %q, want %q", row.Key, f.Label, a, b) }
		}
		if a, b := tagsOf(t, second, row.Key), tagsOf(t, first, row.Key); !reflect.DeepEqual(a, b) { t.Errorf("%s: tags %v, want %v", row.Key, a, b) }
	}
}

func TestEditingAnEntryKeepsKeyOrderAndUnknownFields(t *testing.T) {
	root := t.TempDir()
	must(t, os.WriteFile(filepath.Join(root, vault.LibraryFile), []byte("b:\n  type: Book\n  title: B\n  custom: kept # a note\na:\n  type: Book\n  title: A\n"), 0o644))
	l, err := Load(root)
	must(t, err)
	changed, err := l.SaveFields("b", "b", []FieldValue{{Title, "New"}})
	if err != nil || !changed { t.Fatal(changed, err) }
	if want := "b:\n  type: Book\n  title: New\n  custom: kept # a note\na:\n  type: Book\n  title: A\n"; l.Text() != want { t.Fatalf("%q", l.Text()) }
}
