// Package pdfkit reads and annotates PDFs through Apple's PDFKit, driven with
// purego (no cgo). There is no view: highlights are placed by finding text on a
// page. Highlights across a hyphenated line break are not found.
package pdfkit

import "errors"

// Errors the documents return, matched with errors.Is. Details follow their text.
var (
	ErrCannotRead         = errors.New("pdfkit: cannot read")
	ErrCannotWrite        = errors.New("pdfkit: cannot write")
	ErrClosed             = errors.New("pdfkit: document is closed")
	ErrNoPage             = errors.New("pdfkit: no page")
	ErrNothingToHighlight = errors.New("pdfkit: nothing to highlight")
	ErrHighlightNotFound  = errors.New("pdfkit: highlight not found")
	ErrTextNotFound       = errors.New("not found")
	ErrTextHasNoArea      = errors.New("has no area")
)

// Highlight is a highlight annotation. Index is its position among the page's
// annotations, so SetComment and RemoveHighlight find it again; it shifts when
// an earlier annotation on the page is removed, so re-read after a change.
type Highlight struct {
	Page          int
	Text, Comment string
	Index         int
}
