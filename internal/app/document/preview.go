package document

import (
	"fmt"
	"strconv"
	"time"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/egoist/mygo/ui"
)

const (
	followDelay = 120 * time.Millisecond
	// followAt is how far down the view a followed spot lands, with text above it to read.
	followAt = 1.0 / 3
	// followMargin is how close to an edge counts as out of view.
	followMargin = 48
	pageGap      = 16
	pagePad      = 24
	problemsMax  = 160
)

// preview is the pages' state: where the pane scrolls, and the caret's
// place it keeps in view.
type preview struct {
	scroll       ui.ScrollState
	width, viewH float32
	opened       bool
	reveal       int
	followGen    uint64
	followOffset int
	following    bool
	target       *followTarget
}

type followTarget struct {
	page int
	y    float32
}

// placed is where a page sits in the scrolling column, and at what zoom.
type placed struct{ top, width, height, zoom float32 }

// layout lays pages out in a pane width wide, or at full size before it is measured.
func layout(pages []page, width float32) []placed {
	out := make([]placed, len(pages))
	top := float32(pagePad)
	for i, p := range pages {
		zoom := float32(1)
		if width > 0 {
			zoom = max((width-2*pagePad)/p.width, 0.2)
		}
		out[i] = placed{top: top, width: p.width * zoom, height: p.height * zoom, zoom: zoom}
		top += p.height*zoom + pageGap
	}
	return out
}

// follow keeps the caret's place in view, shortly after it stops moving.
func (d *Open) follow(offset int) {
	if !d.withPreview {
		return
	}
	d.pv.followOffset, d.pv.following = offset, true
	d.locate(followDelay)
}

func (d *Open) locate(delay time.Duration) {
	k := d.s.document
	if k == nil || !d.pv.following {
		return
	}
	d.pv.followGen++
	gen, path, offset := d.pv.followGen, d.id, d.pv.followOffset
	go func() {
		time.Sleep(delay)
		page, y, ok := k.PositionOf(path, offset)
		if !ok {
			return
		}
		d.h.Update(func() {
			if gen == d.pv.followGen && !d.closed {
				d.pv.target = &followTarget{page: page, y: y}
			}
		})
	}()
}

// compiledPages follows new pages: the first show the open file's start, and
// the caret's place may be on a page that just arrived.
func (d *Open) compiledPages() {
	if !d.pv.opened && len(d.s.pages) > 0 {
		d.pv.opened = true
		if !d.pv.following {
			d.pv.reveal = fileStart(d.s.pages)
		}
	}
	d.locate(0)
}

// fileStart is the first page of the longest run of the open file's pages.
// A contents page lists every chapter, so the first match alone would often land there.
func fileStart(pages []page) int {
	best, bestLength, start := 0, 0, 0
	for i, p := range pages {
		if !p.own {
			start = i + 1
		} else if i+1-start > bestLength {
			best, bestLength = start, i+1-start
		}
	}
	return best
}

// scrollToTarget scrolls to the followed place unless it is in view.
func (d *Open) scrollToTarget(c *ui.Context, placed []placed) {
	pv := &d.pv
	if pv.width <= 0 || pv.viewH <= 0 {
		if pv.target != nil || pv.reveal > 0 {
			c.Invalidate()
		}
		return
	}
	if pv.reveal > 0 && pv.reveal < len(placed) {
		pv.scroll.Y = placed[pv.reveal].top - pagePad
	}
	pv.reveal = 0
	t := pv.target
	pv.target = nil
	if t == nil || t.page >= len(placed) {
		return
	}
	at := placed[t.page].top + t.y*placed[t.page].zoom
	if at < pv.scroll.Y+followMargin || at > pv.scroll.Y+pv.viewH-followMargin {
		pv.scroll.Y = max(at-pv.viewH*followAt, 0)
	}
}

// pageClicked goes to the source of what was clicked, at x and y in points.
func (d *Open) pageClicked(i int, x, y float32) {
	k := d.s.document
	if k == nil {
		return
	}
	go func() {
		target, ok := k.TargetAt(i, x, y)
		if !ok {
			return
		}
		d.h.Update(func() {
			switch {
			case d.closed:
			case target.Note != "":
				d.h.FollowLink(target.Note)
			case target.Path == d.id:
				d.GoTo(k.Moved(d.id, target.Offset, d.Text()))
			default:
				d.h.GoToSource(target.Path, target.Offset)
			}
		})
	}()
}

// problemClicked goes to where a diagnostic points.
func (d *Open) problemClicked(p typst.Diagnostic) {
	switch {
	case p.Path == "" || p.Start < 0:
	case p.Path == d.id && d.s.newest != nil:
		d.GoTo(d.s.newest.Moved(d.id, p.Start, d.Text()))
	default:
		d.h.GoToSource(p.Path, p.Start)
	}
}

// status is the corner's text: the open file's words of the whole, and the compile time.
func (d *Open) statusText() string {
	if !d.s.compiled {
		return ""
	}
	total, own, found := 0, 0, false
	for _, w := range d.Words() {
		total += w.Words
		if w.Path == d.id {
			own, found = w.Words, true
		}
	}
	var words string
	switch {
	case found && own != total:
		words = formatCount(own) + " of " + formatCount(total) + " words"
	case total == 1:
		words = "1 word"
	default:
		words = formatCount(total) + " words"
	}
	return words + " · " + formatTook(d.s.took)
}

func formatCount(n int) string {
	digits := strconv.Itoa(n)
	out := make([]byte, 0, len(digits)+len(digits)/3)
	for i := range len(digits) {
		if i > 0 && (len(digits)-i)%3 == 0 {
			out = append(out, ',')
		}
		out = append(out, digits[i])
	}
	return string(out)
}

func formatTook(took time.Duration) string {
	if ms := took.Milliseconds(); ms < 1000 {
		return fmt.Sprintf("%d ms", ms)
	}
	return fmt.Sprintf("%.1f s", took.Seconds())
}

// previewView is the pages scaled to the pane's width, the status in the
// corner, and the diagnostics under them.
func (d *Open) previewView(c *ui.Context) {
	t := c.Theme()
	pages := d.s.pages
	ui.Column(c).Fill().Background(theme.Group(t)).Children(func() {
		ui.Box(c).Grow(1).MinHeight(0).FillWidth().Children(func() {
			placed := layout(pages, d.pv.width)
			d.scrollToTarget(c, placed)
			scroll := ui.Scroll(c).TrackScroll(&d.pv.scroll).Fill().Padding(pagePad).Gap(pageGap).AlignItems(ui.Center)
			if b := scroll.Bounds(); b.W != d.pv.width || b.H != d.pv.viewH {
				d.pv.width, d.pv.viewH = b.W, b.H
				c.Invalidate()
			}
			scroll.Children(func() {
				for i, p := range pages {
					img := ui.Image(c.Key(i), p.bitmap).Size(placed[i].width, placed[i].height).
						Shadow(0, 0, 0, 1, theme.Ring(t)).Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, 0.06)).Shadow(0, 4, 14, 0, ui.RGBA(0, 0, 0, 0.08))
					if img.Clicked() {
						if x, y, ok := img.PointerPosition(); ok {
							d.pageClicked(i, x/placed[i].zoom, y/placed[i].zoom)
						}
					}
				}
			})
			if status := d.statusText(); status != "" {
				theme.Tabular(ui.Text(c, status).FontSize(11).TextColor(theme.Caption(t)).Absolute().Right(12).Bottom(8))
			}
		})
		if len(d.s.diagnostics) == 0 {
			return
		}
		ui.Scroll(c).MaxHeight(problemsMax).PaddingY(6).Background(t.Background).BorderWidth(1, 0, 0, 0).BorderColor(theme.Hairline(t)).Children(func() {
			for i, p := range d.s.diagnostics {
				row := ui.Text(c.Key(i), problemText(p)).FontSize(12).Padding(4, 12)
				if p.Error {
					row.TextColor(theme.Danger(t))
				} else {
					row.TextColor(theme.Caption(t))
				}
				if row.Clicked() {
					d.problemClicked(p)
				}
			}
		})
	})
}

// problemText is "path:line  message".
func problemText(p typst.Diagnostic) string {
	switch {
	case p.Path == "":
		return p.Message
	case p.Line >= 0:
		return fmt.Sprintf("%s:%d  %s", p.Path, p.Line+1, p.Message)
	default:
		return p.Path + "  " + p.Message
	}
}
