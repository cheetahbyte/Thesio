package document

import (
	"errors"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
)

const (
	debounce = 120 * time.Millisecond
	// ownScale is pixels per point for the open file's pages: sharp on a 2x
	// screen at full page width. Pages of other files only need to read as pages.
	ownScale   = 2.0
	otherScale = 0.5
)

// page is a page of the document, ready to draw.
type page struct {
	key           string
	bitmap        *ui.Bitmap
	width, height float32
	own           bool
}

// session is one document's compiles: one at a time on a goroutine, the
// newest winning, published on the main thread.
type session struct {
	h        host.Host
	compiler *typst.Compiler
	render   bool
	latest   atomic.Uint64
	work     sync.Mutex

	// newest is the newest result; document the last one with a document,
	// which the pages, words and questions come from.
	newest, document *typst.Compilation
	diagnostics      []typst.Diagnostic
	pages            []page
	took             time.Duration
	compiled         bool
	published        int
	// downloading are packages being fetched; tried those fetched since the last edit.
	downloading map[string]bool
	tried       map[string]bool
}

func newSession(h host.Host, root string, render bool) *session {
	return &session{h: h, compiler: typst.NewCompiler(root), render: render, downloading: map[string]bool{}, tried: map[string]bool{}}
}

// knowledge is what answers questions about the text: the last good compile,
// since one that failed stopped before reading most of what is imported.
func (s *session) knowledge() *typst.Compilation {
	if s.document != nil {
		return s.document
	}
	return s.newest
}

func (s *session) close() {
	s.latest.Add(1)
	go s.compiler.Close()
}

// compile compiles the text as it is now, superseding any compile pending.
func (d *Open) compile(wait bool) {
	s := d.s
	gen := s.latest.Add(1)
	path, main, text := d.id, d.main, d.Text()
	known := map[string]*ui.Bitmap{}
	for _, p := range s.pages {
		known[p.key] = p.bitmap
	}
	go func() {
		if wait {
			time.Sleep(debounce)
		}
		if s.latest.Load() != gen {
			return
		}
		s.work.Lock()
		defer s.work.Unlock()
		if s.latest.Load() != gen {
			return
		}
		started := time.Now()
		var k *typst.Compilation
		err := s.compiler.SetBuffer(path, text)
		if err == nil {
			k, err = s.compiler.Compile(main)
		}
		var pages []page
		if err == nil && s.render && k.OK {
			pages = renderPages(k, path, known)
		}
		took := time.Since(started)
		s.h.Update(func() {
			if s.latest.Load() == gen && !d.closed {
				d.publish(k, err, pages, took)
			}
		})
	}()
}

// renderPages renders the pages whose content changed, and takes the others
// from known.
func renderPages(k *typst.Compilation, path string, known map[string]*ui.Bitmap) []page {
	own := k.PagesFrom(path)
	pages := make([]page, 0, len(k.Pages))
	for i, info := range k.Pages {
		mine := i >= len(own) || own[i]
		scale := float32(otherScale)
		if mine {
			scale = ownScale
		}
		key := fmt.Sprintf("%s@%g", info.Hash, scale)
		bitmap := known[key]
		if bitmap == nil {
			img, err := k.Render(i, scale)
			if err != nil {
				continue
			}
			bitmap = ui.NewBitmap(img)
			known[key] = bitmap
		}
		pages = append(pages, page{key: key, bitmap: bitmap, width: info.Width, height: info.Height, own: mine})
	}
	return pages
}

// publish takes in a compile: its diagnostics, and its pages when it
// produced a document; a failed one keeps the last good pages on screen.
func (d *Open) publish(k *typst.Compilation, err error, pages []page, took time.Duration) {
	s := d.s
	s.published++
	s.compiled = false
	if err != nil {
		s.diagnostics = []typst.Diagnostic{{Error: true, Message: err.Error(), Start: -1, End: -1, Line: -1}}
		d.ed.SetDiagnostics(d.buf.Version(), nil)
		return
	}
	s.diagnostics = k.Diagnostics
	s.newest = k
	if k.OK {
		s.document = k
		s.took, s.compiled = took, true
		if s.render {
			s.pages = pages
		}
	}
	d.fetchMissing(k)
	d.placeDiagnostics()
	if s.render {
		d.compiledPages()
	}
}

// fetchMissing downloads what a compile stopped at, off the compile so typing
// goes on compiling, and compiles again when each download ends.
func (d *Open) fetchMissing(k *typst.Compilation) {
	s := d.s
	for _, spec := range k.Missing {
		if s.downloading[spec] || s.tried[spec] {
			continue
		}
		s.downloading[spec], s.tried[spec] = true, true
		go func() {
			k.Fetch(spec)
			d.h.Update(func() {
				delete(s.downloading, spec)
				if !d.closed {
					d.compile(false)
				}
			})
		}()
	}
}

// placeDiagnostics underlines the newest compile's problems in this file, on the text as it is now.
func (d *Open) placeDiagnostics() {
	k := d.s.newest
	if k == nil {
		return
	}
	text, version := d.Text(), d.buf.Version()
	var out []editor.Diagnostic
	for _, p := range k.Placed(d.id, text) {
		if p.Start < 0 {
			continue
		}
		severity := editor.SeverityWarning
		switch {
		case p.Error:
			severity = editor.SeverityError
		case strings.HasPrefix(p.Message, "Downloading"):
			severity = editor.SeverityInfo
		}
		msg := p.Message
		for _, hint := range p.Hints {
			msg += "\nHint: " + hint
		}
		out = append(out, editor.Diagnostic{Start: p.Start, End: max(p.End, p.Start), Severity: severity, Message: msg, Source: "typst"})
	}
	d.ed.SetDiagnostics(version, out)
}

// ErrNotCompiled is a document that did not compile; ErrPackagesUnfetched one
// whose packages could not be fetched.
var (
	ErrNotCompiled       = errors.New("the document did not compile")
	ErrPackagesUnfetched = errors.New("the packages the document needs could not be fetched")
)

// CompileError is a document's first error, in Typst's words. It is an ErrNotCompiled.
type CompileError struct{ Message string }

// Error is the message.
func (e *CompileError) Error() string { return e.Message }

// Unwrap makes a CompileError an ErrNotCompiled.
func (e *CompileError) Unwrap() error { return ErrNotCompiled }

// fetchAttempts bounds how often ExportPDF fetches packages and compiles again.
const fetchAttempts = 8

// ExportPDF compiles id of the vault at root with text as its content, fetching
// the packages it needs, and exports it as PDF. The first error is the error.
func ExportPDF(root, id, text string) ([]byte, error) {
	c := typst.NewCompiler(root)
	defer c.Close()
	if err := c.SetBuffer(id, text); err != nil {
		return nil, err
	}
	tried := map[string]bool{}
	for range fetchAttempts {
		k, err := c.Compile(id)
		if err != nil {
			return nil, err
		}
		fetched := false
		for _, spec := range k.Missing {
			if !tried[spec] {
				tried[spec] = true
				k.Fetch(spec)
				fetched = true
			}
		}
		if fetched {
			k.Close()
			continue
		}
		defer k.Close()
		for _, diag := range k.Diagnostics {
			if diag.Error {
				return nil, &CompileError{Message: diag.Message}
			}
		}
		if !k.OK {
			return nil, ErrNotCompiled
		}
		return k.PDF()
	}
	return nil, ErrPackagesUnfetched
}
