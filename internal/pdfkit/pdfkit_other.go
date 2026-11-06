//go:build !darwin

package pdfkit

import "errors"

var errUnsupported = errors.New("pdfkit: PDFKit is only available on macOS")

type Document struct{}

func Available() bool { return false }

func Open(path string) (*Document, error) { return nil, errUnsupported }

func (d *Document) Close() {}

func (d *Document) PageCount() int { return 0 }

func (d *Document) PageLabel(i int) string { return "" }

func (d *Document) PageText(i int) string { return "" }

func (d *Document) Highlights() []Highlight { return nil }

func (d *Document) AddHighlight(page int, text, comment string) error { return errUnsupported }

func (d *Document) SetComment(h Highlight, comment string) error { return errUnsupported }

func (d *Document) RemoveHighlight(h Highlight) error { return errUnsupported }
