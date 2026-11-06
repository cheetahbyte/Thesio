package document

import (
	"bytes"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
)

// fakeHost queues Update's functions for the test to run on its own goroutine,
// as the main thread would.
type fakeHost struct {
	store  *vault.Store
	prefs  host.Preferences
	mu     sync.Mutex
	queue  []func()
	toasts []string
}

func (h *fakeHost) Store() *vault.Store                           { return h.store }
func (h *fakeHost) Entries() []vault.Entry                        { return h.store.List() }
func (h *fakeHost) Entry(string) (vault.Entry, bool)              { return vault.Entry{}, false }
func (h *fakeHost) RefreshEntries()                               {}
func (h *fakeHost) Library() *library.Library                     { return nil }
func (h *fakeHost) LibraryError() string                          { return "" }
func (h *fakeHost) ReloadLibrary()                                {}
func (h *fakeHost) Prefs() *host.Preferences                      { return &h.prefs }
func (h *fakeHost) SavePrefs()                                    {}
func (h *fakeHost) Toast(msg string)                              { h.toasts = append(h.toasts, msg) }
func (h *fakeHost) ToastError(msg string)                         { h.toasts = append(h.toasts, msg) }
func (h *fakeHost) Warn([]string)                                 {}
func (h *fakeHost) Invalidate()                                   {}
func (h *fakeHost) Window() *mygo.Window                          { return nil }
func (h *fakeHost) ShowView(host.View)                            {}
func (h *fakeHost) CurrentView() host.View                        { return host.ViewDoc }
func (h *fakeHost) Open(string)                                   {}
func (h *fakeHost) GoToSource(string, int)                        {}
func (h *fakeHost) FollowLink(string)                             {}
func (h *fakeHost) Bar(c *ui.Context, content func())             { ui.Row(c).Children(content) }
func (h *fakeHost) Current() string                               { return "" }
func (h *fakeHost) TextOf(id string) (string, error)              { return h.store.Load(id) }
func (h *fakeHost) WriteDocument(string, string, bool) error      { return nil }
func (h *fakeHost) MarkUnsaved()                                  {}
func (h *fakeHost) SaveAll() bool                                 { return true }
func (h *fakeHost) WhenSaved(then func())                         { then() }
func (h *fakeHost) PathChanged(string, string, []vault.Rewritten) {}
func (h *fakeHost) KeepVersion(string, string, bool)              {}
func (h *fakeHost) TopDocument(id string) string                  { return id }
func (h *fakeHost) Documents() *project.Documents                 { return nil }
func (h *fakeHost) HasPaper(string) bool                          { return false }
func (h *fakeHost) OpenPaper(string)                              {}
func (h *fakeHost) FetchPapers([]string)                          {}
func (h *fakeHost) AttachPaper(string, string)                    {}
func (h *fakeHost) OpenSourceNotes(string)                        {}
func (h *fakeHost) PickSource(host.PickSourceOptions)             {}
func (h *fakeHost) InsertQuote(string, bool)                      {}
func (h *fakeHost) QuoteTarget() string                           { return "" }
func (h *fakeHost) InsertCitation(string)                         {}
func (h *fakeHost) CitationAdded(string)                          {}
func (h *fakeHost) SyncBibliographies()                           {}
func (h *fakeHost) RenameCitations(string, string)                {}
func (h *fakeHost) PickFiles(string, bool) ([]string, error)      { return nil, nil }
func (h *fakeHost) PickSavePath(string, string) (string, error)   { return "", nil }
func (h *fakeHost) Confirm(string, string, string, bool, func())  {}
func (h *fakeHost) Reveal(string)                                 {}
func (h *fakeHost) OpenExternally(string)                         {}
func (h *fakeHost) CopyText(string)                               {}

func (h *fakeHost) Update(fn func()) {
	h.mu.Lock()
	h.queue = append(h.queue, fn)
	h.mu.Unlock()
}

func (h *fakeHost) pump() {
	h.mu.Lock()
	queue := h.queue
	h.queue = nil
	h.mu.Unlock()
	for _, fn := range queue {
		fn()
	}
}

var _ host.Host = (*fakeHost)(nil)

// opened opens id of a vault holding files, in a tester showing it.
func opened(t *testing.T, files map[string]string, id string, withPreview bool) (*Open, *fakeHost, *ui.Tester) {
	t.Helper()
	root := t.TempDir()
	for path, text := range files {
		if err := os.WriteFile(filepath.Join(root, path), []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	store, _, err := vault.OpenAt(root)
	if err != nil {
		t.Fatal(err)
	}
	h := &fakeHost{store: store, prefs: host.DefaultPreferences()}
	d := NewOpen(h, id, files[id], id, withPreview)
	t.Cleanup(d.Close)
	tt := ui.NewTester(d.View, 1000, 700)
	d.Focus()
	tt.Frame()
	return d, h, tt
}

// until runs the main thread's work and frames until done, for at most ten seconds.
func until(t *testing.T, h *fakeHost, tt *ui.Tester, what string, done func() bool) {
	t.Helper()
	for deadline := time.Now().Add(10 * time.Second); time.Now().Before(deadline); time.Sleep(5 * time.Millisecond) {
		h.pump()
		tt.Frame()
		if done() {
			return
		}
	}
	t.Fatalf("timed out waiting for %s", what)
}

func words(d *Open) int {
	total := 0
	for _, w := range d.Words() {
		total += w.Words
	}
	return total
}

func TestTypingCompilesAndSaves(t *testing.T) {
	d, h, tt := opened(t, map[string]string{"doc.typ": "= Title\n\nSome words here.\n"}, "doc.typ", true)
	until(t, h, tt, "the first compile", func() bool { return len(d.s.pages) > 0 && words(d) > 0 })
	before := words(d)
	if !tt.HasText(d.statusText()) {
		t.Errorf("status %q not shown in %q", d.statusText(), tt.Texts())
	}
	tt.Type("More words ")
	if !strings.HasPrefix(d.Text(), "More words ") {
		t.Fatalf("text %q", d.Text())
	}
	until(t, h, tt, "a compile of the typing", func() bool { return words(d) > before })
	if !d.Unsaved() {
		t.Fatal("an edit left nothing unsaved")
	}
	until(t, h, tt, "the save", func() bool { return d.Status().State == Saved })
	data, err := os.ReadFile(filepath.Join(d.h.Store().Root(), "doc.typ"))
	if err != nil || string(data) != d.Text() {
		t.Fatalf("file %q, %v", data, err)
	}
}

func TestHighlighterTokens(t *testing.T) {
	h := newHighlighter(editor.NewBuffer("= Heading\n#let x = 1\n").Snapshot())
	h.Highlight(1, time.Second)
	heading, _ := h.Tokens(0, "= Heading", nil)
	if len(heading) == 0 || heading[len(heading)-1] != (editor.Token{End: 9, Kind: editor.TokenHeading}) {
		t.Errorf("heading %+v", heading)
	}
	code, _ := h.Tokens(1, "#let x = 1", nil)
	kinds := map[editor.TokenKind]int{}
	for _, tok := range code {
		kinds[tok.Kind] = tok.End
	}
	if kinds[editor.TokenKeyword] != 4 || kinds[editor.TokenNumber] != 10 {
		t.Errorf("code %+v", code)
	}
}

func TestCompletionAfterAHash(t *testing.T) {
	d, h, tt := opened(t, map[string]string{"doc.typ": "Text.\n"}, "doc.typ", true)
	until(t, h, tt, "the first compile", func() bool { return d.s.knowledge() != nil })
	d.GoTo(len(d.Text()))
	tt.Frame()
	tt.Type("#")
	tt.Type("headi")
	until(t, h, tt, "heading offered", func() bool { return tt.HasText("heading") })
	tt.Key(0, ui.KeyEnter)
	h.pump()
	tt.Frame()
	if !strings.Contains(d.Text(), "#heading") {
		t.Fatalf("accepted %q", d.Text())
	}
}

func TestTheBibliographyBlockStaysFolded(t *testing.T) {
	text := typst.EmbedBibliography("= Notes\n\nText.\n", "a:\n  type: Book\n  title: X\n")
	d, _, tt := opened(t, map[string]string{"doc.typ": text}, "doc.typ", false)
	start, _, ok := typst.BibliographyBlockRange(text)
	if !ok {
		t.Fatal("no block")
	}
	first := d.buf.Pos(start).Line
	if !slices.Contains(d.ed.Folded(), first) {
		t.Fatalf("folded %v, block at line %d", d.ed.Folded(), first)
	}
	d.ed.Select(start+5, start+5)
	tt.Frame()
	if c := d.Caret(); c >= start {
		t.Fatalf("caret %d in the block from %d", c, start)
	}
	d.ToggleBlock()
	tt.Frame()
	if !d.BlockShown() || slices.Contains(d.ed.Folded(), first) {
		t.Fatalf("shown %v, folded %v", d.BlockShown(), d.ed.Folded())
	}
}

func TestExportPDF(t *testing.T) {
	data, err := ExportPDF(t.TempDir(), "doc.typ", "= Hello\n\nWorld.\n")
	if err != nil || !bytes.HasPrefix(data, []byte("%PDF")) {
		t.Fatalf("pdf %d bytes, %v", len(data), err)
	}
	if _, err := ExportPDF(t.TempDir(), "doc.typ", "#nonsense(\n"); err == nil {
		t.Fatal("a broken document exported")
	}
}

func TestNoteLinkAndCitationCompletion(t *testing.T) {
	d, h, tt := opened(t, map[string]string{"doc.typ": "Text.\n"}, "doc.typ", false)
	d.NoteLinks = func(string) []editor.Completion {
		return []editor.Completion{{Label: "Other", Detail: "other.typ", Insert: `#link("other.typ")[Other]`}}
	}
	d.Citations = func(string, []typst.Label) []editor.Completion {
		return []editor.Completion{{Label: "Smith 2020", Insert: "@smith2020"}}
	}
	cited := 0
	d.OnCitationCompleted = func() { cited++ }
	d.GoTo(len(d.Text()))
	tt.Frame()
	for _, s := range []string{"See ", "[", "[", "ot"} {
		tt.Type(s)
	}
	until(t, h, tt, "the note offered", func() bool { return tt.HasText("other.typ") })
	tt.Key(0, ui.KeyEnter)
	tt.Type(" and ")
	tt.Type("@")
	tt.Type("sm")
	until(t, h, tt, "the source offered", func() bool { return tt.HasText("Smith 2020") })
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if want := "Text.\nSee #link(\"other.typ\")[Other] and @smith2020"; d.Text() != want || cited != 1 {
		t.Fatalf("text %q, cited %d", d.Text(), cited)
	}
}

func TestADroppedImageBecomesAFigure(t *testing.T) {
	vault.DetectImage = typst.ImageFormat
	d, _, tt := opened(t, map[string]string{"doc.typ": "Text.\n"}, "doc.typ", true)
	img := filepath.Join(t.TempDir(), "plot.png")
	var data bytes.Buffer
	if err := png.Encode(&data, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(img, data.Bytes(), 0o644); err != nil {
		t.Fatal(err)
	}
	if !tt.DropFiles(100, 100, img) {
		t.Fatal("the editor took no files")
	}
	tt.Frame()
	if !strings.Contains(d.Text(), `image("images/plot.png")`) || !strings.HasSuffix(d.Text()[:d.Caret()], "caption: [") {
		t.Fatalf("text %q, caret %d", d.Text(), d.Caret())
	}
}
