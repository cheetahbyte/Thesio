package document

import (
	"slices"

	"github.com/cheetahbyte/thesio/internal/typst"
)

// blockRange is the generated bibliography's bytes; atEnd when it ends the
// text, so nothing can be typed below it.
type blockRange struct {
	start, end int
	ok, atEnd  bool
}

// holds reports whether a caret at offset would be inside the block.
func (b blockRange) holds(offset int) bool {
	return b.ok && offset >= b.start && (offset < b.end || b.atEnd)
}

func (d *Open) refreshBlock(text string) {
	start, end, ok := typst.BibliographyBlockRange(text)
	d.block = blockRange{start: start, end: end, ok: ok && end > start, atEnd: end == len(text)}
}

// BlockShown reports whether the generated bibliography shows unfolded.
func (d *Open) BlockShown() bool { return d.blockShown }

// ToggleBlock shows or folds the generated bibliography.
func (d *Open) ToggleBlock() {
	d.blockShown = !d.blockShown
	if d.blockShown && d.block.ok {
		d.ed.Unfold(d.buf.Pos(d.block.start).Line)
	}
	d.keepFolded()
}

// keepFolded folds the generated bibliography unless shown, and keeps the
// caret out of it, moved the way it was heading.
func (d *Open) keepFolded() {
	if !d.block.ok || d.blockShown {
		return
	}
	first := d.buf.Pos(d.block.start).Line
	if !slices.Contains(d.ed.Folded(), first) {
		d.ed.Fold(first)
	}
	sel := d.ed.Selection()
	if !sel.Empty() || !d.block.holds(sel.Head) {
		return
	}
	at := d.block.end
	if d.block.atEnd || sel.Head < d.caret {
		at = max(d.block.start-1, 0)
	}
	d.selectRange(at, at)
}
