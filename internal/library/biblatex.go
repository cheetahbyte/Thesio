package library

import (
	"errors"
	"fmt"
	"sort"
	"strings"

	"github.com/cheetahbyte/thesio/internal/vault"
	"gopkg.in/yaml.v3"
)

// BibLaTeX import and export. Hayagriva does the parsing; what it drops, and
// every field name, comes from the table in source_type.go.

type keyed struct {
	key   string
	entry *yaml.Node
}

// parseBiblatex is the Hayagriva entries in BibLaTeX text, in file order.
func parseBiblatex(text string) ([]keyed, BiblatexImport, error) {
	if Hayagriva == nil {
		return nil, BiblatexImport{}, errors.New("library: Hayagriva is not set")
	}
	imported, err := Hayagriva.ImportBiblatex(text)
	if err != nil {
		return nil, imported, err
	}
	entries, err := parseMapping(imported.YAML)
	if err != nil {
		return nil, imported, err
	}
	var list []keyed
	for i := 0; i+1 < len(entries.Content); i += 2 {
		if entry := asMap(entries.Content[i+1]); entry != nil {
			list = append(list, keyed{entries.Content[i].Value, entry})
		}
	}
	return list, imported, nil
}

var errNoRecord = errors.New("the service returned no record")

// record is a Hayagriva entry from one BibTeX record, as a web service sends it.
func record(text string) (*yaml.Node, error) {
	// Services use keys such as URLs that BibTeX parsers reject, and the key
	// is replaced anyway.
	open := strings.IndexByte(text, '{')
	if open < 0 {
		return nil, errNoRecord
	}
	comma := strings.IndexByte(text[open:], ',')
	if comma < 0 {
		return nil, errNoRecord
	}
	entries, _, err := parseBiblatex(text[:open] + "{entry" + text[open+comma:])
	if err != nil {
		return nil, err
	}
	if len(entries) == 0 {
		return nil, errNoRecord
	}
	return entries[0].entry, nil
}

// refine gives an imported entry a better type than BibLaTeX could: @misc holds
// preprints and web pages alike, and Hayagriva reads @software as misc too.
func refine(entry *yaml.Node) {
	if !same(read(entry, Slot{Kind: SlotType}), typeMisc) {
		return
	}
	var kind string
	switch {
	case read(entry, Arxiv.Slot) != "":
		kind = typeArticle
	case read(entry, Version.Slot) != "":
		kind = typeRepository
	case read(entry, Accessed.Slot) != "":
		kind = typeWeb
	default:
		return
	}
	write(entry, Slot{Kind: SlotType}, kind)
}

// importBiblatex is the entries by key from BibLaTeX text, with the fields
// Hayagriva has no place for.
func importBiblatex(text string) ([]keyed, error) {
	entries, imported, err := parseBiblatex(text)
	if err != nil {
		return nil, err
	}
	for _, in := range entries {
		refine(in.entry)
		raw := imported.Verbatim[in.key]
		var names []string
		for _, tag := range strings.FieldsFunc(raw[keywordsField], func(c rune) bool { return c == ',' || c == ';' }) {
			if tag = strings.TrimSpace(tag); tag != "" {
				names = append(names, tag)
			}
		}
		setTags(in.entry, names)
		// langid names a language (german); only codes are valid here.
		if code := raw[Language.Biblatex]; len(code) == 2 && isLowerASCII(code) {
			write(in.entry, Language.Slot, code)
		}
	}
	return entries, nil
}

func isLowerASCII(text string) bool {
	for i := 0; i < len(text); i++ {
		if text[i] < 'a' || text[i] > 'z' {
			return false
		}
	}
	return true
}

// value is what BibLaTeX gets for f. Hayagriva normalises titles and dates.
func value(f Field, parsed ParsedEntry, raw *yaml.Node) string {
	switch {
	case f.Slot == Title.Slot:
		return parsed.Title
	case f.Slot == parentTitle:
		return parsed.ParentTitle
	case f.Slot.Kind == SlotDate:
		return parsed.Date
	case f.Slot.Kind == SlotPersons:
		return strings.Join(persons(raw, f.Slot), " and ")
	}
	return read(raw, f.Slot)
}

func exportEntry(parsed ParsedEntry, raw *yaml.Node, schema []Field) string {
	kind := biblatexType(raw)
	var fields [][2]string
	for _, f := range schema {
		name := f.BiblatexName(kind)
		if name == "" {
			continue
		}
		v := value(f, parsed, raw)
		if v == "" {
			continue
		}
		// A serial number is the entry's own number, ahead of the issue it may share a name with.
		known := -1
		for i := range fields {
			if fields[i][0] == name {
				known = i
				break
			}
		}
		if known < 0 {
			fields = append(fields, [2]string{name, v})
		} else if f.Slot.Kind == SlotSerial {
			fields[known][1] = v
		}
	}
	if read(raw, Arxiv.Slot) != "" {
		fields = append(fields, [2]string{eprintType, eprintTypeName})
	}
	fields = append(fields, [2]string{keywordsField, strings.Join(tags(raw), ", ")})
	escape := strings.NewReplacer("{", "\\{", "}", "\\}")
	var body []string
	for _, f := range fields {
		if f[1] != "" {
			body = append(body, fmt.Sprintf("  %s = {%s},", f[0], escape.Replace(f[1])))
		}
	}
	return fmt.Sprintf("@%s{%s,\n%s\n}\n", kind, parsed.Key, strings.Join(body, "\n"))
}

// export is a whole library as BibLaTeX. Fields BibLaTeX has no place for, such
// as a running time, are left out.
func export(parsed []ParsedEntry, raw *yaml.Node) string {
	schema := AllFields()
	rows := append([]ParsedEntry(nil), parsed...)
	sort.SliceStable(rows, func(i, j int) bool { return vault.NaturalLess(rows[i].Key, rows[j].Key) })
	out := make([]string, len(rows))
	for i, entry := range rows {
		node := asMap(get(raw, entry.Key))
		if node == nil {
			node = newMap()
		}
		out[i] = exportEntry(entry, node, schema)
	}
	return strings.Join(out, "\n")
}
