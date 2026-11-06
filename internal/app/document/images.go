package document

import (
	"path/filepath"

	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui/editor"
)

// dropped takes in files dropped on the editor: images as figures.
func (d *Open) dropped(files []string) {
	var images []vault.Incoming
	for _, f := range files {
		if !vault.NamedAsImage(f) {
			d.h.ToastError(filepath.Base(f) + " is not an image Typst can show: use PNG, JPEG, GIF, WebP, SVG, or PDF")
			continue
		}
		images = append(images, vault.Incoming{File: f})
	}
	d.Focus()
	d.addImages(images)
}

// pasteImages takes a paste of images, which has no text; false leaves a
// paste of text to the editor.
func (d *Open) pasteImages() bool {
	if mygo.Clipboard.ReadText() != "" {
		return false
	}
	if data := mygo.Clipboard.ReadImage(); len(data) > 0 {
		d.addImages([]vault.Incoming{{Pasted: data}})
		return true
	}
	files, err := mygo.Clipboard.ReadFiles()
	if err != nil {
		return false
	}
	var images []vault.Incoming
	for _, f := range files {
		if vault.NamedAsImage(f) {
			images = append(images, vault.Incoming{File: f})
		}
	}
	if len(images) == 0 {
		return false
	}
	d.addImages(images)
	return true
}

// addImages copies images beside the document and puts their figures at the
// caret. One that cannot be copied says so, and the rest go on.
func (d *Open) addImages(images []vault.Incoming) {
	var paths []string
	for _, in := range images {
		path, err := d.h.Store().AddImage(d.id, in)
		if err != nil {
			d.h.ToastError(err.Error())
			continue
		}
		paths = append(paths, path)
	}
	if len(paths) == 0 {
		return
	}
	d.h.RefreshEntries()
	d.InsertFigures(paths)
}

// InsertFigures replaces the selection, or puts at the next place a figure
// can stand, a figure for each path, with the caret in the first caption and
// Tab reaching the others.
func (d *Open) InsertFigures(paths []string) {
	if len(paths) == 0 {
		return
	}
	text := d.Text()
	sel := d.ed.Selection()
	start, end := sel.Start(), sel.End()
	if start == end {
		start = typst.FigurePlace(text, start)
		end = start
	}
	inserted, captions := vault.Figures(paths, text[:start], text[end:])
	d.buf.Edit(editor.Edit{Start: start, End: end, Text: inserted})
	first := start + captions[0]
	d.selectRange(first, first)
	n := d.buf.Len()
	d.stops = nil
	for _, c := range captions[1:] {
		d.stops = append(d.stops, [2]int{n - (start + c), n - (start + c)})
	}
}
