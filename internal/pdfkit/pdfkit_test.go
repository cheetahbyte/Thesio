package pdfkit

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// writePDF writes a one-page PDF with two lines of Helvetica text.
func writePDF(t *testing.T) string {
	t.Helper()
	content := "BT /F1 18 Tf 72 720 Td (Hello highlight world) Tj 0 -24 Td (second line here) Tj ET"
	objects := []string{
		"<< /Type /Catalog /Pages 2 0 R >>",
		"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
		"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
		fmt.Sprintf("<< /Length %d >>\nstream\n%s\nendstream", len(content), content),
		"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
	}
	var b bytes.Buffer
	b.WriteString("%PDF-1.4\n")
	offsets := make([]int, len(objects))
	for i, o := range objects {
		offsets[i] = b.Len()
		fmt.Fprintf(&b, "%d 0 obj\n%s\nendobj\n", i+1, o)
	}
	xref := b.Len()
	fmt.Fprintf(&b, "xref\n0 %d\n0000000000 65535 f \n", len(objects)+1)
	for _, off := range offsets {
		fmt.Fprintf(&b, "%010d 00000 n \n", off)
	}
	fmt.Fprintf(&b, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(objects)+1, xref)
	path := filepath.Join(t.TempDir(), "paper.pdf")
	if err := os.WriteFile(path, b.Bytes(), 0o644); err != nil {
		t.Fatal(err)
	}
	return path
}

func open(t *testing.T, path string) *Document {
	t.Helper()
	d, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(d.Close)
	return d
}

func only(t *testing.T, d *Document) Highlight {
	t.Helper()
	hs := d.Highlights()
	if len(hs) != 1 {
		t.Fatalf("highlights = %+v, want one", hs)
	}
	return hs[0]
}

func TestHighlights(t *testing.T) {
	if !Available() {
		t.Skip("PDFKit is not available")
	}
	path := writePDF(t)
	d := open(t, path)
	if n := d.PageCount(); n != 1 {
		t.Fatalf("PageCount = %d", n)
	}
	if s := d.PageText(0); !strings.Contains(s, "Hello") {
		t.Fatalf("PageText = %q", s)
	}
	if d.PageText(3) != "" || d.PageLabel(3) != "" {
		t.Fatal("out of range page has text")
	}
	if err := d.AddHighlight(0, "highlight", "a note"); err != nil {
		t.Fatal(err)
	}
	if err := d.AddHighlight(0, "missing words", ""); err == nil {
		t.Fatal("highlighted text that is not there")
	}
	h := only(t, d)
	if h.Comment != "a note" || !strings.Contains(h.Text, "highlight") || h.Page != 0 {
		t.Fatalf("highlight = %+v", h)
	}
	h = only(t, open(t, path))
	if h.Comment != "a note" {
		t.Fatalf("reopened highlight = %+v", h)
	}
	if err := d.SetComment(h, "changed"); err != nil {
		t.Fatal(err)
	}
	if h = only(t, open(t, path)); h.Comment != "changed" {
		t.Fatalf("comment after SetComment = %q", h.Comment)
	}
	if err := d.RemoveHighlight(h); err != nil {
		t.Fatal(err)
	}
	if hs := d.Highlights(); len(hs) != 0 {
		t.Fatalf("after remove: %+v", hs)
	}
	if hs := open(t, path).Highlights(); len(hs) != 0 {
		t.Fatalf("reopened after remove: %+v", hs)
	}
}

func TestHighlightAcrossLines(t *testing.T) {
	if !Available() {
		t.Skip("PDFKit is not available")
	}
	path := writePDF(t)
	d := open(t, path)
	if err := d.AddHighlight(0, "world second", ""); err != nil {
		t.Fatal(err)
	}
	h := only(t, open(t, path))
	if !strings.Contains(h.Text, "world") || !strings.Contains(h.Text, "second") {
		t.Fatalf("text = %q", h.Text)
	}
}
