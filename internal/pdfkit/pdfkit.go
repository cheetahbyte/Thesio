// Package pdfkit reads and annotates PDFs through Apple's PDFKit, driven with
// purego (no cgo). There is no view: highlights are placed by finding text on a
// page. Highlights across a hyphenated line break are not found.
package pdfkit

// Highlight is a highlight annotation. Index is its position among the page's
// annotations, so SetComment and RemoveHighlight find it again; it shifts when
// an earlier annotation on the page is removed, so re-read after a change.
type Highlight struct {
	Page          int
	Text, Comment string
	Index         int
}
