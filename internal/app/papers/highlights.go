package papers

import (
	"errors"
	"os"
	"strings"
	"time"

	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/pdfkit"
	"github.com/egoist/mygo"
)

// ErrQuoteNotInPaper is copied text that the paper does not contain.
var ErrQuoteNotInPaper = errors.New("the copied text is not in this paper")

// clipboardText reads what the user copied; tests replace it.
var clipboardText = mygo.Clipboard.ReadText

// stamp tells one state of a file from another.
type stamp struct {
	mod  time.Time
	size int64
}

// indexed is a paper's highlights, read once for each state of its file.
type indexed struct {
	stamp   stamp
	read    bool
	loading bool
	list    []pdfkit.Highlight
	// labels are the page labels of the pages with highlights.
	labels map[int]string
}

type highlightKey struct{ page, index int }

func keyOf(h pdfkit.Highlight) highlightKey { return highlightKey{h.Page, h.Index} }

// Highlights are the highlights of paper id as last read. A changed file is
// read again on a goroutine, and the new list shows in a later frame.
func (s *State) Highlights(id string) []pdfkit.Highlight {
	if e := s.indexed(id); e != nil {
		return e.list
	}
	return nil
}

func (s *State) indexed(id string) *indexed {
	path, err := s.h.Store().Path(id)
	if err != nil {
		return nil
	}
	info, err := os.Stat(path)
	if err != nil {
		delete(s.cache, id)
		return nil
	}
	now := stamp{info.ModTime(), info.Size()}
	e := s.cache[id]
	if e == nil {
		e = &indexed{}
		s.cache[id] = e
	}
	if (!e.read || e.stamp != now) && !e.loading {
		e.loading = true
		go func() {
			list, labels := readHighlights(path)
			s.h.Update(func() {
				e.loading, e.read = false, true
				e.stamp, e.list, e.labels = now, list, labels
			})
		}()
	}
	return e
}

func readHighlights(path string) ([]pdfkit.Highlight, map[int]string) {
	if !pdfkit.Available() {
		return nil, nil
	}
	doc, err := pdfkit.Open(path)
	if err != nil {
		return nil, nil
	}
	defer doc.Close()
	list := doc.Highlights()
	labels := map[int]string{}
	for _, h := range list {
		if _, ok := labels[h.Page]; !ok {
			labels[h.Page] = doc.PageLabel(h.Page)
		}
	}
	return list, labels
}

// label is the page label of a highlight's page in paper id, as last read.
func (s *State) label(id string, page int) string {
	if e := s.cache[id]; e != nil {
		return e.labels[page]
	}
	return ""
}

// highlightPage is the page of a highlight in paper id, as printed.
func (s *State) highlightPage(id string, h pdfkit.Highlight) string {
	return library.PrintedPage(s.label(id, h.Page), h.Page, s.pageOffset(id))
}

// highlightSelection highlights the first occurrence of the copied text: the
// web view's selection cannot be read from outside it.
func (s *State) highlightSelection() {
	text := strings.TrimSpace(clipboardText())
	if text == "" {
		s.h.ToastError("Copy some text in the paper first")
		return
	}
	path, err := s.h.Store().Path(s.current)
	if err != nil {
		s.h.ToastError("Could not save the highlight: " + err.Error())
		return
	}
	go func() {
		page, err := addHighlight(path, text)
		s.h.Update(func() {
			if err != nil {
				s.h.ToastError("Could not save the highlight: " + err.Error())
				return
			}
			s.tab, s.panelHidden = tabHighlights, false
			s.reload(page)
		})
	}()
}

// addHighlight highlights text on the first page that holds it.
func addHighlight(path, text string) (int, error) {
	doc, err := pdfkit.Open(path)
	if err != nil {
		return 0, err
	}
	defer doc.Close()
	want := normalized(text)
	for i, n := 0, doc.PageCount(); i < n; i++ {
		if strings.Contains(normalized(doc.PageText(i)), want) {
			return i, doc.AddHighlight(i, text, "")
		}
	}
	return 0, ErrQuoteNotInPaper
}

// normalized matches as pdfkit's search does: any whitespace, any case.
func normalized(text string) string {
	return strings.ToLower(strings.Join(strings.Fields(text), " "))
}

// changeHighlight runs a change to a highlight of the current paper off the
// main thread, then shows the paper again with it.
func (s *State) changeHighlight(h pdfkit.Highlight, what string, change func(*pdfkit.Document) error) {
	path, err := s.h.Store().Path(s.current)
	if err != nil {
		s.h.ToastError("Could not save " + what + ": " + err.Error())
		return
	}
	go func() {
		doc, err := pdfkit.Open(path)
		if err == nil {
			err = change(doc)
			doc.Close()
		}
		s.h.Update(func() {
			if err != nil {
				s.h.ToastError("Could not save " + what + ": " + err.Error())
				return
			}
			s.reload(h.Page)
		})
	}()
}

func (s *State) setComment(h pdfkit.Highlight, comment string) {
	s.editing = nil
	s.changeHighlight(h, "the comment", func(d *pdfkit.Document) error { return d.SetComment(h, strings.TrimSpace(comment)) })
}

func (s *State) removeHighlight(h pdfkit.Highlight) {
	s.editing = nil
	s.changeHighlight(h, "the paper", func(d *pdfkit.Document) error { return d.RemoveHighlight(h) })
}
