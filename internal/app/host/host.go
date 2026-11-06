// Package host is what the workspace offers its feature packages (document,
// citations, papers): the vault, the library, the open document, navigation,
// toasts, dialogs. The workspace in internal/app implements it; the feature
// packages never import internal/app. Every method runs on the main thread
// unless its doc says otherwise.
package host

import (
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
)

// View is what the main pane shows.
type View int

const (
	ViewDoc View = iota
	ViewHome
	ViewLibrary
	ViewSources
	ViewPaper
	ViewTrash
	ViewSettings
)

// PickSourceOptions says what choosing a source in the picker is for.
type PickSourceOptions struct {
	// Prompt heads the picker, as "Choose the source for this paper"; "" for citing.
	Prompt string
	// ExcludeWithPaper leaves out sources that have a paper already.
	ExcludeWithPaper bool
	// Alternative labels a button under the list, as "Add Without a Source", with what it does.
	Alternative   string
	OnAlternative func()
	// Then runs with the chosen key.
	Then func(key string)
}

// Host is the workspace as its feature packages see it.
type Host interface {
	// The vault and what it holds.
	Store() *vault.Store
	// Entries is the vault's listing, most recently modified first. Read-only.
	Entries() []vault.Entry
	// Entry finds one by id.
	Entry(id string) (vault.Entry, bool)
	RefreshEntries()

	// Library is nil while library.yaml cannot be read; LibraryError says why.
	Library() *library.Library
	LibraryError() string
	ReloadLibrary()

	Prefs() *Preferences
	// SavePrefs writes the preferences; a failure is toasted.
	SavePrefs()

	// Toasts. Warn toasts the warnings joined, when there are any.
	Toast(msg string)
	ToastError(msg string)
	Warn(warnings []string)

	// Update runs fn on the main thread and draws a frame; safe from any
	// goroutine. Invalidate only draws a frame.
	Update(fn func())
	Invalidate()
	Window() *mygo.Window

	// Navigation.
	ShowView(v View)
	CurrentView() View
	// Open shows a document, toggles a folder, or opens a paper, saving first.
	Open(id string)
	GoToSource(path string, offset int)
	// FollowLink opens a linked file, or says that it is gone.
	FollowLink(path string)

	// The open document ("" when none) and what documents hold now: the open
	// editor's copy, or the file.
	Current() string
	TextOf(id string) (string, error)
	// WriteDocument is the one way text reaches a document: the file, its
	// entry, the editors showing it, and the preview. generated marks Thesio's
	// own text, outside the undo history.
	WriteDocument(id, text string, generated bool) error
	// MarkUnsaved writes the open document again after text changed without an edit.
	MarkUnsaved()
	// SaveAll flushes the open document and notes; false when a save failed.
	SaveAll() bool
	// WhenSaved runs then once everything is saved, or from the Save failed dialog.
	WhenSaved(then func())
	// PathChanged follows a rename or move through everything open.
	PathChanged(old, new string, rewritten []vault.Rewritten)
	KeepVersion(id, text string, now bool)

	// Documents and includes, read from the open editor and the entries.
	TopDocument(id string) string
	Documents() *project.Documents

	// Papers and citations, provided by their packages through the workspace.
	HasPaper(key string) bool
	OpenPaper(id string)
	FetchPapers(keys []string)
	// AttachPaper copies a PDF from outside the vault in as paper id.
	AttachPaper(path, id string)
	// OpenSourceNotes opens the notes of key as a document, starting them if there are none.
	OpenSourceNotes(key string)
	// PickSource opens the source picker (the citations package's) for a purpose.
	PickSource(opts PickSourceOptions)
	// InsertQuote puts a quotation into the quote target: the open document,
	// else the paper panel's notes. QuoteTarget is that document's id, "" when none.
	InsertQuote(markup string, block bool)
	QuoteTarget() string
	// InsertCitation cites key at the caret of the open editor, embedding a
	// bibliography if nothing covers the document yet.
	InsertCitation(key string)
	CitationAdded(id string)
	SyncBibliographies()
	RenameCitations(old, new string)

	// Dialogs, safe from any goroutine; they block until answered.
	// PickFiles returns nil when canceled; ext "" accepts any file.
	PickFiles(ext string, multiple bool) ([]string, error)
	// PickSavePath returns "" when canceled.
	PickSavePath(dir, name string) (string, error)
	// Confirm asks a question with a danger or primary action; true when chosen.
	Confirm(title, body, action string, danger bool, then func())

	// The system.
	Reveal(path string)
	OpenExternally(path string)
	CopyText(text string)
}
