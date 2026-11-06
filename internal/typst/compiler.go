package typst

/*
#include <stdlib.h>
#include "thesio_typst.h"
*/
import "C"

import (
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"runtime"
	"sync"
	"unsafe"
)

// Compiler compiles documents of one vault. Unsaved text set with SetBuffer
// shadows the file on disk. Compiling never reaches the network: packages it
// lacks are listed in Compilation.Missing, to Fetch and compile again.
type Compiler struct {
	mu      sync.Mutex
	ptr     *C.ThesioCompiler
	cleanup runtime.Cleanup
}

// ErrCompilerClosed is a compile on a closed compiler, or one whose root is not UTF-8.
var ErrCompilerClosed = errors.New("typst: compiler is closed or its root is not UTF-8")

// ErrNoPage is a page the compilation does not have.
var ErrNoPage = errors.New("typst: no page")

// NewCompiler is a compiler for the vault at root.
func NewCompiler(root string) *Compiler {
	var a arena
	defer a.free()
	c := &Compiler{ptr: C.thesio_compiler_new(a.str(root))}
	if c.ptr != nil {
		c.cleanup = runtime.AddCleanup(c, func(p *C.ThesioCompiler) { C.thesio_compiler_free(p) }, c.ptr)
	}
	return c
}

// Close frees the compiler; it is idempotent.
func (c *Compiler) Close() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.ptr == nil {
		return
	}
	c.cleanup.Stop()
	C.thesio_compiler_free(c.ptr)
	c.ptr = nil
}

// SetBuffer makes unsaved text shadow the file at vault path. It fails for an
// invalid path, and does nothing once closed.
func (c *Compiler) SetBuffer(path, text string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.ptr == nil {
		return nil
	}
	var a arena
	defer a.free()
	_, err := result(C.thesio_compiler_set_buffer(c.ptr, a.str(path), a.str(text)))
	return err
}

// ClearBuffer drops the unsaved text of path, so the file on disk counts again.
func (c *Compiler) ClearBuffer(path string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.ptr == nil {
		return
	}
	var a arena
	defer a.free()
	C.thesio_compiler_clear_buffer(c.ptr, a.str(path))
}

// Compile compiles the document at vault path main. It fails only for an
// invalid path; a document with errors is a Compilation with OK false.
func (c *Compiler) Compile(main string) (*Compilation, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.ptr == nil {
		return nil, ErrCompilerClosed
	}
	var a arena
	defer a.free()
	cerr := new(*C.char)
	ptr := C.thesio_compiler_compile(c.ptr, a.str(main), cerr)
	if ptr == nil {
		message, _ := take(*cerr)
		return nil, &Error{Message: message}
	}
	k := &Compilation{ptr: ptr}
	k.cleanup = runtime.AddCleanup(k, func(p *C.ThesioCompilation) { C.thesio_compilation_free(p) }, ptr)
	raw, err := result(C.thesio_compilation_info(ptr))
	if err == nil {
		err = json.Unmarshal(raw, k)
	}
	if err != nil {
		k.Close()
		return nil, fmt.Errorf("typst: reading the compilation: %w", err)
	}
	return k, nil
}

// Diagnostic is an error or warning of a compile.
type Diagnostic struct {
	Error   bool     `json:"error"` // false for warnings and "Downloading @preview/x…" statuses
	Message string   `json:"message"`
	Hints   []string `json:"hints"`
	Path    string   `json:"path"`  // vault path, "" when none
	Start   int      `json:"start"` // byte range in Path; Start == End == -1 when none
	End     int      `json:"end"`
	Line    int      `json:"line"` // zero-based line of Start, -1 when none
}

// Label is what `@` can reference; Page is one-based, 0 when unknown.
type Label struct {
	Key   string `json:"key"`
	Title string `json:"title"`
	Kind  string `json:"kind"`
	Page  int    `json:"page"`
}

// FileWords is how many words a compile drew from a file.
type FileWords struct {
	Path  string `json:"path"`
	Words int    `json:"words"`
}

// PageInfo is a page's size in points and a hash of what it draws.
type PageInfo struct {
	Width  float32 `json:"width"`
	Height float32 `json:"height"`
	Hash   string  `json:"hash"`
}

// Compilation is the outcome of one compile. It holds the files it read, so
// it answers questions on its own and stays valid after later compiles.
type Compilation struct {
	Main        string       `json:"main"`
	OK          bool         `json:"ok"` // a document was produced
	Diagnostics []Diagnostic `json:"diagnostics"`
	Words       []FileWords  `json:"words"`
	Labels      []Label      `json:"labels"`  // what `@` can reference, document order
	Missing     []string     `json:"missing"` // package specs to Fetch, then compile again
	Pages       []PageInfo   `json:"pages"`

	mu      sync.RWMutex
	ptr     *C.ThesioCompilation
	cleanup runtime.Cleanup
}

// Close frees the compilation; it is idempotent, and a forgotten one is freed by the collector.
func (k *Compilation) Close() {
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.ptr == nil {
		return
	}
	k.cleanup.Stop()
	C.thesio_compilation_free(k.ptr)
	k.ptr = nil
}

// use runs f with the handle held open, or reports false once closed.
func (k *Compilation) use(f func(p *C.ThesioCompilation)) bool {
	k.mu.RLock()
	defer k.mu.RUnlock()
	if k.ptr == nil {
		return false
	}
	f(k.ptr)
	return true
}

var errClosed = errors.New("typst: compilation is closed")

// Fetch downloads one of Missing, blocking. A failure shows in the next compile.
func (k *Compilation) Fetch(spec string) {
	var a arena
	defer a.free()
	k.use(func(p *C.ThesioCompilation) { C.thesio_compilation_fetch(p, a.str(spec)) })
}

// Render rasterizes a zero-based page; scale is pixels per point.
func (k *Compilation) Render(page int, scale float32) (*image.RGBA, error) {
	if page < 0 {
		return nil, fmt.Errorf("%w %d", ErrNoPage, page)
	}
	var out *image.RGBA
	var err error
	ok := k.use(func(p *C.ThesioCompilation) {
		var width, height C.uint32_t
		var n C.size_t
		cerr := new(*C.char)
		data := C.thesio_compilation_render(p, C.size_t(page), C.float(scale), &width, &height, &n, cerr)
		if data == nil {
			message, _ := take(*cerr)
			err = &Error{Message: message}
			return
		}
		defer C.thesio_bytes_free(data, n)
		// Typst's pixmap is premultiplied RGBA, as image.RGBA is.
		out = image.NewRGBA(image.Rect(0, 0, int(width), int(height)))
		copy(out.Pix, unsafe.Slice((*byte)(unsafe.Pointer(data)), int(n)))
	})
	if !ok {
		return nil, errClosed
	}
	return out, err
}

// PDF exports the document as PDF.
func (k *Compilation) PDF() ([]byte, error) {
	var out []byte
	var err error
	ok := k.use(func(p *C.ThesioCompilation) {
		var n C.size_t
		cerr := new(*C.char)
		data := C.thesio_compilation_pdf(p, &n, cerr)
		if data == nil {
			message, _ := take(*cerr)
			err = &Error{Message: message}
			return
		}
		defer C.thesio_bytes_free(data, n)
		out = C.GoBytes(unsafe.Pointer(data), C.int(n))
	})
	if !ok {
		return nil, errClosed
	}
	return out, err
}

// Suggestion is a completion of the text before the caret.
type Suggestion struct {
	Title  string   `json:"title"`
	Kind   string   `json:"kind"`
	Detail string   `json:"detail"`
	From   int      `json:"from"`   // replaces text[From:caret]
	Tail   int      `json:"tail"`   // and Tail bytes after the caret
	Insert string   `json:"insert"` // text to insert
	Fields [][2]int `json:"fields"` // placeholder ranges in Insert, in fill order; empty = caret at end
}

// Hover is what to show for the code under the pointer; Code says it is code.
type Hover struct {
	Text string `json:"text"`
	Code bool   `json:"code"`
}

// Target is what a click on a page lands on: Note != "" for a note link, else Path and Offset.
type Target struct {
	Path   string `json:"path"`
	Offset int    `json:"offset"`
	Note   string `json:"note"`
}

// Complete lists what can replace the text before caret in text, the unsaved
// version of path, best match first. explicit says the user asked.
func (k *Compilation) Complete(path, text string, caret int, explicit bool) []Suggestion {
	var a arena
	defer a.free()
	var found []Suggestion
	k.use(func(p *C.ThesioCompilation) {
		found = decode[[]Suggestion](C.thesio_compilation_complete(p, a.str(path), a.str(text), size(caret), C.bool(explicit)))
	})
	return found
}

// Hover is what to show for byte offset of text, the unsaved version of path.
func (k *Compilation) Hover(path, text string, offset int) (Hover, bool) {
	var a arena
	defer a.free()
	var hover *Hover
	k.use(func(p *C.ThesioCompilation) {
		hover = decode[*Hover](C.thesio_compilation_hover(p, a.str(path), a.str(text), size(offset)))
	})
	if hover == nil {
		return Hover{}, false
	}
	return *hover, true
}

// PositionOf says where byte offset of path appears: a zero-based page and y in points.
func (k *Compilation) PositionOf(path string, offset int) (page int, y float32, ok bool) {
	var a arena
	defer a.free()
	k.use(func(p *C.ThesioCompilation) {
		var at C.size_t
		var height C.float
		if C.thesio_compilation_position_of(p, a.str(path), size(offset), &at, &height) {
			page, y, ok = int(at), float32(height), true
		}
	})
	return page, y, ok
}

// TargetAt says what is at a point, in points, on a zero-based page.
func (k *Compilation) TargetAt(page int, x, y float32) (Target, bool) {
	if page < 0 {
		return Target{}, false
	}
	var target *Target
	k.use(func(p *C.ThesioCompilation) {
		target = decode[*Target](C.thesio_compilation_target_at(p, C.size_t(page), C.float(x), C.float(y)))
	})
	if target == nil {
		return Target{}, false
	}
	return *target, true
}

// PagesFrom says for each page whether it shows content from path or what path includes.
func (k *Compilation) PagesFrom(path string) []bool {
	var a arena
	defer a.free()
	var pages []bool
	k.use(func(p *C.ThesioCompilation) {
		pages = decode[[]bool](C.thesio_compilation_pages_from(p, a.str(path)))
	})
	return pages
}

// Placed is this compile's diagnostics in path, moved onto the newer text.
func (k *Compilation) Placed(path, text string) []Diagnostic {
	var a arena
	defer a.free()
	var placed []Diagnostic
	k.use(func(p *C.ThesioCompilation) {
		placed = decode[[]Diagnostic](C.thesio_compilation_placed(p, a.str(path), a.str(text)))
	})
	return placed
}

// Moved carries byte offset of the text this compile read of path onto text.
func (k *Compilation) Moved(path string, offset int, text string) int {
	var a arena
	defer a.free()
	moved := offset
	k.use(func(p *C.ThesioCompilation) {
		moved = int(C.thesio_compilation_moved(p, a.str(path), size(offset), a.str(text)))
	})
	return moved
}
