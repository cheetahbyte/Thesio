package citations

import (
	"fmt"
	"slices"

	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

func citedKeys(text string) []string { return typst.CitedKeys(text) }

func (s *State) read(id string) (string, bool) {
	text, err := s.h.TextOf(id)
	return text, err == nil
}

// readDocument is id's text, or nothing after saying why.
func (s *State) readDocument(id string) (string, bool) {
	text, err := s.h.TextOf(id)
	if err != nil {
		s.h.ToastError(fmt.Sprintf("Could not read %s: %v", id, err))
		return "", false
	}
	return text, true
}

// CitedIn is every key cited in a document and the files it includes, sorted.
func (s *State) CitedIn(id string) []string {
	var keys []string
	for _, f := range project.Open(id, s.read).Files() {
		keys = append(keys, citedKeys(f.Text)...)
	}
	slices.Sort(keys)
	return slices.Compact(keys)
}

// SyncBibliographies rewrites each generated block to hold what its document cites.
func (s *State) SyncBibliographies() {
	lib := s.library()
	if lib == nil {
		return
	}
	type update struct{ id, text string }
	var updates []update
	var failed []string
	for _, e := range s.h.Entries() {
		if !e.Typst {
			continue
		}
		if _, _, ok := typst.BibliographyBlockRange(e.Body); !ok {
			continue
		}
		keys := slices.DeleteFunc(s.CitedIn(e.ID), func(k string) bool { return !lib.Contains(k) })
		yaml, err := lib.Subset(keys)
		var text string
		if err == nil {
			text, err = s.h.TextOf(e.ID)
		}
		if err != nil {
			failed = append(failed, fmt.Sprintf("Could not update the bibliography in %s: %v", e.ID, err))
			continue
		}
		if synced, ok := typst.SyncBibliographyBlock(text, yaml); ok {
			updates = append(updates, update{e.ID, synced})
		}
	}
	s.h.Warn(failed)
	for _, u := range updates {
		if err := s.h.WriteDocument(u.id, u.text, true); err != nil {
			s.h.ToastError(fmt.Sprintf("Could not update the bibliography in %s: %v", u.id, err))
		}
	}
}

// BibliographyMode is how document id gets its bibliography.
func (s *State) BibliographyMode(id string) typst.BibMode {
	text, ok := s.read(id)
	if id == "" || !ok {
		return typst.BibNone
	}
	return typst.BibliographyMode(text, id)
}

// SetBibliography switches the open document's bibliography as one undoable edit.
func (s *State) SetBibliography(mode typst.BibMode) {
	id := s.h.Current()
	if id == "" {
		return
	}
	text, ok := s.readDocument(id)
	if !ok {
		return
	}
	text = typst.RemoveBibliography(text, id)
	switch mode {
	case typst.BibEmbedded:
		text = typst.EmbedBibliography(text, "")
	case typst.BibLinked:
		text = typst.LinkBibliography(text, id)
	}
	if err := s.h.WriteDocument(id, text, false); err != nil {
		s.h.ToastError(fmt.Sprintf("Could not save %s: %v", id, err))
	}
}

// StyleOwner is the document holding the open file's bibliography, and its
// style. A chapter's bibliography is usually in the document that includes it.
func (s *State) StyleOwner() (doc, style string, ok bool) {
	id := s.h.Current()
	if id == "" {
		return "", "", false
	}
	for _, doc := range []string{id, s.h.TopDocument(id)} {
		text, ok := s.read(doc)
		if !ok || typst.BibliographyMode(text, doc) == typst.BibNone {
			continue
		}
		style, ok := typst.BibliographyStyle(text)
		if !ok {
			style = library.DefaultStyle
		}
		return doc, style, true
	}
	return "", "", false
}

// SetStyle gives the open file's bibliography a citation style.
func (s *State) SetStyle(style string) {
	doc, _, ok := s.StyleOwner()
	if !ok {
		return
	}
	text, ok := s.readDocument(doc)
	if !ok {
		return
	}
	updated, ok := typst.SetBibliographyStyle(text, style)
	if !ok {
		return
	}
	if err := s.h.WriteDocument(doc, updated, false); err != nil {
		s.h.ToastError(fmt.Sprintf("Could not change the style in %s: %v", doc, err))
	}
}

// EnsureBibliography embeds a bibliography in id's top-level document unless
// the document or any file above it already has one.
func (s *State) EnsureBibliography(id string) {
	text, ok := s.readDocument(id)
	if !ok || typst.BibliographyMode(text, id) != typst.BibNone {
		return
	}
	documents := s.h.Documents()
	for _, doc := range documents.Includers(id) {
		if text, ok := s.read(doc); ok && typst.BibliographyMode(text, doc) != typst.BibNone {
			return
		}
	}
	top := documents.TopDocument(id)
	if text, ok = s.readDocument(top); !ok {
		return
	}
	if err := s.h.WriteDocument(top, typst.EmbedBibliography(text, ""), true); err != nil {
		s.h.ToastError(fmt.Sprintf("Could not add a bibliography to %s: %v", top, err))
	}
}

// RenameCitations follows a key change through every document that cites it.
func (s *State) RenameCitations(old, new string) {
	documents := s.h.Documents()
	changed := 0
	for _, e := range s.h.Entries() {
		if !e.Typst {
			continue
		}
		text, ok := s.readDocument(e.ID)
		if !ok {
			continue
		}
		updated, ok := typst.RenameCitation(text, old, new, documents.HasLabel(e.ID, old))
		if !ok {
			continue
		}
		if err := s.h.WriteDocument(e.ID, updated, false); err != nil {
			s.h.ToastError(fmt.Sprintf("Could not update the citation in %s: %v", e.ID, err))
			continue
		}
		changed++
	}
	switch {
	case changed == 1:
		s.h.Toast("Updated the citation in 1 document")
	case changed > 1:
		s.h.Toast(fmt.Sprintf("Updated citations in %d documents", changed))
	}
}

// DocMenuItems adds the open document's citation items to its menu, each
// group after a separator: the style, then citing and the bibliography.
func (s *State) DocMenuItems(m *ui.Menu) {
	id := s.h.Current()
	if !vault.IsDocument(id) {
		return
	}
	mode := s.BibliographyMode(id)
	owner, style, owned := s.StyleOwner()
	// A document has one bibliography, and a chapter's may be in its main file.
	elsewhere := owned && owner != id
	if owned {
		label := style
		for _, cs := range library.CitationStyles {
			if cs[1] == style {
				label = cs[0]
			}
		}
		m.Separator()
		m.Submenu("Citation Style: "+label, func(m *ui.Menu) {
			for _, choice := range library.CitationStyles {
				if m.Item(choice[0]).Radio(choice[1] == style).Chosen() {
					s.SetStyle(choice[1])
				}
			}
		})
	}
	if mode == typst.BibCustom {
		return
	}
	m.Separator()
	if m.Item("Insert citation").Chosen() {
		s.OpenPicker()
	}
	if mode == typst.BibEmbedded && s.ToggleBlock != nil {
		label := "Show bibliography data"
		if s.BlockShown != nil && s.BlockShown() {
			label = "Hide bibliography data"
		}
		if m.Item(label).Chosen() {
			s.ToggleBlock()
		}
	}
	if mode != typst.BibEmbedded && !elsewhere && m.Item("Embed bibliography").Chosen() {
		s.SetBibliography(typst.BibEmbedded)
	}
	if mode != typst.BibLinked && !elsewhere && m.Item("Link library file").Chosen() {
		s.SetBibliography(typst.BibLinked)
	}
	if mode != typst.BibNone && m.Item("Remove bibliography").Chosen() {
		s.SetBibliography(typst.BibNone)
	}
}
