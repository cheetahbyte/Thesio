// Package citations is the Library: the table of the citation library, the
// inspector that edits an entry, tags, the source picker, adding by
// identifier, and keeping documents' bibliographies in step with the library.
package citations

import (
	"fmt"
	"maps"
	"os"
	"slices"
	"strings"
	"time"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

// State is the Library's state: the table, its selection and sort, the tag
// filter, the inspector, and the dialogs.
type State struct {
	h host.Host

	// ToggleBlock shows or hides the open editor's bibliography block, and
	// BlockShown says whether it shows; the shell sets them. Without
	// ToggleBlock, the document menu leaves the item out.
	ToggleBlock func()
	BlockShown  func() bool
	// OpenHistory opens the shell's Version History dialog on the library.
	OpenHistory func()

	search    string
	tagFilter string
	list      ui.ListState
	chosen    ui.Selection[string]
	cursor    int
	sort      ui.SortOrder
	widths    map[string]float32
	widthsAt  time.Time
	insp      *inspector
	first     ui.Handle
	picker    picker
	lookup    lookup
	tagEditor tagEditor
	tagRename tagRename
}

// New makes the Library's state over the workspace.
func New(h host.Host) *State {
	s := &State{h: h, cursor: -1, sort: ui.SortOrder{Column: colTitle}}
	s.list.Columns.Widths = savedWidths(h.Prefs().LibraryColumns)
	s.widths = maps.Clone(s.list.Columns.Widths)
	return s
}

func (s *State) library() *library.Library { return s.h.Library() }

// refresh follows a change to library.yaml: the sidebar and the bibliographies.
func (s *State) refresh() {
	s.h.RefreshEntries()
	s.SyncBibliographies()
}

// VisibleRows are the rows query finds, by title ascending. query is lowercase.
func (s *State) VisibleRows(query string) []library.Row {
	return s.sortedRows(query, ui.SortOrder{Column: colTitle})
}

func (s *State) sortedRows(query string, order ui.SortOrder) []library.Row {
	lib := s.library()
	if lib == nil {
		return nil
	}
	var rows []library.Row
	for _, row := range lib.Rows() {
		if query == "" || row.Matches(query) {
			rows = append(rows, row)
		}
	}
	slices.SortStableFunc(rows, func(a, b library.Row) int {
		n := vault.NaturalCompare(cell(a, order.Column), cell(b, order.Column))
		if order.Descending {
			return -n
		}
		return n
	})
	return rows
}

// rows are the table's rows in its order, with the search and the tag filter applied.
func (s *State) rows() []library.Row {
	rows := s.sortedRows(strings.ToLower(strings.TrimSpace(s.search)), s.sort)
	if s.tagFilter != "" {
		rows = slices.DeleteFunc(rows, func(r library.Row) bool { return !slices.Contains(r.Tags, s.tagFilter) })
	}
	return rows
}

// selectedIn are the chosen keys among rows, in their order. Rows a search or
// filter hides stay out, so nothing acts on an entry the user cannot see.
func (s *State) selectedIn(rows []library.Row) []string {
	var keys []string
	for _, row := range rows {
		if s.chosen.Has(row.Key) {
			keys = append(keys, row.Key)
		}
	}
	return keys
}

func (s *State) selectOnly(key string) {
	s.chosen.Clear()
	s.chosen.Add(key)
	s.cursor = slices.IndexFunc(s.rows(), func(r library.Row) bool { return r.Key == key })
}

// TableChanged follows what changes the table's rows: a filter on a tag that
// is gone ends, and the inspector matches the selection.
func (s *State) TableChanged() {
	if s.tagFilter != "" {
		var tags []library.TagCount
		if lib := s.library(); lib != nil {
			tags = lib.Tags()
		}
		if !slices.ContainsFunc(tags, func(t library.TagCount) bool { return t.Name == s.tagFilter }) {
			s.tagFilter = ""
		}
	}
	s.syncInspector()
}

func (s *State) deleteEntries(keys []string) {
	lib := s.library()
	if lib == nil {
		return
	}
	if err := lib.DeleteEntries(keys); err != nil {
		s.h.ToastError(fmt.Sprintf("Could not delete entry: %v", err))
		return
	}
	for _, key := range keys {
		s.chosen.Remove(key)
	}
	s.refresh()
	s.TableChanged()
}

// askDelete confirms deleting keys, saying how many documents cite them.
func (s *State) askDelete(keys []string) {
	if len(keys) == 0 {
		return
	}
	citing := 0
	for _, e := range s.h.Entries() {
		if !e.Typst {
			continue
		}
		cited := citedKeys(e.Body)
		if slices.ContainsFunc(keys, func(k string) bool { return slices.Contains(cited, k) }) {
			citing++
		}
	}
	it, title := "them", fmt.Sprintf("Delete %d entries?", len(keys))
	if len(keys) == 1 {
		it, title = "it", fmt.Sprintf("Delete %s?", keys[0])
	}
	effect := fmt.Sprintf("No document cites %s.", it)
	switch {
	case citing == 1:
		effect = fmt.Sprintf("1 document cites %s and will show an error.", it)
	case citing > 1:
		effect = fmt.Sprintf("%d documents cite %s and will show an error.", citing, it)
	}
	body := fmt.Sprintf("This removes %s from the library and cannot be undone. %s", it, effect)
	keys = slices.Clone(keys)
	s.h.Confirm(title, body, "Delete", true, func() { s.deleteEntries(keys) })
}

// LibraryRestored takes in a library version: the inspector, the selection,
// the documents and the open file follow it.
func (s *State) LibraryRestored(text string) {
	s.insp = nil
	s.chosen.Clear()
	s.refresh()
	if s.h.Current() == vault.LibraryFile {
		if err := s.h.WriteDocument(vault.LibraryFile, text, false); err != nil {
			s.h.ToastError(fmt.Sprintf("Could not show the restored library: %v", err))
		}
	}
	s.TableChanged()
}

// OpenLibraryHistory opens Version History on library.yaml, through OpenHistory.
func (s *State) OpenLibraryHistory() {
	if s.library() != nil && s.OpenHistory != nil {
		s.OpenHistory()
	}
}

func (s *State) importBib() {
	go func() {
		paths, err := s.h.PickFiles("bib", true)
		s.h.Update(func() {
			if err != nil {
				s.h.ToastError(fmt.Sprintf("Could not choose a file: %v", err))
				return
			}
			if paths == nil {
				return
			}
			added, skipped := 0, 0
			for _, path := range paths {
				a, k, err := s.importFile(path)
				if err != nil {
					s.h.ToastError(fmt.Sprintf("Could not import %s: %v", path, err))
					continue
				}
				added, skipped = added+a, skipped+k
			}
			if added+skipped > 0 {
				s.refresh()
				already := ""
				if skipped > 0 {
					already = fmt.Sprintf(", %d already in the library", skipped)
				}
				s.h.Toast(fmt.Sprintf("Imported %d entries%s", added, already))
			}
			s.TableChanged()
		})
	}()
}

func (s *State) importFile(path string) (added, skipped int, err error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return 0, 0, fmt.Errorf("read the file: %w", err)
	}
	lib := s.library()
	if lib == nil {
		return 0, 0, host.ErrNoLibrary
	}
	return lib.ImportBiblatex(string(data))
}

func (s *State) exportBib() {
	lib := s.library()
	if lib == nil {
		return
	}
	text, root := lib.ExportBiblatex(), s.h.Store().Root()
	go func() {
		path, err := s.h.PickSavePath(root, "library.bib")
		if err == nil && path != "" {
			err = os.WriteFile(path, []byte(text), 0o644)
		}
		s.h.Update(func() {
			switch {
			case err != nil:
				s.h.ToastError(fmt.Sprintf("Could not export: %v", err))
			case path != "":
				s.h.Toast("Exported BibLaTeX")
			}
		})
	}()
}

func (s *State) findMissingPapers() {
	lib := s.library()
	if lib == nil {
		return
	}
	var keys []string
	for _, row := range lib.Rows() {
		keys = append(keys, row.Key)
	}
	s.h.FetchPapers(keys)
}

// attachPaper asks for a PDF and copies it in as key's paper.
func (s *State) attachPaper(key string) {
	go func() {
		paths, err := s.h.PickFiles("pdf", false)
		s.h.Update(func() {
			if err != nil {
				s.h.ToastError(fmt.Sprintf("Could not choose a file: %v", err))
			} else if len(paths) > 0 {
				s.h.AttachPaper(paths[0], library.PaperFile(key))
			}
		})
	}()
}

// renamePaper follows a key change to the paper and its notes, which are named after it.
func (s *State) renamePaper(old, renamed string) {
	from, to := library.PaperFile(old), library.PaperFile(renamed)
	s.followKey(from, to)
	s.followKey(library.NoteFile(from), library.NoteFile(to))
}

func (s *State) followKey(from, to string) {
	if from == to {
		return
	}
	if _, ok := s.h.Entry(from); !ok {
		return
	}
	moved, err := s.h.Store().Rename(from, vault.BaseName(to))
	if err != nil {
		s.h.ToastError(fmt.Sprintf("Could not rename %s: %v", from, err))
		return
	}
	s.h.PathChanged(from, moved.ID, moved.Rewritten)
	s.h.Warn(moved.Warnings)
}

func (s *State) canCite() bool {
	return vault.IsDocument(s.h.Current())
}
