package citations

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// engine is Hayagriva through internal/typst, as the app wires it.
type engine struct{}

func (engine) Check(yaml string) error { return typst.CheckLibrary(yaml) }

func (engine) Entries(yaml string) ([]library.ParsedEntry, error) {
	parsed, err := typst.LibraryEntries(yaml)
	out := make([]library.ParsedEntry, len(parsed))
	for i, p := range parsed {
		out[i] = library.ParsedEntry{Key: p.Key, Title: p.Title, Authors: p.Authors, Date: p.Date, ParentTitle: p.ParentTitle}
	}
	return out, err
}

func (engine) ImportBiblatex(bibtex string) (library.BiblatexImport, error) {
	imported, err := typst.ImportBiblatex(bibtex)
	return library.BiblatexImport{YAML: imported.YAML, Verbatim: imported.Verbatim}, err
}

func TestMain(m *testing.M) {
	library.Hayagriva = engine{}
	project.Parse = func(text string) project.Parsed {
		parsed := typst.ParseDocument(text)
		var out project.Parsed
		for _, h := range parsed.Headings {
			out.Headings = append(out.Headings, project.Heading{Offset: h.Offset, Level: h.Level, Title: h.Title})
		}
		for _, inc := range parsed.Includes {
			out.Includes = append(out.Includes, project.Include{Offset: inc.Offset, Literal: inc.Literal})
		}
		return out
	}
	project.DefinesLabel = typst.DefinesLabel
	project.RewritePaths = typst.RewritePaths
	os.Exit(m.Run())
}

const libraryYAML = `smith2020:
  type: article
  title: Deep Learning for Cats
  author: Smith, John
  date: 2020
  tags:
    - ml
jones2021:
  type: book
  title: A History of Dogs
  author: Jones, Mary
  date: 2021
brown2019:
  type: article
  title: Birds and Bees
  author: Brown, Alex
  date: 2019
  tags:
    - nature
`

// fakeHost is the workspace over a temporary vault, recording what it is asked.
type fakeHost struct {
	t       *testing.T
	store   *vault.Store
	lib     *library.Library
	libErr  string
	prefs   *host.Preferences
	current string
	toasts  []string
}

func newHost(t *testing.T) *fakeHost {
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, vault.LibraryFile), []byte(libraryYAML), 0o644); err != nil {
		t.Fatal(err)
	}
	store, _, err := vault.OpenAt(root)
	if err != nil {
		t.Fatal(err)
	}
	h := &fakeHost{t: t, store: store, prefs: host.LoadPreferences(root)}
	h.ReloadLibrary()
	if h.lib == nil {
		t.Fatal(h.libErr)
	}
	return h
}

func (h *fakeHost) libraryText() string {
	data, err := os.ReadFile(filepath.Join(h.store.Root(), vault.LibraryFile))
	if err != nil {
		h.t.Fatal(err)
	}
	return string(data)
}

func (h *fakeHost) Store() *vault.Store                          { return h.store }
func (h *fakeHost) Entries() []vault.Entry                       { return h.store.List() }
func (h *fakeHost) RefreshEntries()                              {}
func (h *fakeHost) Library() *library.Library                    { return h.lib }
func (h *fakeHost) LibraryError() string                         { return h.libErr }
func (h *fakeHost) Prefs() *host.Preferences                     { return h.prefs }
func (h *fakeHost) SavePrefs()                                   {}
func (h *fakeHost) Toast(msg string)                             { h.toasts = append(h.toasts, msg) }
func (h *fakeHost) ToastError(msg string)                        { h.toasts = append(h.toasts, msg) }
func (h *fakeHost) Warn(warnings []string)                       { h.toasts = append(h.toasts, warnings...) }
func (h *fakeHost) Update(fn func())                             { fn() }
func (h *fakeHost) Invalidate()                                  {}
func (h *fakeHost) Window() *mygo.Window                         { return nil }
func (h *fakeHost) ShowView(host.View)                           {}
func (h *fakeHost) CurrentView() host.View                       { return host.ViewLibrary }
func (h *fakeHost) Open(string)                                  {}
func (h *fakeHost) GoToSource(string, int)                       {}
func (h *fakeHost) FollowLink(string)                            {}
func (h *fakeHost) Bar(c *ui.Context, content func())            { ui.Row(c).Children(content) }
func (h *fakeHost) Current() string                              { return h.current }
func (h *fakeHost) MarkUnsaved()                                 {}
func (h *fakeHost) SaveAll() bool                                { return true }
func (h *fakeHost) WhenSaved(then func())                        { then() }
func (h *fakeHost) KeepVersion(string, string, bool)             {}
func (h *fakeHost) HasPaper(string) bool                         { return false }
func (h *fakeHost) OpenPaper(string)                             {}
func (h *fakeHost) FetchPapers([]string)                         {}
func (h *fakeHost) AttachPaper(string, string)                   {}
func (h *fakeHost) OpenSourceNotes(string)                       {}
func (h *fakeHost) PickSource(host.PickSourceOptions)            {}
func (h *fakeHost) InsertQuote(string, bool)                     {}
func (h *fakeHost) QuoteTarget() string                          { return "" }
func (h *fakeHost) InsertCitation(string)                        {}
func (h *fakeHost) CitationAdded(string)                         {}
func (h *fakeHost) SyncBibliographies()                          {}
func (h *fakeHost) RenameCitations(string, string)               {}
func (h *fakeHost) PickFiles(string, bool) ([]string, error)     { return nil, nil }
func (h *fakeHost) PickSavePath(string, string) (string, error)  { return "", nil }
func (h *fakeHost) Confirm(string, string, string, bool, func()) {}
func (h *fakeHost) Reveal(string)                                {}
func (h *fakeHost) OpenExternally(string)                        {}
func (h *fakeHost) CopyText(string)                              {}

func (h *fakeHost) PathChanged(string, string, []vault.Rewritten) {}

func (h *fakeHost) Entry(id string) (vault.Entry, bool) {
	for _, e := range h.store.List() {
		if e.ID == id {
			return e, true
		}
	}
	return vault.Entry{}, false
}

func (h *fakeHost) ReloadLibrary() {
	lib, err := library.Load(h.store.Root())
	h.lib, h.libErr = lib, ""
	if err != nil {
		h.libErr = err.Error()
	}
}

func (h *fakeHost) TextOf(id string) (string, error) { return h.store.Load(id) }

func (h *fakeHost) WriteDocument(id, text string, _ bool) error { return h.store.Save(id, text) }

func (h *fakeHost) read(id string) (string, bool) {
	text, err := h.store.Load(id)
	return text, err == nil
}

func (h *fakeHost) Documents() *project.Documents {
	var ids []string
	for _, e := range h.store.List() {
		if e.Typst {
			ids = append(ids, e.ID)
		}
	}
	return project.LoadDocuments(ids, h.read)
}

func (h *fakeHost) TopDocument(id string) string { return h.Documents().TopDocument(id) }

var _ host.Host = (*fakeHost)(nil)

func libraryPage(t *testing.T) (*fakeHost, *State, *ui.Tester) {
	h := newHost(t)
	s := New(h)
	tt := ui.NewTester(func(c *ui.Context) {
		s.LibraryView(c)
		s.Overlays(c)
	}, 1400, 800)
	return h, s, tt
}

func TestTheTableShowsEveryEntry(t *testing.T) {
	_, _, tt := libraryPage(t)
	for _, title := range []string{"Deep Learning for Cats", "A History of Dogs", "Birds and Bees"} {
		if !tt.HasText(title) {
			t.Errorf("no row %q in %q", title, tt.Texts())
		}
	}
}

func TestSearchFiltersTheRows(t *testing.T) {
	_, _, tt := libraryPage(t)
	if err := tt.Click("Search library"); err != nil {
		t.Fatal(err)
	}
	tt.Type("dogs")
	if !tt.HasText("A History of Dogs") || tt.HasText("Deep Learning for Cats") {
		t.Errorf("search for dogs shows %q", tt.Texts())
	}
}

func TestATagChipFiltersTheRows(t *testing.T) {
	_, s, tt := libraryPage(t)
	if err := tt.Click("nature"); err != nil {
		t.Fatal(err)
	}
	if s.tagFilter != "nature" || !tt.HasText("Birds and Bees") || tt.HasText("A History of Dogs") {
		t.Errorf("filter %q shows %q", s.tagFilter, tt.Texts())
	}
}

func TestSelectingARowOpensTheInspector(t *testing.T) {
	_, s, tt := libraryPage(t)
	if err := tt.Click("Birds and Bees"); err != nil {
		t.Fatal(err)
	}
	if s.insp == nil || s.insp.key != "brown2019" || !tt.HasText("Citation Key") || !tt.HasText("Cited In") {
		t.Fatalf("no inspector for brown2019: %q", tt.Texts())
	}
	if s.insp.fields[0] != library.Title || s.insp.values[0] != "Birds and Bees" {
		t.Errorf("first field %q holds %q", s.insp.fields[0].Label, s.insp.values[0])
	}
}

func TestEditingTheTitleAndPressingEnterSavesIt(t *testing.T) {
	h, s, tt := libraryPage(t)
	s.OpenEntry("brown2019")
	tt.Frame()
	if s.insp == nil {
		t.Fatal("the inspector did not open")
	}
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Type("Bees and Birds")
	tt.Key(0, ui.KeyEnter)
	if text := h.libraryText(); !strings.Contains(text, "Bees and Birds") {
		t.Errorf("library.yaml after the edit:\n%s", text)
	}
}

func TestCompletionRanksAMatchingKeyFirst(t *testing.T) {
	_, s, _ := libraryPage(t)
	labels := []typst.Label{{Key: "smooth", Title: "A curve", Kind: "Figure", Page: 3}, {Key: "plot", Title: "Small data", Kind: "Figure"}}
	items := s.CompletionItems("sm", labels)
	var inserted []string
	for _, item := range items {
		inserted = append(inserted, item.Insert)
	}
	if len(inserted) != 3 || inserted[0] != "@smith2020" || inserted[1] != "@smooth" || inserted[2] != "@plot" {
		t.Errorf("sm offers %q", inserted)
	}
	if items[1].Detail != "Figure · p. 3 · smooth" {
		t.Errorf("label detail %q", items[1].Detail)
	}
}

func TestSyncFillsAGeneratedBibliography(t *testing.T) {
	h, s, _ := libraryPage(t)
	doc := "paper.typ"
	if err := h.store.Save(doc, typst.EmbedBibliography("See @smith2020.\n", "")); err != nil {
		t.Fatal(err)
	}
	s.SyncBibliographies()
	text, err := h.store.Load(doc)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(text, "Deep Learning for Cats") || strings.Contains(text, "Birds and Bees") {
		t.Errorf("the document after syncing:\n%s", text)
	}
}

func TestTagsFromTheEntryMenuTagTheEntry(t *testing.T) {
	_, s, tt := libraryPage(t)
	// The row is named for its entry, but its cells take the pointer.
	row, ok := tt.Find("A History of Dogs")
	if !ok {
		t.Fatal("no row A History of Dogs")
	}
	tt.RightClickAt(row.X+row.W/2, row.Y+row.H/2)
	if err := tt.ChooseMenuItem("Tags…"); err != nil {
		t.Fatalf("%v in %q", err, tt.Menu())
	}
	tt.Frame()
	if !s.tagEditor.open || !tt.HasText("ml") {
		t.Fatalf("no tag editor: %q", tt.Texts())
	}
	tt.Type("history")
	tt.Key(0, ui.KeyEnter)
	if row, ok := rowOf(s, "jones2021"); !ok || len(row.Tags) != 1 || row.Tags[0] != "history" {
		t.Errorf("jones2021 after tagging: %+v", row)
	}
}

func rowOf(s *State, key string) (library.Row, bool) {
	for _, row := range s.library().Rows() {
		if row.Key == key {
			return row, true
		}
	}
	return library.Row{}, false
}
