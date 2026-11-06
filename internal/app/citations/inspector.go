package citations

import (
	"errors"
	"fmt"
	"strconv"
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/egoist/mygo/ui"
)

const inspectorW = 320

// inspector is the panel beside the table that edits the selected entry.
type inspector struct {
	key     string
	keyText string
	fields  []library.Field
	values  []string
	// err is why the library refused what the fields hold.
	err     string
	citedIn []citedIn
	// focused is the field that had the focus last frame, len(fields) for the key, -1 for none.
	focused int
}

// citedIn is a document citing the entry: times cited, and where it first is.
type citedIn struct {
	id           string
	count, first int
}

// OpenEntry shows key in the inspector and puts the caret in its first field.
func (s *State) OpenEntry(key string) {
	s.showLibrary()
	if s.h.Prefs().LibraryDetailsHidden {
		s.setDetailsHidden(false)
	}
	s.selectOnly(key)
	s.TableChanged()
	s.first.Focus()
}

// OpenNewEntry adds a blank Journal Article and opens it to fill in.
func (s *State) OpenNewEntry() {
	lib := s.library()
	if lib == nil {
		return
	}
	key, err := lib.AddBlank(library.JournalArticle)
	if err != nil {
		s.h.ToastError(fmt.Sprintf("Could not add an entry: %v", err))
		return
	}
	s.tagFilter, s.search = "", ""
	s.refresh()
	s.OpenEntry(key)
}

// CloseInspector closes the panel without saving.
func (s *State) CloseInspector() { s.insp = nil }

// syncInspector matches the inspector to the selection: one visible entry, or none.
func (s *State) syncInspector() {
	wanted := ""
	if !s.h.Prefs().LibraryDetailsHidden {
		if keys := s.selectedIn(s.rows()); len(keys) == 1 {
			wanted = keys[0]
		}
	}
	current := ""
	if s.insp != nil {
		current = s.insp.key
	}
	if current == wanted {
		return
	}
	s.SaveInspector()
	s.insp = s.buildInspector(wanted)
}

func (s *State) buildInspector(key string) *inspector {
	lib := s.library()
	if key == "" || lib == nil {
		return nil
	}
	kind, ok := lib.SourceType(key)
	if !ok {
		return nil
	}
	in := &inspector{key: key, keyText: key, fields: kind.Fields, focused: -1}
	for _, f := range kind.Fields {
		in.values = append(in.values, lib.FieldText(key, f))
	}
	for _, e := range s.h.Entries() {
		if !e.Typst {
			continue
		}
		if places := typst.CitationsOf(e.Body, key); len(places) > 0 {
			in.citedIn = append(in.citedIn, citedIn{id: e.ID, count: len(places), first: places[0]})
		}
	}
	return in
}

// revertInspector goes back to what the file holds.
func (s *State) revertInspector() {
	if s.insp == nil {
		return
	}
	s.insp = s.buildInspector(s.insp.key)
	s.list.Handle.Focus()
}

// SaveInspector writes what the inspector holds. The fields stay as typed
// when the library refuses them, with the reason beside them.
func (s *State) SaveInspector() {
	in, lib := s.insp, s.library()
	if in == nil || lib == nil {
		return
	}
	key, newKey := in.key, strings.TrimSpace(in.keyText)
	fields := make([]library.FieldValue, len(in.fields))
	for i, f := range in.fields {
		fields[i] = library.FieldValue{Field: f, Text: in.values[i]}
	}
	before := lib.PaperIDs(key)
	changed, err := lib.SaveFields(key, newKey, fields)
	foundByNewID := lib.PaperIDs(newKey) != before
	in.err = ""
	if err != nil {
		// Beside the field, the file's path only gets in the way.
		var invalid *library.InvalidLibrary
		if errors.As(err, &invalid) {
			in.err = invalid.Reason
		} else {
			in.err = err.Error()
		}
	}
	if !changed {
		return
	}
	// Before the refresh: bibliographies list what documents cite.
	if newKey != key {
		s.RenameCitations(key, newKey)
	}
	s.refresh()
	if newKey != key {
		in.key = newKey
		s.selectOnly(newKey)
		s.renamePaper(key, newKey)
	}
	if foundByNewID {
		s.h.FetchPapers([]string{newKey})
	}
}

// chooseType changes the entry's type, asking first if that would lose data.
func (s *State) chooseType(kind *library.SourceType) {
	lib := s.library()
	if s.insp == nil || lib == nil {
		return
	}
	key := s.insp.key
	if lib.TypeChangeDropsParent(key, kind) {
		s.h.Confirm(fmt.Sprintf("Change to %s?", kind.Label),
			"This removes the publication it appeared in, along with that publication's details. It cannot be undone.",
			"Change", true, func() { s.setSourceType(key, kind) })
		return
	}
	s.setSourceType(key, kind)
}

func (s *State) setSourceType(key string, kind *library.SourceType) {
	s.SaveInspector()
	lib := s.library()
	if lib == nil {
		return
	}
	if err := lib.SetSourceType(key, kind); err != nil {
		s.h.ToastError(fmt.Sprintf("Could not change the type: %v", err))
	}
	if s.insp != nil && s.insp.key == key {
		s.insp = s.buildInspector(key)
	}
	s.refresh()
}

// hint is worded as an example: a bare value in an empty field reads as data.
func hint(example string) string {
	if example == "" {
		return ""
	}
	return "e.g. " + example
}

func (s *State) inspectorView(c *ui.Context) {
	in, lib := s.insp, s.library()
	if in == nil || lib == nil {
		return
	}
	kind, ok := lib.SourceType(in.key)
	if !ok {
		return
	}
	t := c.Theme()
	var after func()
	row := func(label string, value func()) {
		ui.Row(c).AlignItems(ui.Start).Gap(10).PaddingY(4).Children(func() {
			ui.Text(c, label).Width(92).Shrink(0).TextAlign(ui.End).FontSize(12).TextColor(theme.Caption(t)).Padding(4, 0, 0, 0)
			ui.Column(c).Grow(1).MinWidth(0).Children(value)
		})
	}
	focused := -1
	field := func(i int, value *string, label, placeholder string, long bool) {
		ctx := c.Key("field:" + label)
		var el ui.Element
		if long {
			el = ui.TextArea(ctx, value).Lines(1, 4)
		} else {
			el = ui.TextInput(ctx, value)
		}
		el.Label(label).Placeholder(placeholder).FontSize(13)
		if i == 0 {
			el.Bind(&s.first)
		}
		el.OnTextKey(func(k ui.TextKey) bool {
			switch {
			case k.Key == ui.KeyEscape:
				s.revertInspector()
				return true
			case long && k.Key == ui.KeyEnter && k.Mods == 0:
				s.SaveInspector()
				return true
			}
			return false
		})
		if el.Submitted() {
			after = s.SaveInspector
		}
		if el.Focused() {
			focused = i
		}
	}
	ui.Scroll(c.Key("inspector")).Width(inspectorW).Shrink(0).Background(theme.Group(t)).
		BorderStartWidth(1).BorderColor(theme.Hairline(t)).Padding(8, 12, 16, 12).Children(func() {
		row("Type", func() {
			ui.PopoverMenuButton(c.Key("inspector-type"), kind.Label, func(m *ui.Menu) {
				for _, k := range library.SourceTypes {
					if m.Item(k.Label).Checked(k == kind).Chosen() {
						after = func() { s.chooseType(k) }
					}
				}
			})
		})
		for i, f := range in.fields {
			row(f.Label, func() { field(i, &in.values[i], f.Label, hint(f.Hint), f.Long) })
		}
		row("Citation Key", func() { field(len(in.fields), &in.keyText, "Citation Key", hint("knuth1984"), false) })
		ui.Text(c, "Cited In").FontSize(12).TextColor(theme.Caption(t)).Padding(14, 4, 2, 4)
		if len(in.citedIn) == 0 {
			ui.Text(c, "No document yet").FontSize(13).TextColor(t.TextMuted).PaddingX(4)
		}
		for _, cited := range in.citedIn {
			b := ui.ButtonBase(c.Key("cited:" + cited.id)).Height(28).PaddingX(4).Gap(8).Radius(t.Radius).Label(cited.id)
			if b.Hovered() {
				b.Background(theme.Hover(t))
			}
			b.Children(func() {
				ui.Text(c, cited.id).FontSize(13).SingleLine().Grow(1).Shrink(1)
				theme.Tabular(ui.Text(c, strconv.Itoa(cited.count)).FontSize(12).TextColor(theme.Caption(t)))
			})
			if b.Clicked() {
				s.h.GoToSource(cited.id, cited.first)
			}
		}
		if in.err != "" {
			ui.Text(c, in.err).FontSize(12).TextColor(theme.Danger(t)).Padding(8, 0, 0, 0)
		}
	})
	// Leaving a field, by Tab, a click or the table, saves it.
	if in.focused >= 0 && focused != in.focused && after == nil {
		after = s.SaveInspector
	}
	in.focused = focused
	if after != nil {
		after()
	}
}
