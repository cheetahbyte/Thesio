package papers

import (
	"errors"
	"fmt"
	"io/fs"
	"os"

	"github.com/cheetahbyte/thesio/internal/app/document"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/typst"
)

// ErrNotesUnwritten is notes the editor could not write.
var ErrNotesUnwritten = errors.New("the file could not be written")

// notes are the open paper's notes: id follows the file, which appears once
// something is written.
type notes struct {
	id  string
	doc *document.Open
	// exists: the file is in the vault, so the next save is not its first.
	exists bool
	// err is why the last save failed, for the Save failed dialog.
	err string
}

// notesStart is what a paper's notes begin with.
func (s *State) notesStart(paper string) string {
	return "= " + s.PaperTitle(paper) + "\n\n"
}

// loadNotes opens the notes of paper, or a heading to start them with.
func (s *State) loadNotes(paper string) {
	id := library.NoteFile(paper)
	path, err := s.h.Store().Path(id)
	if err != nil {
		s.h.ToastError("Could not read the notes: " + err.Error())
		return
	}
	_, err = os.Stat(path)
	exists := err == nil
	text := s.notesStart(paper)
	if exists {
		loaded, readErr := s.h.TextOf(id)
		if readErr != nil {
			// No editor: the next keystroke would replace a file nobody could read.
			s.h.ToastError("Could not read the notes: " + readErr.Error())
			return
		}
		text = loaded
		s.h.KeepVersion(id, text, true)
	} else if !errors.Is(err, fs.ErrNotExist) {
		s.h.ToastError("Could not read the notes: " + err.Error())
		return
	}
	n := &notes{id: id, exists: exists}
	n.doc = document.NewOpen(s.h, id, text, id, false)
	n.doc.OnChange = func(string) { s.notesChanged(n) }
	n.doc.OnSaved = func(id, text string) { s.notesSaved(n, id, text) }
	n.doc.OnCitationCompleted = func() { s.LinkNoteLibrary(n.id) }
	s.notes = n
}

func (s *State) closeNotes() {
	if s.notes != nil {
		s.notes.doc.Close()
		s.notes = nil
	}
}

// notesChanged makes room for a first save: the folder may not be there yet.
func (s *State) notesChanged(n *notes) {
	if n.exists {
		return
	}
	if err := s.notesDir(); err != nil {
		s.notesFailed(n, err)
	}
}

func (s *State) notesSaved(n *notes, id, text string) {
	n.err = ""
	if !n.exists {
		n.exists = true
		s.h.RefreshEntries()
	}
	s.h.KeepVersion(id, text, false)
}

func (s *State) notesFailed(n *notes, err error) {
	n.err = "Could not save the notes: " + err.Error()
	s.h.ToastError(n.err)
}

func (s *State) notesDir() error {
	dir, err := s.h.Store().Path(library.NotesDir)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("create the notes folder: %w", err)
	}
	return nil
}

// NotesEditor is the panel's notes when they are id, else nil.
func (s *State) NotesEditor(id string) *document.Open {
	if s.notes == nil || s.notes.id != id {
		return nil
	}
	return s.notes.doc
}

// FlushNotes writes the panel's notes if they changed. False when that
// failed: they stay pending, and a later save retries them.
func (s *State) FlushNotes() bool {
	n := s.notes
	if n == nil || !n.doc.Unsaved() {
		return true
	}
	if err := s.notesDir(); err != nil {
		s.notesFailed(n, err)
		return false
	}
	if !n.doc.Flush() {
		s.notesFailed(n, ErrNotesUnwritten)
		return false
	}
	if !n.exists {
		n.exists = true
		s.h.RefreshEntries()
	}
	n.err = ""
	return true
}

// DiscardNotes gives the panel's pending notes up, for Discard changes.
func (s *State) DiscardNotes() {
	if s.notes != nil {
		s.notes.doc.Discard()
		s.notes.err = ""
	}
}

// NotesError is why the last notes save failed, "" when it did not.
func (s *State) NotesError() string {
	if s.notes == nil {
		return ""
	}
	return s.notes.err
}

// writeNotes writes a notes file, which may be new.
func (s *State) writeNotes(id, text string) error {
	_, known := s.h.Entry(id)
	if err := s.notesDir(); err != nil {
		return err
	}
	if err := s.h.WriteDocument(id, text, true); err != nil {
		return err
	}
	if !known {
		s.h.RefreshEntries()
	}
	if s.notes != nil && s.notes.id == id {
		s.notes.exists = true
	}
	return nil
}

// LinkNoteLibrary links notes id that cite to library.yaml when they have no
// bibliography, so they compile on their own.
func (s *State) LinkNoteLibrary(id string) {
	var text string
	if d := s.NotesEditor(id); d != nil {
		text = d.Text()
	} else {
		t, err := s.h.TextOf(id)
		if err != nil {
			s.h.ToastError("Could not read the notes: " + err.Error())
			return
		}
		text = t
	}
	if typst.BibliographyMode(text, id) != typst.BibNone {
		return
	}
	if err := s.writeNotes(id, typst.LinkBibliography(text, id)); err != nil {
		s.h.ToastError("Could not save the notes: " + err.Error())
	}
}

// OpenSourceNotes opens the notes of key as a document, starting them if there are none.
func (s *State) OpenSourceNotes(key string) {
	paper := library.PaperFile(key)
	id := library.NoteFile(paper)
	if _, ok := s.h.Entry(id); !ok {
		if err := s.writeNotes(id, s.notesStart(paper)); err != nil {
			s.h.ToastError("Could not create the notes: " + err.Error())
			return
		}
	}
	s.h.Open(id)
}
