package app

import (
	"errors"
	"fmt"
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
)

// confirmState is a question with a primary or danger action, and maybe a second one.
type confirmState struct {
	open                bool
	title, body, action string
	danger              bool
	then                func()
	// other labels a second action, as Discard changes, which runs orElse.
	other  string
	orElse func()
}

type moveState struct {
	open bool
	ids  []string
	// target is the chosen folder, "" for the vault root, once picked.
	target   string
	picked   bool
	expanded map[string]bool
	query    string
}

type historyState struct {
	open bool
	// id is the document, or vault.LibraryFile for the library.
	id       string
	library  bool
	versions []int64
	selected int
	diff     *editor.DiffState
	same     bool
}

// closeOverlays closes the shell's dialogs: they belong to the page they opened on.
func (ws *Workspace) closeOverlays() {
	ws.search.open = false
	ws.move.open = false
	ws.history.open = false
	ws.words = false
}

// overlays builds the shell's dialogs, every frame.
func (ws *Workspace) overlays(c *ui.Context) {
	ws.searchDialog(c)
	ws.moveDialog(c)
	ws.historyDialog(c)
	ws.wordCountDialog(c)
	// Last, so that the Save failed dialog stays above another dialog.
	ws.confirmDialog(c)
}

func dialogButtons(c *ui.Context, fn func()) {
	ui.Row(c).Gap(8).Justify(ui.End).Margin(12, 0, 0, 0).Children(fn)
}

func dangerButton(c *ui.Context, label string) ui.Element {
	t := c.Theme()
	b := ui.PrimaryButton(c, label).Background(t.Danger).TextColor(t.AccentText)
	return b
}

func (ws *Workspace) confirmDialog(c *ui.Context) {
	if !ws.confirm.open {
		return
	}
	cf := ws.confirm
	theme.Dialog(ui.Modal(c, &ws.confirm.open, func() {
		ui.Text(c, cf.title).FontSize(15).FontWeight(600)
		ui.Text(c, cf.body).FontSize(13).TextColor(theme.Caption(c.Theme()))
		dialogButtons(c, func() {
			if ui.Button(c, "Cancel").Clicked() {
				ws.confirm.open = false
			}
			if cf.other != "" && ui.Button(c, cf.other).TextColor(c.Theme().Danger).Clicked() {
				ws.confirm.open = false
				ws.after(cf.orElse)
			}
			var action ui.Element
			if cf.danger {
				action = dangerButton(c, cf.action)
			} else {
				action = ui.PrimaryButton(c, cf.action)
			}
			if action.Clicked() {
				ws.confirm.open = false
				ws.after(cf.then)
			}
		})
	}).Width(400), c.Theme())
}

// openMove opens the Move to dialog for ids.
func (ws *Workspace) openMove(ids []string) {
	ws.closeOverlays()
	ws.move = moveState{open: true, ids: ids, expanded: map[string]bool{}}
}

func (ws *Workspace) moveDialog(c *ui.Context) {
	if !ws.move.open {
		return
	}
	t := c.Theme()
	width, _ := c.Size()
	tr := newTree(ws.entries)
	hidden := map[string]bool{}
	for _, id := range ws.move.ids {
		for _, sub := range tr.subtree(id) {
			hidden[sub] = true
		}
	}
	q := strings.ToLower(strings.TrimSpace(ws.move.query))
	theme.Dialog(ui.Modal(c, &ws.move.open, func() {
		ui.Text(c, "Move from Files to").FontSize(15).FontWeight(600)
		ui.Row(c).Height(512).Margin(8, 0, 0, 0).Radius(8).Border(1, theme.Hairline(t)).Clip().AlignItems(ui.Stretch).Children(func() {
			ui.Column(c).Width(240).Padding(12, 8).Gap(10).Background(theme.Group(t)).BorderWidth(0, 1, 0, 0).BorderColor(theme.Hairline(t)).Children(func() {
				ui.SearchField(c, &ws.move.query).Placeholder("Search destinations").AutoFocus()
				ui.Row(c).Height(30).PaddingX(10).Gap(8).Radius(6).Background(theme.Selected(t)).AlignItems(ui.Center).Children(func() {
					theme.Glyph(c, theme.Icons.Library, 15, t.TextMuted)
					ui.Text(c, "Vault").FontSize(13)
				})
			})
			ui.Scroll(c.Key("move-tree")).Grow(1).Padding(12, 8).Children(func() {
				if q != "" {
					found := false
					for _, e := range ws.entries {
						if e.Folder && !hidden[e.ID] && strings.Contains(strings.ToLower(vault.DisplayTitle(e.Title)), q) {
							found = true
							ws.moveRow(c, e.ID, vault.DisplayTitle(e.Title), 0, false, false)
						}
					}
					if !found {
						ui.Text(c, "No results").FontSize(13).TextColor(theme.Caption(t)).Padding(80, 0, 0, 0).AlignSelf(ui.Center)
					}
					return
				}
				ws.moveRow(c, "", "Vault", 0, false, false)
				for _, r := range tr.rows(func(id string) bool { return ws.move.expanded[id] }) {
					if !r.entry.Folder || hidden[r.entry.ID] {
						continue
					}
					kids := false
					for _, k := range tr.children(r.entry.ID) {
						kids = kids || k.Folder && !hidden[k.ID]
					}
					ws.moveRow(c, r.entry.ID, vault.DisplayTitle(r.entry.Title), r.depth, kids, true)
				}
			})
		})
		dialogButtons(c, func() {
			if ui.Button(c, "Cancel").Clicked() {
				ws.move.open = false
			}
			if ui.PrimaryButton(c, "OK").Disabled(!ws.move.picked).Clicked() {
				ids, target := ws.move.ids, ws.move.target
				ws.move.open = false
				ws.exitSelection()
				ws.after(func() { ws.moveDocs(ids, target) })
			}
		})
	}).Width(min(828, width-48)), c.Theme())
}

// moveRow is a destination: the vault root (id "") or a folder.
func (ws *Workspace) moveRow(c *ui.Context, id, title string, depth int, kids, folder bool) {
	t := c.Theme()
	row := ui.ButtonBase(c.Key("move:"+id)).Label(title).Height(30).Margin(0, 0, 2, 0).Padding(0, 5, 0, 16+20*float32(depth)).Gap(8).Radius(6).AlignItems(ui.Center)
	if ws.move.picked && ws.move.target == id {
		row.Background(theme.Selected(t))
	}
	if row.Clicked() {
		ws.move.target, ws.move.picked = id, true
	}
	row.Children(func() {
		if folder {
			arrow := ui.ButtonBase(c).Label("Expand").Size(20, 20).Center().Radius(5)
			if kids {
				chevron := theme.Icons.ChevronRight
				if ws.move.expanded[id] {
					chevron = theme.Icons.ChevronDown
				}
				arrow.Children(func() { theme.Glyph(c, chevron, 10, t.TextMuted) })
				if arrow.Clicked() {
					ws.move.expanded[id] = !ws.move.expanded[id]
				}
			}
		}
		glyph := theme.Icons.Folder
		if id == "" {
			glyph = theme.Icons.Library
		}
		theme.Glyph(c, glyph, 16, t.TextMuted)
		ui.Text(c, title).FontSize(13).SingleLine().Ellipsis("…").Grow(1)
	})
}

// openHistory opens Version History for the open document, once it is saved.
func (ws *Workspace) openHistory() {
	if ws.doc == nil || !ws.flushDoc() {
		return
	}
	ws.openVersions(ws.doc.ID(), false)
}

// openLibraryHistory opens Version History for the library.
func (ws *Workspace) openLibraryHistory() {
	ws.cites.SaveInspector()
	if ws.lib != nil {
		ws.openVersions(vault.LibraryFile, true)
	}
}

func (ws *Workspace) openVersions(id string, library bool) {
	ws.closeOverlays()
	ws.history = historyState{open: true, id: id, library: library}
	versions, err := ws.historyVersions()
	if err != nil {
		ws.history.open = false
		ws.ToastError(err.Error())
		return
	}
	ws.history.versions = versions.List()
	ws.showVersion(0)
}

func (ws *Workspace) historyVersions() (*vault.Versions, error) {
	if ws.history.library {
		if ws.lib == nil {
			return nil, errors.New("The library could not be read")
		}
		return ws.lib.Versions(), nil
	}
	return ws.store.Versions(ws.history.id)
}

// historyText is what the dialog compares against: the document or the library now.
func (ws *Workspace) historyText() (string, error) {
	if ws.history.library {
		if ws.lib == nil {
			return "", errors.New("The library could not be read")
		}
		return ws.lib.Text(), nil
	}
	return ws.TextOf(ws.history.id)
}

func (ws *Workspace) versionText(time int64) (string, error) {
	versions, err := ws.historyVersions()
	if err != nil {
		return "", err
	}
	return versions.Read(time)
}

// showVersion compares a version with now: what restoring it would change.
func (ws *Workspace) showVersion(i int) {
	h := &ws.history
	if i >= len(h.versions) {
		return
	}
	current, err := ws.historyText()
	if err != nil {
		ws.ToastError(err.Error())
		return
	}
	text, err := ws.versionText(h.versions[i])
	if err != nil {
		ws.ToastError(err.Error())
		return
	}
	h.selected, h.same = i, text == current
	h.diff = &editor.DiffState{Old: editor.NewBuffer(current), New: editor.NewBuffer(text), Layout: editor.DiffUnified}
}

// restoreVersion brings a version back. What it replaces becomes a version
// first, and the editor can undo it.
func (ws *Workspace) restoreVersion() {
	h := ws.history
	time := h.versions[h.selected]
	text, err := ws.versionText(time)
	if err != nil {
		ws.ToastError(err.Error())
		return
	}
	if h.library {
		if err := ws.lib.Restore(text); err != nil {
			ws.ToastError(fmt.Sprintf("Could not restore the library: %v", err))
			return
		}
		ws.cites.LibraryRestored(text)
	} else {
		if current, err := ws.TextOf(h.id); err == nil {
			ws.KeepVersion(h.id, current, true)
		}
		if err := ws.WriteDocument(h.id, text, false); err != nil {
			ws.ToastError(fmt.Sprintf("Could not save %s: %v", h.id, err))
			return
		}
	}
	ws.history.open = false
	ws.Toast("Restored the version from " + formatVisit(time, vault.Now()))
}

func (ws *Workspace) historyDialog(c *ui.Context) {
	if !ws.history.open {
		return
	}
	t := c.Theme()
	h := &ws.history
	width, height := c.Size()
	now := vault.Now()
	theme.Dialog(ui.Modal(c, &h.open, func() {
		name := vault.BaseName(h.id)
		if h.library {
			name = "the Library"
		}
		ui.Text(c, "Version History of "+name).FontSize(15).FontWeight(600)
		ui.Text(c, "Green is what restoring brings back. Red is what it replaces.").FontSize(12).TextColor(theme.Caption(t))
		ui.Row(c).Grow(1).Margin(12, 0, 0, 0).Border(1, theme.Hairline(t)).Radius(8).Clip().AlignItems(ui.Stretch).Children(func() {
			ui.Scroll(c.Key("versions")).Width(220).Padding(8).BorderWidth(0, 1, 0, 0).BorderColor(theme.Hairline(t)).Children(func() {
				for i, time := range h.versions {
					row := ui.ButtonBase(c.Key(time)).Height(30).PaddingX(10).Radius(6).AlignItems(ui.Center)
					if i == h.selected {
						row.Background(theme.Selected(t))
					}
					row.Children(func() { ui.Text(c, formatVisit(time, now)).FontSize(13) })
					if row.Clicked() {
						ws.after(func() { ws.showVersion(i) })
					}
				}
			})
			ui.Column(c).Grow(1).Basis(0).Children(func() {
				switch {
				case len(h.versions) == 0:
					ui.Text(c, "Versions appear here as you write.").FontSize(13).TextColor(theme.Caption(t)).Padding(16)
				case h.same || h.diff == nil:
					ui.Text(c, "This version is the same as the document now.").FontSize(13).TextColor(theme.Caption(t)).Padding(16)
				default:
					editor.DiffView(c, h.diff).Grow(1).FillWidth()
				}
			})
		})
		dialogButtons(c, func() {
			if ui.Button(c, "Close").Clicked() {
				h.open = false
			}
			if ui.PrimaryButton(c, "Restore This Version").Disabled(h.same || len(h.versions) == 0).Clicked() {
				ws.after(ws.restoreVersion)
			}
		})
	}).Width(min(960, width-48)).Height(min(620, height-48)), c.Theme())
}

func (ws *Workspace) wordCountDialog(c *ui.Context) {
	if !ws.words || ws.doc == nil {
		ws.words = false
		return
	}
	t := c.Theme()
	files := ws.doc.Words()
	total := 0
	for _, f := range files {
		total += f.Words
	}
	row := func(name string, count int, strong bool) {
		ui.Row(c).Height(28).Gap(16).AlignItems(ui.Center).Children(func() {
			label := ui.Text(c, name).FontSize(13).SingleLine().Ellipsis("…").Grow(1)
			if strong {
				label.FontWeight(600)
			}
			theme.Tabular(ui.Text(c, formatCount(count)).FontSize(13).TextColor(theme.Caption(t)))
		})
	}
	theme.Dialog(ui.Modal(c, &ws.words, func() {
		ui.Text(c, "Word Count").FontSize(15).FontWeight(600).Margin(0, 0, 10, 0)
		ui.Scroll(c.Key("word-count-files")).MaxHeight(320).Children(func() {
			for _, f := range files {
				row(f.Path, f.Words, f.Path == ws.Current())
			}
		})
		ui.Column(c).Margin(6, 0, 0, 0).Padding(6, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(theme.Hairline(t)).Children(func() {
			row("Total", total, true)
		})
		ui.Text(c, "Counts the text the document draws. The bibliography and page numbers are left out.").FontSize(12).TextColor(theme.Caption(t)).Margin(8, 0, 0, 0)
		dialogButtons(c, func() {
			if ui.Button(c, "Close").Clicked() {
				ws.words = false
			}
		})
	}).Width(440), c.Theme())
}
