// Package app is the workspace of a Thesio window: the sidebar, the main
// pane and its pages, the dialogs, and the file operations behind them. It
// implements host.Host for the feature packages it routes between.
package app

import (
	"cmp"
	"fmt"
	"path/filepath"
	"slices"
	"strings"
	"sync"

	"github.com/cheetahbyte/thesio/internal/app/citations"
	"github.com/cheetahbyte/thesio/internal/app/document"
	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/papers"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

var _ host.Host = (*Workspace)(nil)

// Workspace is the state of one window.
type Workspace struct {
	win   *mygo.Window
	store *vault.Store
	prefs *host.Preferences

	// entries are most recently modified first; index finds them by id.
	entries []vault.Entry
	index   map[string]int

	lib    *library.Library
	libErr string

	doc    *document.Open
	cites  *citations.State
	papers *papers.State
	view   host.View

	mu sync.Mutex
	// queued runs on the next frame, for Update without a window (tests).
	queued []func()
	toasts []ui.Toast

	sidebar  sidebarState
	outline  outlineState
	home     homeState
	search   searchState
	trash    []vault.TrashEntry
	settings settingsState
	confirm  confirmState
	move     moveState
	history  historyState
	words    bool
	icon     ui.ImageSource
	// pending runs once the view is built.
	pending []func()
}

// New is the workspace of win over store; warnings are the store's from opening it.
func New(win *mygo.Window, store *vault.Store, warnings []string) *Workspace {
	ws := &Workspace{win: win, store: store, prefs: host.LoadPreferences(store.Root()), view: host.ViewHome}
	store.SetRewriter(project.NewRewriter())
	ws.sidebar = newSidebarState(store.Settings())
	ws.RefreshEntries()
	ws.ReloadLibrary()
	ws.cites = citations.New(ws)
	ws.cites.ToggleBlock = func() {
		if ws.doc != nil {
			ws.doc.ToggleBlock()
		}
	}
	ws.cites.BlockShown = func() bool { return ws.doc != nil && ws.doc.BlockShown() }
	ws.cites.OpenHistory = ws.openLibraryHistory
	ws.papers = papers.New(ws)
	ws.Warn(warnings)
	return ws
}

// Wire points the engine's injected parsers at the Typst library. Call it before opening a store.
func Wire() {
	project.Parse = func(text string) project.Parsed {
		parsed := typst.ParseDocument(text)
		out := project.Parsed{Headings: make([]project.Heading, len(parsed.Headings)), Includes: make([]project.Include, len(parsed.Includes))}
		for i, h := range parsed.Headings {
			out.Headings[i] = project.Heading{Offset: h.Offset, Level: h.Level, Title: h.Title}
		}
		for i, inc := range parsed.Includes {
			out.Includes[i] = project.Include{Offset: inc.Offset, Literal: inc.Literal}
		}
		return out
	}
	project.DefinesLabel = typst.DefinesLabel
	project.RewritePaths = typst.RewritePaths
	library.Hayagriva = hayagriva{}
	vault.DetectImage = typst.ImageFormat
}

type hayagriva struct{}

func (hayagriva) Check(yaml string) error { return typst.CheckLibrary(yaml) }

func (hayagriva) Entries(yaml string) ([]library.ParsedEntry, error) {
	parsed, err := typst.LibraryEntries(yaml)
	if err != nil {
		return nil, err
	}
	out := make([]library.ParsedEntry, len(parsed))
	for i, e := range parsed {
		out[i] = library.ParsedEntry{Key: e.Key, Title: e.Title, Authors: e.Authors, Date: e.Date, ParentTitle: e.ParentTitle}
	}
	return out, nil
}

func (hayagriva) ImportBiblatex(bibtex string) (library.BiblatexImport, error) {
	parsed, err := typst.ImportBiblatex(bibtex)
	return library.BiblatexImport{YAML: parsed.YAML, Verbatim: parsed.Verbatim}, err
}

// ApplyAppearance follows the appearance preference: "light", "dark", or "" for the system's.
// Off the main thread it waits for the app to run.
func ApplyAppearance(appearance string) {
	switch appearance {
	case "light":
		mygo.Theme.SetSource(mygo.ThemeLight)
	case "dark":
		mygo.Theme.SetSource(mygo.ThemeDark)
	default:
		mygo.Theme.SetSource(mygo.ThemeSystem)
	}
}

// Start opens id, or a new document when id is "".
func (ws *Workspace) Start(id string) {
	if _, ok := ws.Entry(id); ok {
		ws.Open(id)
		return
	}
	ws.NewDocument("")
}

// Store is the vault.
func (ws *Workspace) Store() *vault.Store { return ws.store }

// Entries is the vault's listing, most recently modified first.
func (ws *Workspace) Entries() []vault.Entry { return ws.entries }

// Entry finds one by id.
func (ws *Workspace) Entry(id string) (vault.Entry, bool) {
	i, ok := ws.index[id]
	if !ok {
		return vault.Entry{}, false
	}
	return ws.entries[i], true
}

func (ws *Workspace) entry(id string) *vault.Entry {
	if i, ok := ws.index[id]; ok {
		return &ws.entries[i]
	}
	return nil
}

// RefreshEntries reads the vault again. A paper that went leaves its view.
func (ws *Workspace) RefreshEntries() {
	ws.entries = ws.store.List()
	ws.sort()
	if ws.view == host.ViewPaper && ws.papers != nil {
		if _, ok := ws.Entry(ws.papers.Current()); !ok {
			ws.setView(host.ViewSources)
		}
	}
}

func (ws *Workspace) sort() {
	slices.SortStableFunc(ws.entries, func(a, b vault.Entry) int { return cmp.Compare(b.Modified, a.Modified) })
	ws.index = make(map[string]int, len(ws.entries))
	for i, e := range ws.entries {
		ws.index[e.ID] = i
	}
}

// Library is nil while library.yaml cannot be read.
func (ws *Workspace) Library() *library.Library { return ws.lib }

// LibraryError is why the library cannot be read, "" when it can.
func (ws *Workspace) LibraryError() string { return ws.libErr }

// ReloadLibrary reads library.yaml again.
func (ws *Workspace) ReloadLibrary() {
	lib, err := library.Load(ws.store.Root())
	ws.lib, ws.libErr = lib, ""
	if err != nil {
		ws.lib, ws.libErr = nil, err.Error()
	}
}

func (ws *Workspace) libraryRows() []library.Row {
	if ws.lib == nil {
		return nil
	}
	return ws.lib.Rows()
}

// Prefs are the vault's preferences.
func (ws *Workspace) Prefs() *host.Preferences { return ws.prefs }

// SavePrefs writes the preferences; a failure is toasted.
func (ws *Workspace) SavePrefs() {
	if err := ws.prefs.Save(); err != nil {
		ws.ToastError(fmt.Sprintf("Could not save settings: %v", err))
	}
	ws.Invalidate()
}

// Toast shows a message.
func (ws *Workspace) Toast(msg string) { ws.addToast(ui.Toast{Title: msg}) }

// ToastError shows an error.
func (ws *Workspace) ToastError(msg string) { ws.addToast(ui.Toast{Title: msg, Type: "error"}) }

// Warn toasts the warnings joined, when there are any.
func (ws *Workspace) Warn(warnings []string) {
	if len(warnings) > 0 {
		ws.ToastError(strings.Join(warnings, "; "))
	}
}

func (ws *Workspace) addToast(t ui.Toast) {
	ws.mu.Lock()
	ws.toasts = append(ws.toasts, t)
	ws.mu.Unlock()
	ws.Invalidate()
}

// Update runs fn on the main thread and draws a frame; safe from any goroutine.
// Without a window (tests), fn waits for the next frame.
func (ws *Workspace) Update(fn func()) {
	if ws.win != nil {
		ws.win.Update(fn)
		return
	}
	ws.mu.Lock()
	ws.queued = append(ws.queued, fn)
	ws.mu.Unlock()
}

// Invalidate draws a frame.
func (ws *Workspace) Invalidate() {
	if ws.win != nil {
		ws.win.Invalidate()
	}
}

// Window is the main window, nil before it opens.
func (ws *Workspace) Window() *mygo.Window { return ws.win }

// drain runs what Update queued and shows the toasts waiting.
func (ws *Workspace) drain(c *ui.Context) {
	ws.mu.Lock()
	queued, toasts := ws.queued, ws.toasts
	ws.queued, ws.toasts = nil, nil
	ws.mu.Unlock()
	for _, fn := range queued {
		fn()
	}
	for _, t := range toasts {
		c.AddToast(t)
	}
}

// CurrentView is what the main pane shows.
func (ws *Workspace) CurrentView() host.View { return ws.view }

// ShowView shows a page of the main pane.
func (ws *Workspace) ShowView(v host.View) {
	if v == host.ViewTrash {
		ws.refreshTrash()
	}
	if v == host.ViewLibrary || v == host.ViewSources {
		ws.SaveAll()
		ws.ReloadLibrary()
		ws.cites.CloseInspector()
	}
	ws.setView(v)
	if v == host.ViewDoc && ws.doc != nil {
		ws.doc.Focus()
	}
	if v == host.ViewLibrary {
		ws.cites.TableChanged()
	}
	ws.Invalidate()
}

// setView is what every change of the main pane goes through: the shell's
// overlays belong to the page they opened on.
func (ws *Workspace) setView(v host.View) {
	if ws.view == host.ViewPaper && v != host.ViewPaper {
		ws.papers.ClosePaper()
	}
	ws.closeOverlays()
	ws.view = v
}

// Current is the open document, "" when none.
func (ws *Workspace) Current() string {
	if ws.doc == nil {
		return ""
	}
	return ws.doc.ID()
}

// TextOf is what id holds now: an open editor's copy, or the file.
func (ws *Workspace) TextOf(id string) (string, error) {
	if ws.doc != nil && ws.doc.ID() == id {
		return ws.doc.Text(), nil
	}
	if notes := ws.papers.NotesEditor(id); notes != nil {
		return notes.Text(), nil
	}
	return ws.store.Load(id)
}

// read is TextOf for the project package, which reads bodies from memory.
func (ws *Workspace) read(path string) (string, bool) {
	if ws.doc != nil && ws.doc.ID() == path {
		return ws.doc.Text(), true
	}
	if e, ok := ws.Entry(path); ok && !e.Folder {
		return e.Body, true
	}
	return "", false
}

func (ws *Workspace) typstIDs() []string {
	var ids []string
	for _, e := range ws.entries {
		if e.Typst {
			ids = append(ids, e.ID)
		}
	}
	return ids
}

// Documents are the vault's documents and their includes.
func (ws *Workspace) Documents() *project.Documents {
	return project.LoadDocuments(ws.typstIDs(), ws.read)
}

// TopDocument is the document that includes id, at the top.
func (ws *Workspace) TopDocument(id string) string { return ws.Documents().TopDocument(id) }

// HasPaper tells whether the source key has a paper.
func (ws *Workspace) HasPaper(key string) bool { return ws.papers.HasPaper(key) }

// OpenPaper shows a paper.
func (ws *Workspace) OpenPaper(id string) { ws.papers.OpenPaper(id) }

// FetchPapers downloads the open-access papers of the sources.
func (ws *Workspace) FetchPapers(keys []string) { ws.papers.FetchPapers(keys) }

// AttachPaper copies a PDF from outside the vault in as paper id.
func (ws *Workspace) AttachPaper(path, id string) { ws.papers.AttachPaper(path, id) }

// OpenSourceNotes opens the notes of key, starting them if there are none.
func (ws *Workspace) OpenSourceNotes(key string) { ws.papers.OpenSourceNotes(key) }

// SyncBibliographies brings the embedded bibliographies up to date with the library.
func (ws *Workspace) SyncBibliographies() { ws.cites.SyncBibliographies() }

// RenameCitations follows a key change through every document that cites it.
func (ws *Workspace) RenameCitations(old, renamed string) { ws.cites.RenameCitations(old, renamed) }

// PickSource opens the source picker for a purpose.
func (ws *Workspace) PickSource(opts host.PickSourceOptions) { ws.cites.OpenSourcePicker(opts) }

// notesID is the paper panel's notes when they are open, else "".
func (ws *Workspace) notesID() string {
	if paper := ws.papers.Current(); paper != "" {
		id := library.NoteFile(paper)
		if ws.papers.NotesEditor(id) != nil {
			return id
		}
	}
	return ""
}

// QuoteTarget is where a quotation goes: the open document, else the paper panel's notes.
func (ws *Workspace) QuoteTarget() string {
	notes := ws.notesID()
	if ws.doc != nil && vault.IsDocument(ws.doc.ID()) && ws.doc.ID() != notes {
		return ws.doc.ID()
	}
	return notes
}

// InsertQuote puts a quotation into the QuoteTarget.
func (ws *Workspace) InsertQuote(markup string, block bool) {
	target := ws.QuoteTarget()
	switch {
	case target == "":
		ws.ToastError("Open a document to insert a quote")
	case ws.doc != nil && ws.doc.ID() == target:
		ws.doc.InsertQuote(markup, block)
	default:
		if notes := ws.papers.NotesEditor(target); notes != nil {
			notes.InsertQuote(markup, block)
		}
	}
}

// InsertCitation cites key at the caret of the open document.
func (ws *Workspace) InsertCitation(key string) {
	if ws.doc == nil {
		ws.ToastError("Open a document to insert a citation")
		return
	}
	id := ws.doc.ID()
	if !vault.IsDocument(id) {
		ws.ToastError("Citations go in Typst documents")
		return
	}
	ws.ShowView(host.ViewDoc)
	ws.doc.InsertCitation(key)
	ws.CitationAdded(id)
	ws.doc.Focus()
}

// CitationAdded embeds a bibliography where none covers id yet and saves.
func (ws *Workspace) CitationAdded(id string) {
	ws.cites.EnsureBibliography(id)
	ws.MarkUnsaved()
}

// PickFiles asks for files to open; nil when canceled.
func (ws *Workspace) PickFiles(ext string, multiple bool) ([]string, error) {
	opts := mygo.OpenDialogOptions{Parent: ws.win, Multiple: multiple}
	if ext != "" {
		opts.Filters = []mygo.FileFilter{{Name: strings.ToUpper(ext), Extensions: []string{ext}}}
	}
	paths, err := mygo.Dialog.Open(opts)
	if err != nil {
		return nil, fmt.Errorf("the open dialog: %w", err)
	}
	if len(paths) == 0 {
		return nil, nil
	}
	return paths, nil
}

// PickSavePath asks where to save a file named name; "" when canceled.
func (ws *Workspace) PickSavePath(dir, name string) (string, error) {
	path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{Parent: ws.win, DefaultPath: filepath.Join(dir, name), CreateDirectories: true})
	if err != nil {
		return "", fmt.Errorf("the save dialog: %w", err)
	}
	return path, nil
}

// Confirm asks a question with a danger or primary action; safe from any goroutine.
func (ws *Workspace) Confirm(title, body, action string, danger bool, then func()) {
	ws.Update(func() {
		ws.confirm = confirmState{open: true, title: title, body: body, action: action, danger: danger, then: then}
	})
}

// Reveal shows path in the system's file manager.
func (ws *Workspace) Reveal(path string) { mygo.Shell.ShowItemInFolder(path) }

// OpenExternally opens path in its default app.
func (ws *Workspace) OpenExternally(path string) {
	if err := mygo.Shell.OpenPath(path); err != nil {
		ws.ToastError(fmt.Sprintf("Could not open %s: %v", filepath.Base(path), err))
	}
}

// CopyText puts text on the clipboard.
func (ws *Workspace) CopyText(text string) { mygo.Clipboard.WriteText(text) }

// revealPath shows a vault file in the system's file manager.
func (ws *Workspace) revealPath(id string) {
	path, err := ws.store.Path(id)
	if err != nil {
		ws.ToastError(err.Error())
		return
	}
	ws.Reveal(path)
}
