package app

import (
	"cmp"
	"hash/fnv"
	"slices"
	"strconv"
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

const (
	homeColumn    = 640
	homeRecent    = 5
	homeFavorites = 6
	homeUncited   = 6
)

// overview counts the writing in the vault: notes on sources stay out, since
// a source cited only in its own notes is not cited in anything.
type overview struct {
	documents, words, sources, papers int
	// uncited are keys and titles, by title.
	uncited [][2]string
}

// homeState keeps the overview until its stamp changes: parsing every
// document is too slow for a frame.
type homeState struct {
	stamp    uint64
	overview *overview
}

func (ws *Workspace) overviewStamp(rows []library.Row) uint64 {
	h := fnv.New64a()
	for _, e := range ws.entries {
		h.Write([]byte(e.ID + "\x00" + strconv.FormatInt(e.Modified, 10) + "\x00" + strconv.Itoa(len(e.Body)) + "\x00"))
	}
	for _, r := range rows {
		h.Write([]byte(r.Key + "\x00" + r.Title + "\x00"))
	}
	return h.Sum64()
}

func (ws *Workspace) computeOverview(rows []library.Row) *overview {
	o := &overview{sources: len(rows)}
	cited := map[string]bool{}
	for _, e := range ws.entries {
		if !e.Typst || strings.HasPrefix(e.ID, library.NotesDir+"/") {
			continue
		}
		o.documents++
		o.words += typst.ProseWords(typst.WithoutBibliographyBlock(e.Body))
		for _, key := range typst.CitedKeys(e.Body) {
			cited[key] = true
		}
	}
	for _, r := range rows {
		if ws.HasPaper(r.Key) {
			o.papers++
		}
		if !cited[r.Key] {
			o.uncited = append(o.uncited, [2]string{r.Key, r.Label()})
		}
	}
	slices.SortFunc(o.uncited, func(a, b [2]string) int { return cmp.Compare(a[1], b[1]) })
	return o
}

// visited is when a document was last opened, or else modified.
func visited(e vault.Entry) int64 {
	if e.Meta.Opened > 0 {
		return e.Meta.Opened
	}
	return e.Modified
}

func (ws *Workspace) homeView(c *ui.Context) {
	rows := ws.libraryRows()
	if stamp := ws.overviewStamp(rows); ws.home.overview == nil || ws.home.stamp != stamp {
		ws.home.stamp, ws.home.overview = stamp, ws.computeOverview(rows)
	}
	o := ws.home.overview
	ws.pageHeader(c, "Home", func() {
		ws.searchButton(c)
		if iconButton(c, theme.Icons.Plus, "New document").Clicked() {
			ws.after(func() { ws.NewDocument("") })
		}
	})
	anyDocument := slices.ContainsFunc(ws.entries, func(e vault.Entry) bool { return e.Typst })
	if o.sources == 0 && !anyDocument {
		emptyState(c, theme.Icons.Home, "Nothing here yet", "Start writing, or add the papers you are reading.", func() {
			ui.Row(c).Gap(8).Margin(16, 0, 0, 0).Children(func() {
				if ui.Button(c, "Add a Source").Clicked() {
					ws.after(func() { ws.ShowView(host.ViewSources) })
				}
				if ui.PrimaryButton(c, "New Document").Clicked() {
					ws.after(func() { ws.NewDocument("") })
				}
			})
		})
		return
	}
	var recent, favorites []vault.Entry
	for _, e := range ws.entries {
		if e.Typst {
			recent = append(recent, e)
		}
		if e.Meta.Starred > 0 {
			favorites = append(favorites, e)
		}
	}
	slices.SortStableFunc(recent, func(a, b vault.Entry) int { return cmp.Compare(visited(b), visited(a)) })
	slices.SortStableFunc(favorites, func(a, b vault.Entry) int { return cmp.Compare(b.Meta.Starred, a.Meta.Starred) })
	recent, favorites = recent[:min(len(recent), homeRecent)], favorites[:min(len(favorites), homeFavorites)]
	now := vault.Now()
	ui.Scroll(c.Key("home-content")).Grow(1).Children(func() {
		ui.Column(c).FillWidth().MaxWidth(homeColumn).Margin(28, ui.Auto, 64, ui.Auto).PaddingX(24).Gap(28).Children(func() {
			ui.Row(c).Gap(8).Children(func() {
				tile(c, o.documents, "Documents")
				tile(c, o.words, "Words")
				tile(c, o.sources, "Sources")
				tile(c, o.papers, "Papers")
			})
			ws.homeSection(c, "Recent", "", recent, now)
			ws.homeSection(c, "Favorites", "", favorites, now)
			if len(o.uncited) > 0 {
				homeList(c, "Not Cited Yet", strconv.Itoa(len(o.uncited)), func() {
					for _, u := range o.uncited[:min(len(o.uncited), homeUncited)] {
						key := u[0]
						if homeRow(c.Key("uncited:"+key), u[1], key).Clicked() {
							ws.after(func() {
								if ws.HasPaper(key) {
									ws.OpenPaper(library.PaperFile(key))
								} else {
									ws.ShowView(host.ViewLibrary)
								}
							})
						}
					}
				})
			}
		})
	})
}

func (ws *Workspace) homeSection(c *ui.Context, title, count string, entries []vault.Entry, now int64) {
	if len(entries) == 0 {
		return
	}
	homeList(c, title, count, func() {
		for _, e := range entries {
			id := e.ID
			if homeRow(c.Key(title+":"+id), vault.DisplayTitle(e.Title), formatVisit(visited(e), now)).Clicked() {
				ws.after(func() { ws.Open(id) })
			}
		}
	})
}

func homeList(c *ui.Context, title, count string, rows func()) {
	ui.Column(c).Gap(2).Children(func() {
		ui.Row(c).PaddingX(8).Gap(6).Margin(0, 0, 4, 0).Children(func() {
			ui.Text(c, title).FontSize(13).FontWeight(600)
			if count != "" {
				theme.Tabular(ui.Text(c, count).FontSize(13).TextColor(theme.Caption(c.Theme())))
			}
		})
		rows()
	})
}

func homeRow(c *ui.Context, title, detail string) ui.Element {
	t := c.Theme()
	row := ui.ButtonBase(c).Label(title).Height(32).PaddingX(8).Gap(10).Radius(6).AlignItems(ui.Center)
	if row.Hovered() {
		row.Background(theme.Hover(t))
	}
	row.Children(func() {
		ui.Text(c, title).FontSize(13).SingleLine().Ellipsis("…").Grow(1)
		theme.Tabular(ui.Text(c, detail).FontSize(12).TextColor(theme.Caption(t)).SingleLine())
	})
	return row
}

func tile(c *ui.Context, value int, label string) {
	t := c.Theme()
	ui.Column(c).Grow(1).Basis(0).Padding(12, 14).Gap(2).Radius(10).Background(theme.Group(t)).Children(func() {
		theme.Tabular(ui.Text(c, formatCount(value)).FontSize(22).FontWeight(600))
		ui.Text(c, label).FontSize(12).TextColor(theme.Caption(t))
	})
}
