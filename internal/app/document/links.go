package document

import "github.com/cheetahbyte/thesio/internal/typst"

// followAt follows the note link at offset, for ⌘-click and F12.
func (d *Open) followAt(offset int) {
	if d.OnFollowLink == nil {
		return
	}
	for _, link := range typst.NoteLinks(d.Text(), d.id) {
		if offset >= link.Start && offset < link.End {
			d.OnFollowLink(link.Target)
			return
		}
	}
}
