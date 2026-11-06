package citations

import (
	"fmt"
	"slices"
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/egoist/mygo/ui"
)

const pickerRows = 8

// picker chooses a source: to cite, or for a paper.
type picker struct {
	open     bool
	opts     host.PickSourceOptions
	query    string
	selected int
}

// OpenPicker opens the picker to cite a source in the open document.
func (s *State) OpenPicker() {
	s.OpenSourcePicker(host.PickSourceOptions{Then: s.h.InsertCitation})
}

// OpenSourcePicker opens the picker for what opts says.
func (s *State) OpenSourcePicker(opts host.PickSourceOptions) {
	s.h.ReloadLibrary()
	s.picker = picker{open: true, opts: opts}
}

func (s *State) pickerRows() []library.Row {
	rows := s.VisibleRows(strings.ToLower(strings.TrimSpace(s.picker.query)))
	// A source holds one paper.
	if s.picker.opts.ExcludeWithPaper {
		rows = slices.DeleteFunc(rows, func(r library.Row) bool { return s.h.HasPaper(r.Key) })
	}
	return rows[:min(len(rows), pickerRows)]
}

func (s *State) pick(key string) {
	then := s.picker.opts.Then
	s.picker.open = false
	if then != nil {
		then(key)
	}
}

func (s *State) pickerView(c *ui.Context) {
	if !s.picker.open {
		return
	}
	t := c.Theme()
	rows := s.pickerRows()
	var after func()
	theme.Dialog(ui.Modal(c.Key("source-picker"), &s.picker.open, func() {
		ui.Column(c).Width(520).Gap(8).Children(func() {
			if s.picker.opts.Prompt != "" {
				ui.Text(c, s.picker.opts.Prompt).FontSize(13).FontWeight(600).PaddingX(4)
			}
			n := len(rows)
			field := ui.TextInput(c.Key("picker-search"), &s.picker.query).Placeholder("Search library").Label("Search library").AutoFocus()
			field.OnTextKey(func(k ui.TextKey) bool {
				switch k.Key {
				case ui.KeyUp:
					s.picker.selected = max(s.picker.selected-1, 0)
				case ui.KeyDown:
					s.picker.selected = max(min(s.picker.selected+1, n-1), 0)
				default:
					return false
				}
				return true
			})
			if field.Changed() {
				s.picker.selected = 0
			}
			if field.Submitted() && s.picker.selected < n {
				key := rows[s.picker.selected].Key
				after = func() { s.pick(key) }
			}
			if n == 0 {
				ui.Text(c, "No matching entries").FontSize(13).TextColor(theme.Caption(t)).Padding(8, 12)
			}
			for i, row := range rows {
				b := ui.ButtonBase(c.Key("pick:"+row.Key)).Column().AlignItems(ui.Stretch).Padding(6, 12).Radius(6).Label(row.Label())
				if i == s.picker.selected || b.Hovered() {
					b.Background(theme.Selected(t))
				}
				b.Children(func() {
					ui.Text(c, row.Label()).FontSize(13).SingleLine()
					ui.Text(c, fmt.Sprintf("%s · %s · @%s", row.Authors, row.Year, row.Key)).FontSize(12).TextColor(theme.Caption(t)).SingleLine()
				})
				if b.Clicked() {
					key := row.Key
					after = func() { s.pick(key) }
				}
			}
			if alt := s.picker.opts.Alternative; alt != "" {
				ui.Row(c).Children(func() {
					if ui.Button(c.Key("picker-alternative"), alt).Clicked() {
						on := s.picker.opts.OnAlternative
						after = func() {
							s.picker.open = false
							if on != nil {
								on()
							}
						}
					}
				})
			}
		})
	}), c.Theme())
	if after != nil {
		after()
	}
}

// Overlays are the Library's dialogs, built every frame.
func (s *State) Overlays(c *ui.Context) {
	s.pickerView(c)
	s.lookupView(c)
}
