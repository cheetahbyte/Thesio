package papers

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/pdfkit"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type engine struct{}

func (engine) Check(yaml string) error { return typst.CheckLibrary(yaml) }

func (engine) Entries(yaml string) ([]library.ParsedEntry, error) {
	parsed, err := typst.LibraryEntries(yaml)
	out := make([]library.ParsedEntry, len(parsed))
	for i, e := range parsed {
		out[i] = library.ParsedEntry{Key: e.Key, Title: e.Title, Authors: e.Authors, Date: e.Date, ParentTitle: e.ParentTitle}
	}
	return out, err
}

func (engine) ImportBiblatex(bibtex string) (library.BiblatexImport, error) {
	imported, err := typst.ImportBiblatex(bibtex)
	return library.BiblatexImport{YAML: imported.YAML, Verbatim: imported.Verbatim}, err
}

func TestMain(m *testing.M) {
	library.Hayagriva = engine{}
	os.Exit(m.Run())
}

// fakeHost is a workspace over a temporary vault. Update queues its work for settle.
type fakeHost struct {
	t       *testing.T
	store   *vault.Store
	lib     *library.Library
	entries []vault.Entry
	prefs   *host.Preferences
	updates chan func()
	toasts  []string
	errors  []string
	quotes  []string
	target  string
	view    host.View
}

func (f *fakeHost) Store() *vault.Store    { return f.store }
func (f *fakeHost) Entries() []vault.Entry { return f.entries }
func (f *fakeHost) Entry(id string) (vault.Entry, bool) {
	for _, e := range f.entries {
		if e.ID == id {
			return e, true
		}
	}
	return vault.Entry{}, false
}
func (f *fakeHost) RefreshEntries()                   { f.entries = f.store.List() }
func (f *fakeHost) Library() *library.Library         { return f.lib }
func (f *fakeHost) LibraryError() string              { return "" }
func (f *fakeHost) ReloadLibrary()                    {}
func (f *fakeHost) Prefs() *host.Preferences          { return f.prefs }
func (f *fakeHost) SavePrefs()                        {}
func (f *fakeHost) Toast(msg string)                  { f.toasts = append(f.toasts, msg) }
func (f *fakeHost) ToastError(msg string)             { f.errors = append(f.errors, msg) }
func (f *fakeHost) Warn(warnings []string)            { f.errors = append(f.errors, warnings...) }
func (f *fakeHost) Update(fn func())                  { f.updates <- fn }
func (f *fakeHost) Invalidate()                       {}
func (f *fakeHost) Window() *mygo.Window              { return nil }
func (f *fakeHost) ShowView(v host.View)              { f.view = v }
func (f *fakeHost) CurrentView() host.View            { return f.view }
func (f *fakeHost) Open(_ string)                     {}
func (f *fakeHost) GoToSource(_ string, _ int)        {}
func (f *fakeHost) FollowLink(_ string)               {}
func (f *fakeHost) Bar(c *ui.Context, content func()) { ui.Row(c).Children(content) }
func (f *fakeHost) Current() string                   { return "" }
func (f *fakeHost) TextOf(id string) (string, error)  { return f.store.Load(id) }
func (f *fakeHost) WriteDocument(id, text string, _ bool) error {
	return f.store.Save(id, text)
}
func (f *fakeHost) MarkUnsaved()                                 {}
func (f *fakeHost) SaveAll() bool                                { return true }
func (f *fakeHost) WhenSaved(then func())                        { then() }
func (f *fakeHost) PathChanged(_, _ string, _ []vault.Rewritten) {}
func (f *fakeHost) KeepVersion(_, _ string, _ bool)              {}
func (f *fakeHost) TopDocument(id string) string                 { return id }
func (f *fakeHost) Documents() *project.Documents                { return nil }
func (f *fakeHost) HasPaper(_ string) bool                       { return false }
func (f *fakeHost) OpenPaper(_ string)                           {}
func (f *fakeHost) FetchPapers(_ []string)                       {}
func (f *fakeHost) AttachPaper(_, _ string)                      {}
func (f *fakeHost) OpenSourceNotes(_ string)                     {}
func (f *fakeHost) PickSource(_ host.PickSourceOptions)          {}
func (f *fakeHost) InsertQuote(markup string, _ bool)            { f.quotes = append(f.quotes, markup) }
func (f *fakeHost) QuoteTarget() string                          { return f.target }
func (f *fakeHost) InsertCitation(_ string)                      {}
func (f *fakeHost) CitationAdded(_ string)                       {}
func (f *fakeHost) SyncBibliographies()                          {}
func (f *fakeHost) RenameCitations(_, _ string)                  {}
func (f *fakeHost) PickFiles(_ string, _ bool) ([]string, error) { return nil, nil }
func (f *fakeHost) PickSavePath(_, _ string) (string, error)     { return "", nil }
func (f *fakeHost) Confirm(_, _, _ string, _ bool, then func()) {
	then()
}
func (f *fakeHost) Reveal(_ string)         {}
func (f *fakeHost) OpenExternally(_ string) {}
func (f *fakeHost) CopyText(_ string)       {}

// settle runs the work goroutines handed to Update, waiting for the first.
func (f *fakeHost) settle() {
	f.t.Helper()
	select {
	case fn := <-f.updates:
		fn()
	case <-time.After(10 * time.Second):
		f.t.Fatal("no update came")
	}
	for {
		select {
		case fn := <-f.updates:
			fn()
		default:
			return
		}
	}
}

// writePDF writes a one-page PDF with two lines of Helvetica text.
func writePDF(t *testing.T, path string) {
	t.Helper()
	content := "BT /F1 18 Tf 72 720 Td (Hello highlight world) Tj 0 -24 Td (second line here) Tj ET"
	objects := []string{
		"<< /Type /Catalog /Pages 2 0 R >>",
		"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
		"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
		fmt.Sprintf("<< /Length %d >>\nstream\n%s\nendstream", len(content), content),
		"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
	}
	var b bytes.Buffer
	b.WriteString("%PDF-1.4\n")
	offsets := make([]int, len(objects))
	for i, o := range objects {
		offsets[i] = b.Len()
		fmt.Fprintf(&b, "%d 0 obj\n%s\nendobj\n", i+1, o)
	}
	xref := b.Len()
	fmt.Fprintf(&b, "xref\n0 %d\n0000000000 65535 f \n", len(objects)+1)
	for _, off := range offsets {
		fmt.Fprintf(&b, "%010d 00000 n \n", off)
	}
	fmt.Fprintf(&b, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(objects)+1, xref)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, b.Bytes(), 0o644); err != nil {
		t.Fatal(err)
	}
}

const libraryYAML = `smith2020:
  type: article
  title: Reading Papers Closely
  author: ["Smith, Jane"]
  date: 2020
`

func vaultWithPaper(t *testing.T) (*fakeHost, *State) {
	t.Helper()
	root := t.TempDir()
	writePDF(t, filepath.Join(root, "sources", "smith2020.pdf"))
	if err := os.WriteFile(filepath.Join(root, vault.LibraryFile), []byte(libraryYAML), 0o644); err != nil {
		t.Fatal(err)
	}
	store, _, err := vault.OpenAt(root)
	if err != nil {
		t.Fatal(err)
	}
	lib, err := library.Load(root)
	if err != nil {
		t.Fatal(err)
	}
	prefs := host.DefaultPreferences()
	f := &fakeHost{t: t, store: store, lib: lib, prefs: &prefs, updates: make(chan func(), 16)}
	f.RefreshEntries()
	return f, New(f)
}

const paperID = "sources/smith2020.pdf"

func TestSourcesShowsThePaperUnderItsSourceTitle(t *testing.T) {
	_, s := vaultWithPaper(t)
	tt := ui.NewTester(s.SourcesView, 900, 400)
	for _, text := range []string{"Sources", "Reading Papers Closely", "Smith", "2020", "smith2020"} {
		if !tt.HasText(text) {
			t.Errorf("no %q in %q", text, tt.Texts())
		}
	}
	if tt.HasText("Link to Source…") {
		t.Error("a linked paper offers to link")
	}
}

func TestPapersAreFoundByKey(t *testing.T) {
	_, s := vaultWithPaper(t)
	if ids := s.PaperIDs(); len(ids) != 1 || ids[0] != paperID {
		t.Fatalf("PaperIDs = %q", ids)
	}
	if !s.HasPaper("smith2020") || s.HasPaper("jones2021") {
		t.Fatal("HasPaper is wrong")
	}
	if title := s.PaperTitle(paperID); title != "Reading Papers Closely" {
		t.Fatalf("PaperTitle = %q", title)
	}
}

func TestAttachRefusesAFileThatIsNotAPDF(t *testing.T) {
	f, s := vaultWithPaper(t)
	text := filepath.Join(t.TempDir(), "notes.pdf")
	if err := os.WriteFile(text, []byte("plain text"), 0o644); err != nil {
		t.Fatal(err)
	}
	s.AttachPaper(text, "sources/notes.pdf")
	if len(f.errors) != 1 || f.errors[0] != "That file is not a PDF" {
		t.Fatalf("errors = %q", f.errors)
	}
	if _, ok := f.Entry("sources/notes.pdf"); ok {
		t.Fatal("the file came in")
	}
	pdf := filepath.Join(t.TempDir(), "other.pdf")
	writePDF(t, pdf)
	s.AttachPaper(pdf, library.PaperNamed("other"))
	if _, ok := f.Entry("sources/other.pdf"); !ok || len(f.toasts) != 1 {
		t.Fatalf("the PDF did not come in: toasts %q, errors %q", f.toasts, f.errors)
	}
}

func TestHighlightsAreReadFromThePDF(t *testing.T) {
	if !pdfkit.Available() {
		t.Skip("PDFKit is not available")
	}
	f, s := vaultWithPaper(t)
	if hs := s.Highlights(paperID); len(hs) != 0 {
		t.Fatalf("highlights before reading: %+v", hs)
	}
	f.settle()
	if hs := s.Highlights(paperID); len(hs) != 0 {
		t.Fatalf("highlights of a fresh paper: %+v", hs)
	}
	path, _ := f.store.Path(paperID)
	doc, err := pdfkit.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := doc.AddHighlight(0, "highlight world", "a note"); err != nil {
		t.Fatal(err)
	}
	doc.Close()
	s.Highlights(paperID)
	f.settle()
	hs := s.Highlights(paperID)
	if len(hs) != 1 || hs[0].Comment != "a note" || !strings.Contains(hs[0].Text, "highlight") {
		t.Fatalf("highlights = %+v", hs)
	}

	s.current, s.tab = paperID, tabHighlights
	tt := ui.NewTester(s.PaperView, 900, 500)
	for _, text := range []string{"Reading Papers Closely", "Page 1", "a note", "Insert as Quote"} {
		if !tt.HasText(text) {
			t.Errorf("no %q in %q", text, tt.Texts())
		}
	}
}

func TestHighlightSelectionHighlightsTheCopiedText(t *testing.T) {
	if !pdfkit.Available() {
		t.Skip("PDFKit is not available")
	}
	f, s := vaultWithPaper(t)
	s.current = paperID
	copied := ""
	clipboardText = func() string { return copied }
	t.Cleanup(func() { clipboardText = mygo.Clipboard.ReadText })
	s.highlightSelection()
	if len(f.errors) != 1 || f.errors[0] != "Copy some text in the paper first" {
		t.Fatalf("errors = %q", f.errors)
	}
	copied = "world\nsecond"
	s.highlightSelection()
	f.settle()
	if s.tab != tabHighlights || len(f.errors) != 1 {
		t.Fatalf("tab %d, errors %q", s.tab, f.errors)
	}
	s.Highlights(paperID)
	f.settle()
	if hs := s.Highlights(paperID); len(hs) != 1 || !strings.Contains(hs[0].Text, "second") {
		t.Fatalf("highlights = %+v", hs)
	}
}

func TestAQuoteGoesInWithItsPage(t *testing.T) {
	f, s := vaultWithPaper(t)
	f.target = "essay.typ"
	s.current = paperID
	s.openQuote(pdfkit.Highlight{Page: 2, Text: "close read- ing matters"})
	if s.quote == nil || s.quote.text != "close reading matters" || s.quote.page != "3" {
		t.Fatalf("quote = %+v", s.quote)
	}
	tt := ui.NewTester(s.Overlays, 700, 500)
	if !tt.HasText("Insert Quote") {
		t.Fatalf("no dialog: %q", tt.Texts())
	}
	s.quote.entered = "13"
	if err := tt.Click("Insert"); err != nil {
		t.Fatal(err)
	}
	if len(f.quotes) != 1 || f.quotes[0] != "#quote[close reading matters] @smith2020[p. 13]" {
		t.Fatalf("quotes = %q", f.quotes)
	}
	if s.quoting || s.quote != nil {
		t.Fatal("the dialog stayed open")
	}
	if e, _ := f.Entry(paperID); e.Meta.PageOffset == nil || *e.Meta.PageOffset != 10 {
		t.Fatalf("page offset = %v", e.Meta.PageOffset)
	}
}

func TestAPaperWithoutASourceCannotBeQuoted(t *testing.T) {
	f, s := vaultWithPaper(t)
	s.current = "sources/loose.pdf"
	s.openQuote(pdfkit.Highlight{Text: "words"})
	if s.quote != nil || len(f.errors) != 1 || f.errors[0] != "Link this paper to a source to quote it" {
		t.Fatalf("quote %+v, errors %q", s.quote, f.errors)
	}
}
