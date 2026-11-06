package citations

import (
	"fmt"
	"slices"
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/egoist/mygo/ui"
)

// tagEditor is the popover that tags the chosen entries. It hangs from the
// Tags cell of anchor's row, or over the table while that row is out of view.
type tagEditor struct {
	open     bool
	keys     []string
	anchor   string
	text     string
	anchored bool
}

// tagRename is the popover that renames a tag, under its chip.
type tagRename struct {
	open      bool
	tag, text string
}

func (s *State) openTagEditor(keys []string, anchor string) {
	s.tagEditor = tagEditor{open: true, keys: slices.Clone(keys), anchor: anchor}
}

// setTag applies tag to keys, or takes it off. False if it was refused.
func (s *State) setTag(keys []string, tag string, on bool) bool {
	lib := s.library()
	if lib == nil {
		return false
	}
	err := lib.SetTag(keys, tag, on)
	if err != nil {
		s.h.ToastError(fmt.Sprintf("Could not change tags: %v", err))
	}
	s.TableChanged()
	return err == nil
}

func (s *State) tagCount(tag string) int {
	if lib := s.library(); lib != nil {
		for _, t := range lib.Tags() {
			if t.Name == tag {
				return t.Count
			}
		}
	}
	return 0
}

func (s *State) askDeleteTag(tag string) {
	entries := "1 entry"
	if n := s.tagCount(tag); n != 1 {
		entries = fmt.Sprintf("%d entries", n)
	}
	s.h.Confirm(fmt.Sprintf("Delete the tag %s?", tag), fmt.Sprintf("This removes it from %s. The entries stay in the library.", entries),
		"Delete", true, func() { s.deleteTag(tag) })
}

func (s *State) deleteTag(tag string) {
	lib := s.library()
	if lib == nil {
		return
	}
	if err := lib.DeleteTag(tag); err != nil {
		s.h.ToastError(fmt.Sprintf("Could not delete the tag: %v", err))
	}
	s.TableChanged()
}

func (s *State) finishTagRename() {
	old, renamed := s.tagRename.tag, strings.TrimSpace(s.tagRename.text)
	s.tagRename.open = false
	lib := s.library()
	if lib == nil {
		return
	}
	if renamed != old {
		if err := lib.RenameTag(old, renamed); err != nil {
			s.h.ToastError(fmt.Sprintf("Could not rename the tag: %v", err))
		} else if s.tagFilter == old {
			s.tagFilter, _ = lib.TagLike(renamed)
		}
	}
	s.TableChanged()
}

func popoverPanel(t *ui.Theme, panel ui.Element, width float32) {
	// Rows are rounded 6 inside 6 of padding: the panel's 12 is concentric.
	theme.Elevate(panel.Width(width).Margin(6, 0, 0, 0).Padding(6).Radius(12).Background(t.Background), t)
}

func (s *State) tagRenameView(c *ui.Context, chip ui.Element) {
	t := c.Theme()
	var after func()
	p := ui.PopoverBase(c.Key("tag-rename"), chip, &s.tagRename.open, func(panel ui.Element) {
		popoverPanel(t, panel, 220)
		field := ui.TextInput(c.Key("tag-rename-field"), &s.tagRename.text).Placeholder("Tag name").Label("Tag name").AutoFocus()
		field.OnTextKey(func(k ui.TextKey) bool {
			if k.Key != ui.KeyEscape {
				return false
			}
			s.tagRename.open = false
			return true
		})
		if field.Submitted() {
			after = s.finishTagRename
		}
	})
	// A press outside keeps what was typed, as clicking away from a rename in Finder does.
	if p.Dismissed() {
		after = s.finishTagRename
	}
	if after != nil {
		after()
	}
}

func (s *State) tagEditorView(c *ui.Context, anchor ui.Element, over bool) {
	lib := s.library()
	if lib == nil {
		return
	}
	s.tagEditor.anchored = true
	t := c.Theme()
	ed := &s.tagEditor
	var after func()
	ui.PopoverBase(c.Key("tag-editor"), anchor, &ed.open, func(panel ui.Element) {
		popoverPanel(t, panel, 240)
		if over {
			panel.AttachTo(anchor, ui.AnchorCenter, ui.AnchorCenter)
		}
		keys := ed.keys
		field := ui.TextInput(c.Key("tag-editor-field"), &ed.text).Placeholder("Add a tag").Label("Add a tag").AutoFocus().Margin(0, 2, 4, 2)
		if field.Submitted() && strings.TrimSpace(ed.text) != "" {
			tag := ed.text
			after = func() {
				if s.setTag(keys, tag, true) {
					ed.text = ""
				}
			}
		}
		typed := strings.TrimSpace(ed.text)
		query := strings.ToLower(typed)
		var rows []library.Row
		for _, row := range lib.Rows() {
			if slices.Contains(keys, row.Key) {
				rows = append(rows, row)
			}
		}
		item := func(key string, mark func(), label string) ui.Element {
			b := ui.ButtonBase(c.Key(key)).Height(26).PaddingX(8).Gap(6).Radius(6).Label(label)
			if b.Hovered() {
				b.Background(theme.Hover(t))
			}
			return b.Children(func() {
				ui.Box(c).Width(16).Shrink(0).Children(mark)
				ui.Text(c, label).FontSize(13).SingleLine()
			})
		}
		ui.Scroll(c.Key("tag-editor-list")).MaxHeight(220).Children(func() {
			for _, tag := range lib.Tags() {
				if !strings.Contains(strings.ToLower(tag.Name), query) {
					continue
				}
				holders := 0
				for _, row := range rows {
					if slices.Contains(row.Tags, tag.Name) {
						holders++
					}
				}
				all := holders == len(rows)
				mark := func() {}
				switch {
				case all:
					mark = func() { theme.Glyph(c, theme.Icons.Check, 13, t.Text) }
				case holders > 0:
					mark = func() { theme.Glyph(c, theme.Icons.Minus, 13, t.Text) }
				}
				if item("tag-row:"+tag.Name, mark, tag.Name).Clicked() {
					name := tag.Name
					after = func() { s.setTag(keys, name, !all) }
				}
			}
		})
		if _, exists := lib.TagLike(typed); typed != "" && !exists {
			plus := func() { theme.Glyph(c, theme.Icons.Plus, 13, t.Text) }
			if item("tag-create", plus, fmt.Sprintf("Create “%s”", typed)).Clicked() {
				after = func() {
					if s.setTag(keys, typed, true) {
						ed.text = ""
					}
				}
			}
		}
	})
	if after != nil {
		after()
	}
}
