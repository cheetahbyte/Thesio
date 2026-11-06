package document

import (
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
)

const hoverWidth = 380

// hover asks Typst what the text under the pointer is, off the main thread.
func (d *Open) hover(r editor.HoverRequest) {
	k := d.s.knowledge()
	if k == nil {
		return
	}
	text, path, ticket := d.Text(), d.id, d.hovering.ticket()
	go func() {
		var found typst.Hover
		var ok bool
		if !d.hovering.run(ticket, func() { found, ok = k.Hover(path, text, r.Offset) }) || !ok || found.Text == "" {
			return
		}
		d.h.Update(func() {
			if d.closed || d.buf.Version() != r.Version {
				return
			}
			start, end := wordAround(text, r.Offset)
			if found.Code {
				d.ed.ShowHover(start, end, found.Text, nil)
				return
			}
			d.ed.ShowHover(start, end, "", func(c *ui.Context) {
				ui.Text(c, found.Text).FontSize(12).MaxWidth(hoverWidth)
			})
		})
	}()
}
