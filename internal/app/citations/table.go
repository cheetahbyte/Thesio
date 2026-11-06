package citations

import (
	"fmt"
	"maps"
	"strconv"
	"strings"
	"time"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/egoist/mygo/ui"
)

// Column IDs, which are also the names their widths are saved under.
const (
	colTitle   = "title"
	colAuthors = "authors"
	colYear    = "year"
	colKind    = "kind"
	colTags    = "tags"
	colKey     = "key"
	colCite    = "cite"
)

const (
	minColumnW = 56
	maxColumnW = 480
	// Widths are saved once a resize pauses this long.
	widthsPause = 500 * time.Millisecond
)

var columns = []ui.TableColumn{
	{Title: "Title", ID: colTitle, MinWidth: 180, Sortable: true},
	{Title: "Authors", ID: colAuthors, Width: 180, MinWidth: minColumnW, MaxWidth: maxColumnW, Sortable: true},
	{Title: "Year", ID: colYear, Width: 64, MinWidth: minColumnW, MaxWidth: maxColumnW, Sortable: true},
	{Title: "Type", ID: colKind, Width: 120, MinWidth: minColumnW, MaxWidth: maxColumnW, Sortable: true},
	{Title: "Tags", ID: colTags, Width: 110, MinWidth: minColumnW, MaxWidth: maxColumnW, Sortable: true},
	{Title: "Key", ID: colKey, Width: 130, MinWidth: minColumnW, MaxWidth: maxColumnW, Sortable: true},
}

var citeColumn = ui.TableColumn{ID: colCite, Width: 64, Fixed: true}

// savedWidths are the saved column widths by ID. Older preferences name a column by its title.
func savedWidths(saved map[string]float32) map[string]float32 {
	widths := map[string]float32{}
	for name, width := range saved {
		for _, col := range columns {
			if col.Title == name {
				name = col.ID
			}
		}
		widths[name] = width
	}
	return widths
}

func cell(row library.Row, col string) string {
	switch col {
	case colTitle:
		return row.Title
	case colAuthors:
		return row.Authors
	case colYear:
		return row.Year
	case colKind:
		return row.Kind
	case colTags:
		return strings.Join(row.Tags, ", ")
	case colKey:
		return row.Key
	}
	return ""
}

// LibraryView is the whole Library page: header, tag chips, table, inspector.
func (s *State) LibraryView(c *ui.Context) {
	lib := s.library()
	total := 0
	if lib != nil {
		total = lib.Len()
	}
	ui.Column(c).Fill().Children(func() {
		s.header(c, lib != nil, total)
		ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			ui.Column(c).Grow(1).MinWidth(0).Children(func() { s.page(c, lib, total) })
			s.inspectorView(c)
		})
	})
}

func (s *State) header(c *ui.Context, canEdit bool, total int) {
	s.h.Bar(c, func() {
		theme.Title(c, "Library")
		if total > 0 {
			theme.Count(c, strconv.Itoa(total))
		}
		ui.Spacer(c)
		if ui.SearchField(c.Key("library-search"), &s.search).Placeholder("Search").Label("Search library").Width(200).Margin(0, 6, 0, 0).Changed() {
			s.TableChanged()
		}
		if !canEdit {
			return
		}
		hidden := s.h.Prefs().LibraryDetailsHidden
		tip := "Hide details"
		if hidden {
			tip = "Show details"
		}
		details := theme.IconButton(c.Key("library-details"), theme.Icons.Panel, tip)
		if !hidden {
			details.Background(theme.Selected(c.Theme()))
		}
		if details.Clicked() {
			s.setDetailsHidden(!hidden)
		}
		theme.IconButton(c.Key("library-more"), theme.Icons.Ellipsis, "More").PopoverMenuButton(s.libraryMenu)
		if theme.AddButton(c.Key("library-add"), "Add").Clicked() {
			s.OpenLookup()
		}
	})
}

func (s *State) libraryMenu(m *ui.Menu) {
	if m.Item("Import BibLaTeX…").Chosen() {
		s.importBib()
	}
	if m.Item("Export BibLaTeX…").Chosen() {
		s.exportBib()
	}
	if m.Item("Version History…").Chosen() {
		s.OpenLibraryHistory()
	}
	if m.Item("Find Missing Papers").Chosen() {
		s.findMissingPapers()
	}
}

// page is what is beside the inspector: the empty states, or the chips and the table.
func (s *State) page(c *ui.Context, lib *library.Library, total int) {
	t := c.Theme()
	if lib == nil {
		ui.Text(c, fmt.Sprintf("%s. Fix the file to use the Library; Thesio won't change it until it can be read.", s.h.LibraryError())).
			Padding(26).FontSize(13).TextColor(theme.Danger(t))
		return
	}
	if total == 0 {
		theme.EmptyState(c, theme.Icons.Library, "Your library is empty", "Add an entry by identifier, or import a BibLaTeX file.", nil)
		return
	}
	if tags := lib.Tags(); len(tags) > 0 {
		ui.Row(c).Wrap().Gap(4).Padding(10, 11, 6).Children(func() {
			s.chip(c, "", "All", total)
			for _, tag := range tags {
				s.chip(c, tag.Name, tag.Name, tag.Count)
			}
		})
	}
	rows := s.rows()
	if len(rows) == 0 {
		theme.EmptyState(c, theme.Icons.Search, "No results", "Try a different title, author, year, or key.", nil)
		s.syncInspector()
		return
	}
	s.table(c, rows)
}

// chip is a tag filter: filter "" is All.
func (s *State) chip(c *ui.Context, filter, label string, count int) {
	t := c.Theme()
	on := s.tagFilter == filter
	chip := ui.ButtonBase(c.Key("tag:"+filter)).Height(24).Padding(0, 9).Gap(5).Radius(12).Label(label)
	switch {
	case on:
		chip.Background(theme.Selected(t))
	case chip.Hovered():
		chip.Background(theme.Hover(t))
	}
	chip.Children(func() {
		color := theme.Caption(t)
		if on {
			color = t.Text
		}
		ui.Text(c, label).FontSize(12).TextColor(color).SingleLine()
		theme.Tabular(ui.Text(c, strconv.Itoa(count)).FontSize(12).TextColor(t.TextMuted))
	})
	if chip.Clicked() {
		s.tagFilter = filter
		s.TableChanged()
	}
	if filter == "" {
		return
	}
	chip.PopoverMenu(func(m *ui.Menu) {
		if m.Item("Rename…").Chosen() {
			s.tagRename = tagRename{open: true, tag: filter, text: filter}
		}
		if m.Item("Delete").Chosen() {
			s.askDeleteTag(filter)
		}
	})
	if s.tagRename.open && s.tagRename.tag == filter {
		s.tagRenameView(c, chip)
	}
}

func (s *State) columns() []ui.TableColumn {
	if !s.canCite() {
		return columns
	}
	return append(append([]ui.TableColumn(nil), columns...), citeColumn)
}

func (s *State) table(c *ui.Context, rows []library.Row) {
	cols := s.columns()
	var after func()
	s.list.Key = func(i int) any { return rows[i].Key }
	s.list.Label = func(i int) string { return rows[i].Label() }
	s.list.Selection = &s.chosen
	s.list.Selected = &s.cursor
	s.list.Sort = &s.sort
	s.list.CellMenu = func(m *ui.Menu, row, _ int) {
		if row < len(rows) {
			s.entryMenu(m, rows[row].Key, rows, &after)
		}
	}
	deleteSelected := func() { s.askDelete(s.selectedIn(s.rows())) }
	s.list.OnShortcut(c, 0, ui.KeyBackspace, deleteSelected)
	s.list.OnShortcut(c, 0, ui.KeyDelete, deleteSelected)
	s.list.OnShortcut(c, 0, ui.KeyEscape, func() {
		s.chosen.Clear()
		s.TableChanged()
	})
	// As on macOS: the accent while the table has the keyboard.
	active := s.list.FocusWithin(c)
	s.tagEditor.anchored = false
	table := ui.Table(c.Key("library-table"), &s.list, cols, len(rows), func(row, col int) {
		s.cell(c, rows[row], cols[col].ID, active && s.chosen.Has(rows[row].Key))
	}).Grow(1).MarginX(theme.TableInset).Label("Library")
	if table.Submitted() {
		if keys := s.selectedIn(rows); len(keys) == 1 {
			s.OpenEntry(keys[0])
		}
	}
	if s.tagEditor.open && !s.tagEditor.anchored {
		s.tagEditorView(c, table, true)
	}
	if after != nil {
		after()
	}
	s.saveWidths(c)
	s.syncInspector()
}

func (s *State) cell(c *ui.Context, row library.Row, col string, accent bool) {
	t := c.Theme()
	muted, text := theme.Caption(t), t.Text
	if accent {
		muted, text = t.AccentText, t.AccentText
	}
	switch col {
	case colTitle:
		ui.Row(c).Gap(6).MinWidth(0).Children(func() {
			ui.Text(c, row.Title).FontSize(13).TextColor(text).SingleLine().Shrink(1)
			if s.h.HasPaper(row.Key) {
				theme.Glyph(c, theme.Icons.Paper, 12, muted).Shrink(0)
			}
		})
	case colKey:
		ui.Text(c, row.Key).Font(theme.Mono).FontSize(12).TextColor(muted).SingleLine()
	case colCite:
		if ui.Button(c, "Cite").FontSize(12).Height(22).Padding(0, 10).Clicked() {
			s.h.InsertCitation(row.Key)
		}
	default:
		el := ui.Text(c, cell(row, col)).FontSize(13).TextColor(muted).SingleLine()
		if col == colYear {
			theme.Tabular(el)
		}
		if col == colTags && s.tagEditor.open && s.tagEditor.anchor == row.Key {
			s.tagEditorView(c, el, false)
		}
	}
}

// entryMenu is the menu of an entry's row. Its actions run once the table is
// built, through after, as they change the rows.
func (s *State) entryMenu(m *ui.Menu, key string, rows []library.Row, after *func()) {
	selected := s.selectedIn(rows)
	if len(selected) == 0 {
		selected = []string{key}
	}
	single := len(selected) == 1
	do := func(fn func()) { *after = fn }
	if single && m.Item("Edit").Chosen() {
		do(func() { s.OpenEntry(key) })
	}
	if m.Item("Tags…").Chosen() {
		do(func() { s.openTagEditor(selected, key) })
	}
	if single && s.canCite() && m.Item("Cite").Chosen() {
		do(func() { s.h.InsertCitation(key) })
	}
	hasPaper := s.h.HasPaper(key)
	if single {
		label := "Find Paper"
		if hasPaper {
			label = "Open Paper"
		}
		if m.Item(label).Chosen() {
			do(func() {
				if hasPaper {
					s.h.OpenPaper(library.PaperFile(key))
				} else {
					s.h.FetchPapers([]string{key})
				}
			})
		}
	}
	if single && !hasPaper && m.Item("Attach Paper…").Chosen() {
		do(func() { s.attachPaper(key) })
	}
	if single && m.Item("Notes").Chosen() {
		do(func() { s.h.OpenSourceNotes(key) })
	}
	if single && m.Item("Copy Key").Chosen() {
		do(func() { s.h.CopyText(key) })
	}
	m.Separator()
	label := "Delete"
	if !single {
		label = fmt.Sprintf("Delete %d Entries", len(selected))
	}
	if m.Item(label).Chosen() {
		do(func() { s.askDelete(selected) })
	}
}

// saveWidths writes the column widths to the preferences once a resize pauses.
func (s *State) saveWidths(c *ui.Context) {
	widths := s.list.Columns.Widths
	if !maps.Equal(widths, s.widths) {
		s.widths = maps.Clone(widths)
		s.widthsAt = c.Now()
		c.After(widthsPause)
		return
	}
	if s.widthsAt.IsZero() || c.Now().Sub(s.widthsAt) < widthsPause {
		return
	}
	s.widthsAt = time.Time{}
	s.h.Prefs().LibraryColumns = maps.Clone(widths)
	s.h.SavePrefs()
}

func (s *State) setDetailsHidden(hidden bool) {
	s.h.Prefs().LibraryDetailsHidden = hidden
	s.h.SavePrefs()
	s.syncInspector()
	if hidden {
		s.list.Handle.Focus()
	}
}

// showLibrary brings the Library page up.
func (s *State) showLibrary() {
	if s.h.CurrentView() != host.ViewLibrary {
		s.h.ShowView(host.ViewLibrary)
	}
}
