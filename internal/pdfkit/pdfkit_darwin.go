//go:build darwin

package pdfkit

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

type rect struct{ X, Y, W, H float64 }
type point struct{ X, Y float64 }
type nsRange struct{ Location, Length uint }

const (
	caseInsensitive  = 1
	regexSearch      = 1024
	nsNotFound       = uint(1<<63 - 1)
	nsUTF8Encoding   = 4
	highlightSubtype = "Highlight"
)

var (
	loadOnce             sync.Once
	loaded               bool
	poolPush, poolPop    uintptr
	subtypeHighlight     objc.ID
	clsDocument          objc.ID
	clsAnnotation        objc.ID
	clsString, clsURL    objc.ID
	clsArray, clsValue   objc.ID
	clsColor             objc.ID
	selAlloc, selRelease objc.SEL
	selInitWithURL       objc.SEL
	selPageCount         objc.SEL
	selPageAtIndex       objc.SEL
	selLabel, selString  objc.SEL
	selAnnotations       objc.SEL
	selCount, selObject  objc.SEL
	selType, selContents objc.SEL
	selSetContents       objc.SEL
	selBounds, selQuads  objc.SEL
	selSelectionForRect  objc.SEL
	selSelectionForRange objc.SEL
	selByLine, selPages  objc.SEL
	selBoundsForPage     objc.SEL
	selFirstObject       objc.SEL
	selAddAnnotation     objc.SEL
	selRemoveAnnotation  objc.SEL
	selWriteToFile       objc.SEL
	selWithUTF8          objc.SEL
	selUTF8, selLength   objc.SEL
	selFileURL           objc.SEL
	selRangeOfString     objc.SEL
	selArray, selAdd     objc.SEL
	selValueWithPoint    objc.SEL
	selPointValue        objc.SEL
	selInitAnnotation    objc.SEL
	selSRGB, selSetColor objc.SEL
	selSetQuads          objc.SEL
)

func load() {
	loadOnce.Do(func() {
		kit, err := purego.Dlopen("/System/Library/Frameworks/PDFKit.framework/PDFKit", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			return
		}
		libobjc, err := purego.Dlopen("/usr/lib/libobjc.A.dylib", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			return
		}
		if poolPush, err = purego.Dlsym(libobjc, "objc_autoreleasePoolPush"); err != nil {
			return
		}
		if poolPop, err = purego.Dlsym(libobjc, "objc_autoreleasePoolPop"); err != nil {
			return
		}
		p, err := purego.Dlsym(kit, "PDFAnnotationSubtypeHighlight")
		if err != nil {
			return
		}
		subtypeHighlight = **(**objc.ID)(unsafe.Pointer(&p))
		cls := func(name string) objc.ID { return objc.ID(objc.GetClass(name)) }
		clsDocument, clsAnnotation, clsString, clsURL = cls("PDFDocument"), cls("PDFAnnotation"), cls("NSString"), cls("NSURL")
		clsArray, clsValue, clsColor = cls("NSMutableArray"), cls("NSValue"), cls("NSColor")
		for _, c := range []objc.ID{clsDocument, clsAnnotation, clsString, clsURL, clsArray, clsValue, clsColor} {
			if c == 0 {
				return
			}
		}
		for s, name := range map[*objc.SEL]string{
			&selAlloc: "alloc", &selRelease: "release", &selInitWithURL: "initWithURL:", &selPageCount: "pageCount",
			&selPageAtIndex: "pageAtIndex:", &selLabel: "label", &selString: "string", &selAnnotations: "annotations",
			&selCount: "count", &selObject: "objectAtIndex:", &selType: "type", &selContents: "contents",
			&selSetContents: "setContents:", &selBounds: "bounds", &selQuads: "quadrilateralPoints",
			&selSelectionForRect: "selectionForRect:", &selSelectionForRange: "selectionForRange:",
			&selByLine: "selectionsByLine", &selPages: "pages", &selBoundsForPage: "boundsForPage:",
			&selFirstObject: "firstObject", &selAddAnnotation: "addAnnotation:", &selRemoveAnnotation: "removeAnnotation:",
			&selWriteToFile: "writeToFile:", &selWithUTF8: "stringWithUTF8String:", &selUTF8: "UTF8String",
			&selLength: "lengthOfBytesUsingEncoding:", &selFileURL: "fileURLWithPath:", &selRangeOfString: "rangeOfString:options:",
			&selArray: "array", &selAdd: "addObject:", &selValueWithPoint: "valueWithPoint:", &selPointValue: "pointValue",
			&selInitAnnotation: "initWithBounds:forType:withProperties:", &selSRGB: "colorWithSRGBRed:green:blue:alpha:",
			&selSetColor: "setColor:", &selSetQuads: "setQuadrilateralPoints:",
		} {
			*s = objc.RegisterName(name)
		}
		loaded = true
	})
}

// Available reports whether PDFKit could be loaded.
func Available() bool {
	load()
	return loaded
}

// withPool runs fn in an autorelease pool. A pool belongs to the thread that
// pushed it, so the goroutine stays on its thread until the pop.
func withPool(fn func()) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool, _, _ := purego.SyscallN(poolPush)
	defer purego.SyscallN(poolPop, pool)
	fn()
}

func nsString(s string) objc.ID {
	b := append([]byte(strings.ReplaceAll(s, "\x00", "")), 0)
	str := objc.ID(clsString).Send(selWithUTF8, unsafe.Pointer(&b[0]))
	runtime.KeepAlive(b)
	return str
}

func goString(str objc.ID) string {
	if str == 0 {
		return ""
	}
	p := objc.Send[*byte](str, selUTF8)
	if p == nil {
		return ""
	}
	return string(unsafe.Slice(p, objc.Send[int](str, selLength, nsUTF8Encoding)))
}

// Document is an open PDFDocument. Its methods may be called from any
// goroutine, one at a time.
type Document struct {
	mu   sync.Mutex
	doc  objc.ID
	path string
}

// Open reads the PDF at path.
func Open(path string) (*Document, error) {
	if !Available() {
		return nil, errors.New("pdfkit: PDFKit is not available")
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	var doc objc.ID
	withPool(func() {
		url := clsURL.Send(selFileURL, nsString(abs))
		doc = clsDocument.Send(selAlloc).Send(selInitWithURL, url)
	})
	if doc == 0 {
		return nil, fmt.Errorf("pdfkit: cannot read %s", path)
	}
	return &Document{doc: doc, path: abs}, nil
}

func (d *Document) Close() {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.doc != 0 {
		withPool(func() { d.doc.Send(selRelease) })
		d.doc = 0
	}
}

// run holds the lock and a pool around fn; fn is skipped once closed.
func (d *Document) run(fn func()) {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.doc != 0 {
		withPool(fn)
	}
}

func (d *Document) count() int { return objc.Send[int](d.doc, selPageCount) }

func (d *Document) page(i int) objc.ID {
	if i < 0 || i >= d.count() {
		return 0
	}
	return d.doc.Send(selPageAtIndex, uint(i))
}

func (d *Document) PageCount() int {
	n := 0
	d.run(func() { n = d.count() })
	return n
}

// PageLabel is the page's own label, "" when it has none.
func (d *Document) PageLabel(i int) string {
	s := ""
	d.run(func() {
		if p := d.page(i); p != 0 {
			s = goString(p.Send(selLabel))
		}
	})
	return s
}

func (d *Document) PageText(i int) string {
	s := ""
	d.run(func() {
		if p := d.page(i); p != 0 {
			s = goString(p.Send(selString))
		}
	})
	return s
}

// Highlights lists the highlight annotations in page order.
func (d *Document) Highlights() []Highlight {
	var out []Highlight
	d.run(func() {
		for i, n := 0, d.count(); i < n; i++ {
			page := d.page(i)
			list := page.Send(selAnnotations)
			for j, m := 0, objc.Send[int](list, selCount); j < m; j++ {
				a := list.Send(selObject, uint(j))
				if goString(a.Send(selType)) != highlightSubtype {
					continue
				}
				out = append(out, Highlight{Page: i, Index: j, Text: quote(page, a), Comment: goString(a.Send(selContents))})
			}
		}
	})
	return out
}

// quote is the page text under the annotation's quads, or its bounds without any.
func quote(page, a objc.ID) string {
	bounds := objc.Send[rect](a, selBounds)
	rects := []rect{bounds}
	if pts := a.Send(selQuads); pts != 0 {
		if n := objc.Send[int](pts, selCount); n >= 4 {
			rects = rects[:0]
			corner := func(k int) point { return objc.Send[point](pts.Send(selObject, uint(k)), selPointValue) }
			// Quad points are relative to the bounds' origin.
			for q := 0; q+3 < n; q += 4 {
				a, b := corner(q), corner(q+3)
				rects = append(rects, rect{bounds.X + min(a.X, b.X), bounds.Y + min(a.Y, b.Y), abs(b.X - a.X), abs(b.Y - a.Y)})
			}
		}
	}
	parts := make([]string, 0, len(rects))
	for _, r := range rects {
		if sel := page.Send(selSelectionForRect, r); sel != 0 {
			parts = append(parts, strings.Join(strings.Fields(goString(sel.Send(selString))), " "))
		}
	}
	return strings.Join(parts, " ")
}

func abs(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
}

// pattern matches text case-insensitively with any whitespace between words,
// so a quote spanning a line break is found.
func pattern(text string) string {
	words := strings.Fields(text)
	for i, w := range words {
		words[i] = `\Q` + strings.ReplaceAll(w, `\E`, `\E\\E\Q`) + `\E`
	}
	return strings.Join(words, `\s+`)
}

// AddHighlight highlights the first occurrence of text on page, with comment,
// and saves the document.
func (d *Document) AddHighlight(page int, text, comment string) error {
	err := errors.New("pdfkit: document is closed")
	d.run(func() {
		p := d.page(page)
		if p == 0 {
			err = fmt.Errorf("pdfkit: no page %d", page)
			return
		}
		if strings.TrimSpace(text) == "" {
			err = errors.New("pdfkit: nothing to highlight")
			return
		}
		r := objc.Send[nsRange](p.Send(selString), selRangeOfString, nsString(pattern(text)), uint(caseInsensitive|regexSearch))
		if r.Location == nsNotFound || r.Length == 0 {
			err = fmt.Errorf("pdfkit: %q not found on page %d", text, page+1)
			return
		}
		sel := p.Send(selSelectionForRange, r)
		if sel == 0 {
			err = fmt.Errorf("pdfkit: %q not found on page %d", text, page+1)
			return
		}
		var rects []rect
		lines := sel.Send(selByLine)
		for i, n := 0, objc.Send[int](lines, selCount); i < n; i++ {
			if r := objc.Send[rect](lines.Send(selObject, uint(i)), selBoundsForPage, p); r.W > 0 && r.H > 0 {
				rects = append(rects, r)
			}
		}
		if len(rects) == 0 {
			err = fmt.Errorf("pdfkit: %q has no area on page %d", text, page+1)
			return
		}
		addHighlight(p, rects, comment)
		err = d.save()
	})
	return err
}

// addHighlight adds one annotation with a quad per line, as readers expect of
// a multi-line highlight.
func addHighlight(page objc.ID, rects []rect, comment string) {
	l, b, r, t := rects[0].X, rects[0].Y, rects[0].X+rects[0].W, rects[0].Y+rects[0].H
	for _, x := range rects[1:] {
		l, b, r, t = min(l, x.X), min(b, x.Y), max(r, x.X+x.W), max(t, x.Y+x.H)
	}
	points := clsArray.Send(selArray)
	for _, x := range rects {
		x0, y0 := x.X-l, x.Y-b
		for _, pt := range []point{{x0, y0 + x.H}, {x0 + x.W, y0 + x.H}, {x0, y0}, {x0 + x.W, y0}} {
			points.Send(selAdd, clsValue.Send(selValueWithPoint, pt))
		}
	}
	a := clsAnnotation.Send(selAlloc).Send(selInitAnnotation, rect{l, b, r - l, t - b}, subtypeHighlight, objc.ID(0))
	a.Send(selSetColor, clsColor.Send(selSRGB, 1.0, 0.86, 0.2, 0.55))
	a.Send(selSetQuads, points)
	a.Send(selSetContents, nsString(comment))
	page.Send(selAddAnnotation, a)
	a.Send(selRelease)
}

// annotation finds h again; 0 when the page changed underneath it.
func (d *Document) annotation(h Highlight) (page, a objc.ID) {
	if page = d.page(h.Page); page == 0 {
		return 0, 0
	}
	list := page.Send(selAnnotations)
	if h.Index < 0 || h.Index >= objc.Send[int](list, selCount) {
		return 0, 0
	}
	a = list.Send(selObject, uint(h.Index))
	if goString(a.Send(selType)) != highlightSubtype {
		return 0, 0
	}
	return page, a
}

func (d *Document) SetComment(h Highlight, comment string) error {
	err := errors.New("pdfkit: document is closed")
	d.run(func() {
		_, a := d.annotation(h)
		if a == 0 {
			err = errors.New("pdfkit: highlight not found")
			return
		}
		a.Send(selSetContents, nsString(comment))
		err = d.save()
	})
	return err
}

func (d *Document) RemoveHighlight(h Highlight) error {
	err := errors.New("pdfkit: document is closed")
	d.run(func() {
		page, a := d.annotation(h)
		if a == 0 {
			err = errors.New("pdfkit: highlight not found")
			return
		}
		page.Send(selRemoveAnnotation, a)
		err = d.save()
	})
	return err
}

// save writes beside the file and renames it over, so a failed write leaves
// the PDF whole.
func (d *Document) save() error {
	part := filepath.Join(filepath.Dir(d.path), "."+filepath.Base(d.path)+".part")
	if !objc.Send[bool](d.doc, selWriteToFile, nsString(part)) {
		os.Remove(part)
		return fmt.Errorf("pdfkit: cannot write %s", d.path)
	}
	return os.Rename(part, d.path)
}
