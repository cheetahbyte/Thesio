package app

import (
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

// Command is an action of the menu bar.
type Command int

const (
	CmdNewDocument Command = iota
	CmdNewFolder
	CmdInsertCitation
	CmdImport
	CmdToggleSidebar
	CmdHome
	CmdSearch
	CmdSettings
	CmdAbout
)

// Run does what a menu item says, on the main thread.
func (ws *Workspace) Run(cmd Command) {
	switch cmd {
	case CmdNewDocument:
		ws.NewDocument("")
	case CmdNewFolder:
		ws.NewFolder("")
	case CmdInsertCitation:
		ws.cites.OpenPicker()
	case CmdImport:
		ws.importFiles()
	case CmdToggleSidebar:
		ws.sidebar.open = !ws.sidebar.open
	case CmdHome:
		ws.ShowView(host.ViewHome)
	case CmdSearch:
		ws.toggleSearch()
	case CmdSettings:
		ws.OpenSettings("")
	case CmdAbout:
		ws.OpenSettings("about")
	}
	ws.Invalidate()
}

// after runs fn once the view is built: for actions that change what is being built.
func (ws *Workspace) after(fn func()) { ws.pending = append(ws.pending, fn) }

func (ws *Workspace) runAfter(c *ui.Context) {
	for len(ws.pending) > 0 {
		pending := ws.pending
		ws.pending = nil
		for _, fn := range pending {
			fn()
		}
		c.Invalidate()
	}
}

// View builds the window from the workspace, every frame.
func (ws *Workspace) View(c *ui.Context) {
	ws.drain(c)
	ws.saveWidth(c)
	ws.shortcuts(c)
	// The sidebar shows the window's material; the main pane draws its own background.
	if c.Vibrancy() {
		c.Root().Background(ui.Transparent)
	}
	if ws.sidebar.open {
		width, _ := c.Size()
		split := ui.Split(c, &ws.sidebar.width, func() { ws.sidebarView(c) }, func() { ws.mainView(c) }).Fill()
		if split.Changed() {
			ws.sidebar.width = min(max(ws.sidebar.width, sidebarMin), max(sidebarMin, min(sidebarMax, width-contentMin)))
			ws.sidebar.widthDirty, ws.sidebar.widthAt = true, c.Now()
			c.After(widthSave)
		}
	} else {
		ui.Column(c).Fill().Children(func() { ws.mainView(c) })
	}
	ws.cites.Overlays(c)
	ws.papers.Overlays(c)
	ws.overlays(c)
	ws.runAfter(c)
}

func (ws *Workspace) shortcuts(c *ui.Context) {
	switch {
	case ws.view == host.ViewSettings || ws.view == host.ViewTrash:
		c.OnShortcut(0, ui.KeyEscape, ws.closeSettings)
	case ws.sidebar.selecting:
		c.OnShortcut(0, ui.KeyEscape, ws.exitSelection)
	}
	c.OnShortcut(ui.Cmd, ui.KeyBracketRight, func() { ws.sidebar.open = !ws.sidebar.open })
}

func (ws *Workspace) mainView(c *ui.Context) {
	ui.Column(c).Fill().Background(c.Theme().Background).Children(func() {
		switch ws.view {
		case host.ViewDoc:
			ws.docView(c)
		case host.ViewHome:
			ws.homeView(c)
		case host.ViewLibrary:
			ws.cites.LibraryView(c)
		case host.ViewSources:
			ws.papers.SourcesView(c)
		case host.ViewPaper:
			ws.papers.PaperView(c)
		case host.ViewTrash:
			ws.trashView(c)
		case host.ViewSettings:
			ws.settingsView(c)
		}
	})
}

// docView is the open document under its bar: where it sits, its title, and its menu.
func (ws *Workspace) docView(c *ui.Context) {
	t := c.Theme()
	ws.topBar(c, func() {
		ui.Row(c).Shrink(1).MinWidth(0).Gap(12).AlignItems(ui.Center).Children(func() {
			ws.breadcrumbs(c)
			if ws.doc != nil {
				if err := docSaveError(ws.doc); err != "" {
					ui.Text(c, err).FontSize(12).TextColor(theme.Danger(t)).SingleLine().Ellipsis("…").MaxWidth(220)
				}
			}
		})
		ui.Spacer(c)
		if ws.doc != nil {
			iconButton(c, theme.Icons.Ellipsis, "More").PopoverMenuButton(ws.docMenu)
		}
		ws.searchButton(c)
		if iconButton(c, theme.Icons.Plus, "New document").Clicked() {
			ws.after(func() { ws.NewDocument("") })
		}
	})
	if ws.doc != nil {
		ui.Column(c).Grow(1).MinHeight(0).Children(func() { ws.doc.View(c) })
	}
}

// breadcrumbs are the open document's folders, each opening itself, the middle
// ones folded into "…" when the chain is long; then its title.
func (ws *Workspace) breadcrumbs(c *ui.Context) {
	t := c.Theme()
	id := ws.Current()
	if id == "" {
		return
	}
	chain := ws.ancestors(id)
	ui.Row(c).Shrink(1).MinWidth(0).AlignItems(ui.Center).Children(func() {
		separator := func() { theme.Glyph(c, theme.Icons.ChevronRight, 11, t.TextMuted).Margin(0, 2) }
		for i, e := range chain {
			if len(chain) > 2 && i > 0 && i < len(chain)-1 {
				if i == 1 {
					var hidden []string
					for _, h := range chain[1 : len(chain)-1] {
						hidden = append(hidden, vault.DisplayTitle(h.Title))
					}
					ui.Text(c, "…").FontSize(13).TextColor(theme.Caption(t)).Tooltip(strings.Join(hidden, " / ")).PaddingX(4)
					separator()
				}
				continue
			}
			folder := e.ID
			crumb := ui.ButtonBase(c.Key("crumb:" + folder)).Label(vault.DisplayTitle(e.Title)).MaxWidth(200).Height(24).PaddingX(4).Radius(6).AlignItems(ui.Center)
			if i == 0 {
				// The crumb's text, not its hover fill, lines up with the bar's edge.
				crumb.MarginStart(-4)
			}
			if crumb.Hovered() {
				crumb.Background(theme.Hover(t))
			}
			crumb.Children(func() {
				ui.Text(c, vault.DisplayTitle(e.Title)).FontSize(13).TextColor(theme.Caption(t)).SingleLine().Ellipsis("…")
			})
			if crumb.Clicked() {
				ws.after(func() { ws.Open(folder) })
			}
			separator()
		}
		title := ""
		if e, ok := ws.Entry(id); ok {
			title = vault.DisplayTitle(e.Title)
		}
		theme.Title(c, title)
	})
}

// docMenu is the document's "…" menu.
func (ws *Workspace) docMenu(m *ui.Menu) {
	id := ws.Current()
	e, ok := ws.Entry(id)
	if !ok {
		return
	}
	if m.Item("Move to").Chosen() {
		ws.after(func() { ws.openMove([]string{id}) })
	}
	pin := "Pin"
	if e.Meta.Pinned {
		pin = "Unpin"
	}
	if m.Item(pin).Chosen() {
		ws.after(func() { ws.togglePin(id) })
	}
	star := "Add to favorites"
	if e.Meta.Starred > 0 {
		star = "Remove from favorites"
	}
	if m.Item(star).Chosen() {
		ws.after(func() { ws.toggleStar(id) })
	}
	if e.Typst {
		m.Separator()
		if m.Item("Duplicate").Chosen() {
			ws.after(func() { ws.duplicateDoc(id) })
		}
		if m.Item("Word Count").Chosen() {
			ws.after(func() { ws.closeOverlays(); ws.words = true })
		}
		if m.Item("Version History…").Chosen() {
			ws.after(ws.openHistory)
		}
		if m.Item("Export as PDF").Chosen() {
			ws.after(func() { ws.exportPDF(id) })
		}
		ws.cites.DocMenuItems(m)
	}
	m.Separator()
	if m.Item("Delete").Chosen() {
		ws.after(func() { ws.askDelete([]string{id}, false) })
	}
}
