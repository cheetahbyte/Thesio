package papers

import (
	"path/filepath"
	"slices"
	"strconv"
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

// paper is a row of the Sources page.
type paper struct {
	id, title string
	row       library.Row
	linked    bool
}

// papers are the PDFs in `sources/` by title.
func (s *State) papers() []paper {
	var rows []library.Row
	if lib := s.h.Library(); lib != nil {
		rows = lib.Rows()
	}
	var out []paper
	for _, id := range s.PaperIDs() {
		p := paper{id: id, title: vault.BaseName(id)}
		if row, ok := library.RowOfPaper(rows, id); ok {
			p.row, p.linked = row, true
			if row.Title != "" {
				p.title = row.Title
			}
		}
		out = append(out, p)
	}
	slices.SortStableFunc(out, func(a, b paper) int {
		switch {
		case vault.NaturalLess(a.title, b.title):
			return -1
		case vault.NaturalLess(b.title, a.title):
			return 1
		}
		return 0
	})
	return out
}

// addPaper asks for a PDF, then for the source it belongs to.
func (s *State) addPaper() {
	go func() {
		paths, err := s.h.PickFiles("pdf", false)
		s.h.Update(func() {
			if err != nil {
				s.h.ToastError("Could not choose a file: " + err.Error())
				return
			}
			if len(paths) == 0 {
				return
			}
			path := paths[0]
			stem := strings.TrimSuffix(filepath.Base(path), filepath.Ext(path))
			s.h.PickSource(host.PickSourceOptions{
				Prompt:           "Choose the source for this paper",
				ExcludeWithPaper: true,
				Alternative:      "Add Without a Source",
				OnAlternative:    func() { s.AttachPaper(path, library.PaperNamed(stem)) },
				Then:             func(key string) { s.AttachPaper(path, library.PaperFile(key)) },
			})
		})
	}()
}

func (s *State) linkToSource(id string) {
	s.h.PickSource(host.PickSourceOptions{
		Prompt:           "Choose the source for this paper",
		ExcludeWithPaper: true,
		Then:             func(key string) { s.LinkPaper(id, key) },
	})
}

func (s *State) reveal(id string) {
	if path, err := s.h.Store().Path(id); err == nil {
		s.h.Reveal(path)
	}
}

func (s *State) deletePaper(p paper) {
	s.h.Confirm("Delete “"+p.title+"”?", "The paper moves to the Trash. Its notes and highlights stay with it there.", "Delete", true, func() {
		if s.current == p.id {
			s.ClosePaper()
		}
		warnings, err := s.h.Store().Delete(p.id)
		if err != nil {
			s.h.ToastError("Could not delete the paper: " + err.Error())
			return
		}
		s.h.Warn(warnings)
		s.h.RefreshEntries()
	})
}

// SourcesView is the Sources page: the papers in the vault.
func (s *State) SourcesView(c *ui.Context) {
	t := c.Theme()
	papers := s.papers()
	ui.Column(c).Fill().AlignItems(ui.Stretch).Children(func() {
		s.header(c, "Sources", len(papers), func() {
			if s.fetching == 1 {
				ui.Text(c, "Downloading 1 paper…").FontSize(12).TextColor(theme.Caption(t))
			} else if s.fetching > 1 {
				theme.Tabular(ui.Text(c, "Downloading "+strconv.Itoa(s.fetching)+" papers…").FontSize(12).TextColor(theme.Caption(t)))
			}
			theme.AddButton(c, "Add").Tooltip("Add a PDF").OnClick(s.addPaper)
		})
		if len(papers) == 0 {
			theme.EmptyState(c, theme.Icons.Paper, "No papers yet", "Open-access papers download here, or add a PDF yourself.", nil)
			return
		}
		s.sourcesTable(c, papers)
	})
}

func (s *State) sourcesTable(c *ui.Context, papers []paper) {
	t := c.Theme()
	if s.chosen >= len(papers) {
		s.chosen = -1
	}
	cols := []ui.TableColumn{{Title: "Title"}, {Title: "Authors", Width: 200}, {Title: "Year", Width: 60}, {Title: "Key", Width: 170}}
	s.sources.Key = func(i int) any { return papers[i].id }
	s.sources.Label = func(i int) string { return papers[i].title }
	s.sources.CellMenu = func(m *ui.Menu, row, _ int) {
		p := papers[row]
		if m.Item("Open").Chosen() {
			s.h.Open(p.id)
		}
		if m.Item("Reveal in Finder").Chosen() {
			s.reveal(p.id)
		}
		if !p.linked && m.Item("Link to Source…").Chosen() {
			s.linkToSource(p.id)
		}
		m.Separator()
		if m.Item("Delete").Chosen() {
			s.deletePaper(p)
		}
	}
	table := ui.Table(c, &s.sources, cols, len(papers), func(row, col int) {
		p := papers[row]
		muted := theme.Caption(t)
		switch col {
		case 0:
			ui.Text(c, p.title).FontSize(13).SingleLine()
		case 1:
			ui.Text(c, p.row.Authors).FontSize(13).SingleLine().TextColor(muted)
		case 2:
			theme.Tabular(ui.Text(c, p.row.Year).FontSize(13).SingleLine().TextColor(muted))
		case 3:
			if p.linked {
				ui.Text(c, p.row.Key).SingleLine().Font(theme.Mono).FontSize(12).TextColor(muted)
			} else if ui.Button(c, "Link to Source…").FontSize(12).Clicked() {
				s.linkToSource(p.id)
			}
		}
	}).Grow(1).MarginX(theme.TableInset).Label("Sources")
	if (table.Changed() || table.Submitted()) && s.chosen >= 0 {
		s.h.Open(papers[s.chosen].id)
	}
}
