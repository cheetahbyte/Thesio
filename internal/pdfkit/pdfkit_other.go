//go:build !darwin

package pdfkit

import "errors"

// ErrUnavailable is every call off macOS.
var ErrUnavailable = errors.New("pdfkit: PDFKit is only available on macOS")

// Document stands in for PDFKit's, which only macOS has.
type Document struct{}

// Available reports false: there is no PDFKit here.
func Available() bool { return false }

// Open fails with ErrUnavailable.
func Open(string) (*Document, error) { return nil, ErrUnavailable }

// Close does nothing.
func (d *Document) Close() {}

// PageCount is 0.
func (d *Document) PageCount() int { return 0 }

// PageLabel is "".
func (d *Document) PageLabel(int) string { return "" }

// PageText is "".
func (d *Document) PageText(int) string { return "" }

// Highlights is none.
func (d *Document) Highlights() []Highlight { return nil }

// AddHighlight fails with ErrUnavailable.
func (d *Document) AddHighlight(int, string, string) error { return ErrUnavailable }

// SetComment fails with ErrUnavailable.
func (d *Document) SetComment(Highlight, string) error { return ErrUnavailable }

// RemoveHighlight fails with ErrUnavailable.
func (d *Document) RemoveHighlight(Highlight) error { return ErrUnavailable }
