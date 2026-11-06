// Package papers is the Sources page, the paper viewer with its notes and
// highlights panel, and quoting a highlight into a document. Papers are the
// PDFs in `sources/`, each following its entry in the Library; notes are
// `notes/<key>.typ`; highlights are annotations in the PDF itself.
package papers

import (
	"io"
	"os"
	"slices"
	"strconv"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

const (
	tabNotes = iota
	tabHighlights
)

// State is the open paper, the viewer and its panel, and the downloads under way.
type State struct {
	h host.Host
	// current is the vault id of the paper the Paper view shows.
	current string
	web     *mygo.WebView
	// shown is the file the web view loaded last; version counts its reloads.
	shown   string
	version int

	panelHidden bool
	tab         int
	notes       *notes
	// editing is the highlight whose comment is being written, and the text.
	editing *highlightKey
	comment string

	cache map[string]*indexed

	quote   *quoteDialog
	quoting bool

	sources ui.ListState
	chosen  int
	// fetching counts papers still downloading.
	fetching int
}

// New makes the papers state of a workspace.
func New(h host.Host) *State {
	s := &State{h: h, chosen: -1, cache: map[string]*indexed{}}
	s.sources.Selected = &s.chosen
	return s
}

// Current is the paper the Paper view shows, "" when none.
func (s *State) Current() string { return s.current }

// PaperIDs are the ids of the PDFs in `sources/`, sorted.
func (s *State) PaperIDs() []string {
	var ids []string
	for _, e := range s.h.Entries() {
		if !e.Folder && e.HasParent && e.Parent == library.PapersDir && vault.IsPDF(e.ID) {
			ids = append(ids, e.ID)
		}
	}
	slices.Sort(ids)
	return ids
}

// HasPaper reports whether the source key has its paper in the vault.
func (s *State) HasPaper(key string) bool {
	_, ok := s.h.Entry(library.PaperFile(key))
	return ok
}

// PaperRow is the source a paper belongs to.
func (s *State) PaperRow(id string) (library.Row, bool) {
	lib := s.h.Library()
	if lib == nil {
		return library.Row{}, false
	}
	return library.RowOfPaper(lib.Rows(), id)
}

// PaperTitle is the name a paper goes by: its source's title, or its file name.
func (s *State) PaperTitle(id string) string {
	if row, ok := s.PaperRow(id); ok && row.Title != "" {
		return row.Title
	}
	return vault.BaseName(id)
}

// OpenPaper shows paper id once everything open is saved.
func (s *State) OpenPaper(id string) {
	s.h.WhenSaved(func() {
		if s.current != id {
			s.closeNotes()
		}
		s.current = id
		s.chosen = -1
		s.editing = nil
		s.ensureWebView()
		s.show(-1)
		if s.notes == nil {
			s.loadNotes(id)
		}
		s.h.ShowView(host.ViewPaper)
	})
}

// ClosePaper leaves the paper, writing its notes first.
func (s *State) ClosePaper() {
	if s.current == "" {
		return
	}
	s.FlushNotes()
	s.closeNotes()
	s.current = ""
	s.editing = nil
	s.quote, s.quoting = nil, false
}

// AttachPaper copies a PDF from outside the vault in as paper id.
func (s *State) AttachPaper(path, id string) {
	if !isPDF(path) {
		s.h.ToastError("That file is not a PDF")
		return
	}
	if err := s.h.Store().Attach(path, id); err != nil {
		s.h.ToastError("Could not add the paper: " + err.Error())
		return
	}
	s.h.RefreshEntries()
	s.h.Toast("Added the paper")
}

func isPDF(path string) bool {
	f, err := os.Open(path)
	if err != nil {
		return false
	}
	defer f.Close()
	head := make([]byte, len(vault.PDFHeader))
	_, err = io.ReadFull(f, head)
	return err == nil && string(head) == vault.PDFHeader
}

// LinkPaper makes an unlinked paper, and any notes on it, the paper of key.
func (s *State) LinkPaper(id, key string) {
	to := library.PaperFile(key)
	s.followKey(id, to)
	s.followKey(library.NoteFile(id), library.NoteFile(to))
}

// RenamePaper keeps a paper and its notes following their entry when its key changes.
func (s *State) RenamePaper(old, new string) {
	from, to := library.PaperFile(old), library.PaperFile(new)
	s.followKey(from, to)
	s.followKey(library.NoteFile(from), library.NoteFile(to))
}

// followKey renames a file that is named after a citation key.
func (s *State) followKey(from, to string) {
	if from == to {
		return
	}
	if _, ok := s.h.Entry(from); !ok {
		return
	}
	moved, err := s.h.Store().Rename(from, vault.BaseName(to))
	if err != nil {
		s.h.ToastError("Could not rename " + from + ": " + err.Error())
		return
	}
	s.h.PathChanged(from, moved.ID, moved.Rewritten)
	s.h.Warn(moved.Warnings)
}

// PathChanged follows a rename or move of old to new through the paper, its
// notes and the highlights read so far.
func (s *State) PathChanged(old, new string) {
	if p, ok := vault.MovedPath(s.current, old, new); ok && s.current != "" {
		s.current = p
		s.shown = ""
		s.show(-1)
	}
	if s.notes != nil {
		if p, ok := vault.MovedPath(s.notes.id, old, new); ok {
			s.notes.id = p
			s.notes.doc.SetPath(p, p)
		}
	}
	if s.quote != nil {
		if p, ok := vault.MovedPath(s.quote.paper, old, new); ok {
			s.quote.paper = p
		}
	}
	for id, e := range s.cache {
		if p, ok := vault.MovedPath(id, old, new); ok {
			delete(s.cache, id)
			s.cache[p] = e
		}
	}
}

// pageOffset is the paper's printed page numbers minus the ones it reports.
func (s *State) pageOffset(id string) int {
	if e, ok := s.h.Entry(id); ok && e.Meta.PageOffset != nil {
		return *e.Meta.PageOffset
	}
	return 0
}

// header heads a page: its title, a count when there is one, and actions at the end.
func (s *State) header(c *ui.Context, title string, count int, actions func()) {
	s.h.Bar(c, func() {
		theme.Title(c, title)
		if count > 0 {
			theme.Count(c, strconv.Itoa(count))
		}
		ui.Spacer(c)
		actions()
	})
}
