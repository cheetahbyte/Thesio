// Package project is a document and the files it includes, found the way the
// compiler finds them, and which document opens at launch.
package project

import (
	"cmp"
	"os"
	"slices"
	"sync"
	"unicode/utf8"

	"github.com/cheetahbyte/thesio/internal/vault"
)

// Heading is a heading in a document's outline: the file it is written in, and
// where. Level is 1 for `=`.
type Heading struct {
	Path          string
	Offset, Level int
	Title         string
}

// Include is an `#include` at Offset, with its path as written.
type Include struct {
	Offset  int
	Literal string
}

// Parsed is what Parse finds in a file; Heading.Path is left empty.
type Parsed struct {
	Headings []Heading
	Includes []Include
}

// Parse reads a file's headings and includes. The app sets it to an adapter over typst.ParseDocument.
var Parse func(text string) Parsed

// RewritePaths is typst.RewritePaths, set by the app.
var RewritePaths func(text, oldFile, newFile, main string, files []string, from, to string) (string, bool)

// DefinesLabel is typst.DefinesLabel, set by the app: whether text puts the label key on something.
var DefinesLabel func(text, key string) bool

// Candidates are where file may be, nearest first. Typst runs one project per
// root, but a vault holds several: `/template.typ` is sought from the main
// file's folder upward, then at the vault root, as `typst compile` run in that
// project would.
func Candidates(main, file string) []string {
	var out []string
	for dir, ok := vault.ParentOf(main); ok; dir, ok = vault.ParentOf(dir) {
		out = append(out, dir+"/"+file)
	}
	return append(out, file)
}

// Read gives a file's text by vault path.
type Read func(path string) (string, bool)

type include struct {
	offset int
	path   string
}

type file struct {
	path, text string
	// includes are where each include is written, and the path it asks for.
	includes []include
	headings []Heading
}

func parse(path, text string) *file {
	parsed := Parse(text)
	f := &file{path: path, text: text}
	dir := vault.DirParts(path)
	for _, in := range parsed.Includes {
		if target, ok := vault.ResolveLiteral(dir, in.Literal); ok {
			f.includes = append(f.includes, include{in.Offset, target})
		}
	}
	for _, h := range parsed.Headings {
		h.Path = path
		f.headings = append(f.headings, h)
	}
	return f
}

// FileText is a file of a project and its text.
type FileText struct{ Path, Text string }

// Project is the files of one compile, each once.
type Project struct {
	start    string
	hasStart bool
	files    []*file
	// found is the file each requested path turned out to be.
	found map[string]string
}

// Open is main and everything it includes.
func Open(main string, read Read) *Project { return Within(main, main, read) }

// Within is start and what it includes, when main is the file being compiled.
func Within(main, start string, read Read) *Project {
	p := &Project{found: map[string]string{}}
	pending := []string{start}
	for len(pending) > 0 {
		wanted := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if _, ok := p.found[wanted]; ok {
			continue
		}
		path, text, ok := "", "", false
		for _, candidate := range Candidates(main, wanted) {
			if text, ok = read(candidate); ok {
				path = candidate
				break
			}
		}
		if !ok {
			continue
		}
		p.found[wanted] = path
		if p.Contains(path) {
			continue
		}
		f := parse(path, text)
		for _, in := range f.includes {
			pending = append(pending, in.path)
		}
		p.files = append(p.files, f)
	}
	p.start, p.hasStart = p.found[start]
	return p
}

func (p *Project) file(path string) *file {
	for _, f := range p.files {
		if f.path == path {
			return f
		}
	}
	return nil
}

// Contains reports whether path is one of its files.
func (p *Project) Contains(path string) bool { return p.file(path) != nil }

// Files is its files with their text.
func (p *Project) Files() []FileText {
	out := make([]FileText, len(p.files))
	for i, f := range p.files {
		out[i] = FileText{f.path, f.text}
	}
	return out
}

// Outline is the headings of these files in reading order. A file that
// includes itself is followed once.
func (p *Project) Outline() []Heading {
	var out []Heading
	if p.hasStart {
		p.readHeadings(p.start, nil, &out)
	}
	return out
}

func (p *Project) readHeadings(path string, open []string, out *[]Heading) {
	f := p.file(path)
	if f == nil || slices.Contains(open, path) {
		return
	}
	open = append(open, path)
	var includes []include
	for _, in := range f.includes {
		if found, ok := p.found[in.path]; ok {
			includes = append(includes, include{in.offset, found})
		}
	}
	for _, h := range f.headings {
		for len(includes) > 0 && includes[0].offset < h.Offset {
			p.readHeadings(includes[0].path, open, out)
			includes = includes[1:]
		}
		*out = append(*out, h)
	}
	for _, in := range includes {
		p.readHeadings(in.path, open, out)
	}
}

type document struct {
	id      string
	project *Project
}

// Documents is which documents include which.
type Documents struct{ projects []document }

// LoadDocuments reads the documents ids and what each includes.
func LoadDocuments(ids []string, read Read) *Documents {
	type result struct {
		text string
		ok   bool
	}
	known := map[string]result{}
	cached := func(path string) (string, bool) {
		r, seen := known[path]
		if !seen {
			r.text, r.ok = read(path)
			known[path] = r
		}
		return r.text, r.ok
	}
	d := &Documents{}
	for _, id := range ids {
		d.projects = append(d.projects, document{id, Open(id, cached)})
	}
	return d
}

// Includers are the documents that include id, directly or through other files.
func (d *Documents) Includers(id string) []string {
	var out []string
	for _, doc := range d.projects {
		if doc.id != id && doc.project.Contains(id) {
			out = append(out, doc.id)
		}
	}
	return out
}

// TopDocument is the top-level document id belongs to: the first that includes
// it and that nothing includes, or id itself.
func (d *Documents) TopDocument(id string) string {
	for _, top := range d.Includers(id) {
		if len(d.Includers(top)) == 0 {
			return top
		}
	}
	return id
}

// HasLabel reports whether a document id is part of writes the label key, so
// that `@key` there means the label. A label a script makes is not seen.
func (d *Documents) HasLabel(id, key string) bool {
	for _, doc := range d.projects {
		if !doc.project.Contains(id) {
			continue
		}
		for _, f := range doc.project.files {
			if DefinesLabel(f.text, key) {
				return true
			}
		}
	}
	return false
}

// Newest is the document to open when nothing asks for another: the newest of
// ids by modified, the first of those modified together.
func Newest(ids []string, modified []int64) (string, bool) {
	var newest string
	var at int64
	found := false
	for i, time := range modified {
		if i >= len(ids) {
			break
		}
		if !found || time > at {
			newest, at, found = ids[i], time, true
		}
	}
	return newest, found
}

// LaunchTarget is what launch will most likely open: Document, within Main,
// the top-level file that includes it.
type LaunchTarget struct{ Document, Main, Text string }

// Launch reads the vault at root without changing it, for compiling ahead of
// the UI; a wrong guess only wastes that work.
func Launch(root string) (LaunchTarget, bool) {
	type doc struct {
		id       string
		modified int64
		text     string
	}
	var docs []doc
	vault.Walk(root, func(item vault.Item) {
		if !item.IsDocument() {
			return
		}
		if data, err := os.ReadFile(item.Path); err == nil && utf8.Valid(data) {
			docs = append(docs, doc{item.ID, item.Modified(), string(data)})
		}
	})
	ids, modified := make([]string, len(docs)), make([]int64, len(docs))
	for i, d := range docs {
		ids[i], modified[i] = d.id, d.modified
	}
	document, ok := Newest(ids, modified)
	if !ok {
		return LaunchTarget{}, false
	}
	slices.SortStableFunc(docs, func(a, b doc) int { return cmp.Compare(b.modified, a.modified) })
	texts := map[string]string{}
	for i, d := range docs {
		ids[i], texts[d.id] = d.id, d.text
	}
	read := func(path string) (string, bool) { text, ok := texts[path]; return text, ok }
	main := LoadDocuments(ids, read).TopDocument(document)
	return LaunchTarget{Document: document, Main: main, Text: texts[document]}, true
}

// NewRewriter is the vault's Rewriter, over Documents and RewritePaths.
func NewRewriter() vault.Rewriter { return &rewriter{} }

type rewriter struct {
	mu sync.Mutex
	// ids and documents are the last load: a move asks for the top document of
	// every file against the same ids, and loading parses the whole vault.
	ids       []string
	documents *Documents
}

func (r *rewriter) TopDocument(ids []string, read func(id string) (string, bool), id string) string {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.documents == nil || len(ids) != len(r.ids) || len(ids) > 0 && &ids[0] != &r.ids[0] {
		r.ids, r.documents = ids, LoadDocuments(ids, read)
	}
	return r.documents.TopDocument(id)
}

func (r *rewriter) RewritePaths(text, oldFile, newFile, main string, files []string, from, to string) (string, bool) {
	return RewritePaths(text, oldFile, newFile, main, files, from, to)
}
