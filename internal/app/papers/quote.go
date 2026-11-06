package papers

import (
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/pdfkit"
	"github.com/egoist/mygo/ui"
)

// quoteDialog is the review step before a highlight enters a document: PDF text can be wrong.
type quoteDialog struct {
	paper     string
	highlight pdfkit.Highlight
	label     string
	key       string
	// page is the page as first shown, to tell a correction from a confirmation.
	page    string
	text    string
	entered string
}

func (s *State) openQuote(h pdfkit.Highlight) {
	row, ok := s.PaperRow(s.current)
	if !ok {
		s.h.ToastError("Link this paper to a source to quote it")
		return
	}
	page := s.highlightPage(s.current, h)
	s.quote = &quoteDialog{
		paper: s.current, highlight: h, label: s.label(s.current, h.Page), key: row.Key,
		page: page, text: library.CleanQuote(h.Text), entered: page,
	}
	s.quoting = true
}

// quoteTarget names the document a quote goes into.
func (s *State) quoteTarget() string {
	if id := s.h.QuoteTarget(); id != "" {
		if e, ok := s.h.Entry(id); ok {
			return e.Title
		}
		return id
	}
	return "the notes"
}

func (s *State) insertQuote() {
	q := s.quote
	if q == nil || strings.TrimSpace(q.text) == "" {
		return
	}
	target := s.h.QuoteTarget()
	if target == "" {
		return
	}
	entered := strings.TrimSpace(q.entered)
	s.quote, s.quoting = nil, false
	if entered != q.page {
		if offset, ok := library.PageOffset(q.label, q.highlight.Page, entered); ok {
			if err := s.h.Store().SetPageOffset(q.paper, offset); err != nil {
				s.h.ToastError("Could not save the page numbering: " + err.Error())
			}
			s.h.RefreshEntries()
		}
	}
	markup, block := library.QuoteMarkup(q.text, q.key, entered)
	s.h.InsertQuote(markup, block)
	if s.NotesEditor(target) != nil {
		s.LinkNoteLibrary(target)
	} else {
		s.h.CitationAdded(target)
	}
	s.h.Toast("Added the quote to " + s.quoteTarget())
}

// Overlays builds the quote dialog while it is open.
func (s *State) Overlays(c *ui.Context) {
	if s.quote == nil {
		return
	}
	q := s.quote
	t := c.Theme()
	theme.Dialog(ui.Modal(c, &s.quoting, func() {
		ui.Column(c).Width(480).Gap(10).AlignItems(ui.Stretch).Children(func() {
			ui.Text(c, "Insert Quote").FontSize(15).FontWeight(600)
			ui.Text(c, "Text read from a PDF can contain errors, so check it against the paper. It goes into "+s.quoteTarget()+", cited as @"+q.key+".").FontSize(13).TextColor(theme.Caption(t))
			ui.TextArea(c, &q.text).Label("Quotation").Height(160).AutoFocus()
			ui.Row(c).Gap(10).Children(func() {
				ui.Text(c, "Page")
				ui.TextInput(c, &q.entered).Label("Page").Placeholder("None").Width(96).OnSubmit(s.insertQuote)
				ui.Text(c, "Correct a number once and the rest of this paper follows.").FontSize(12).TextColor(theme.Caption(t)).Grow(1)
			})
			ui.Row(c).Gap(8).Justify(ui.End).Margin(8, 0, 0).Children(func() {
				if ui.Button(c, "Cancel").Clicked() {
					s.quoting = false
				}
				ui.PrimaryButton(c, "Insert").OnClick(s.insertQuote)
			})
		})
	}), c.Theme())
	if !s.quoting {
		s.quote = nil
	}
}
