package app

import (
	"fmt"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

// remaining is how long the Trash keeps an item deleted at deleted.
func remaining(deleted, now int64) string {
	left := deleted + vault.TrashDays*vault.Day - now
	if left < vault.Day {
		return "Deleting soon"
	}
	return fmt.Sprintf("%d days", (left+vault.Day-1)/vault.Day)
}

// refreshTrash lists the Trash, emptying what is past its time.
func (ws *Workspace) refreshTrash() {
	warnings := ws.store.PurgeExpired()
	ws.trash = ws.store.ListTrash()
	ws.Warn(warnings)
}

// trashLocation is where a trashed item goes back to: its folder, if that is still there.
func (ws *Workspace) trashLocation(t vault.TrashEntry) string {
	if e, ok := ws.Entry(t.Parent); ok && t.Parent != "" {
		return vault.DisplayTitle(e.Title)
	}
	return "Files"
}

func (ws *Workspace) askRestore(id string) {
	ws.confirm = confirmState{open: true, title: "Restore this document?", body: "The document will be restored to its original location.", action: "Restore", then: func() { ws.restoreDoc(id) }}
}

func (ws *Workspace) askPurge(id string) {
	ws.confirm = confirmState{open: true, title: "Delete permanently?", body: "This item cannot be recovered after permanent deletion.", action: "Delete", danger: true, then: func() { ws.purgeDoc(id) }}
}

func (ws *Workspace) restoreDoc(id string) {
	restored, err := ws.store.Restore(id)
	switch {
	case err != nil:
		ws.ToastError(fmt.Sprintf("Could not restore document: %v", err))
	case restored.Renamed:
		ws.Toast("Restored. Some documents were moved to Files because their original location no longer exists.")
	default:
		ws.Toast("Restored successfully")
	}
	ws.RefreshEntries()
	ws.followTopDocument()
	ws.refreshOutline()
	ws.refreshTrash()
	ws.Warn(restored.Warnings)
}

func (ws *Workspace) purgeDoc(id string) {
	if err := ws.store.Purge(id); err != nil {
		ws.ToastError(fmt.Sprintf("Could not delete document: %v", err))
	} else {
		ws.Toast("Permanently deleted")
	}
	ws.refreshTrash()
}

func (ws *Workspace) trashView(c *ui.Context) {
	t := c.Theme()
	ws.pageHeader(c, "Trash", func() { ws.searchButton(c) })
	if len(ws.trash) == 0 {
		emptyState(c, theme.Icons.Delete, "Trash is empty", "Deleted documents stay here for a while.", nil)
		return
	}
	header := func(label string, width float32) {
		cell := ui.Text(c, label).FontSize(12).FontWeight(600).TextColor(theme.Caption(t)).PaddingStart(12)
		if width > 0 {
			cell.Width(width)
		} else {
			cell.Grow(1).Basis(0)
		}
	}
	ui.Row(c).Height(32).MarginX(8).AlignItems(ui.Center).BorderWidth(0, 0, 1, 0).BorderColor(theme.Hairline(t)).Children(func() {
		header("Title", 0)
		header("Location", 168)
		header("Time remaining", 168)
		ui.Box(c).Width(48)
	})
	now := vault.Now()
	ui.Scroll(c.Key("trash-list")).Grow(1).Padding(0, 8, 24, 8).Children(func() {
		for i, item := range ws.trash {
			id := item.ID
			menu := func(m *ui.Menu) {
				if m.Item("Restore").Chosen() {
					ws.after(func() { ws.askRestore(id) })
				}
				if m.Item("Delete permanently").Chosen() {
					ws.after(func() { ws.askPurge(id) })
				}
			}
			row := ui.Row(c.Key(id)).Height(40).AlignItems(ui.Center).PopoverMenu(menu)
			if i > 0 {
				row.BorderWidth(1, 0, 0, 0).BorderColor(theme.Hairline(t))
			}
			row.Children(func() {
				ui.Row(c).Grow(1).Basis(0).PaddingStart(12).Gap(8).AlignItems(ui.Center).Children(func() {
					theme.Glyph(c, theme.Icons.Doc, 15, t.TextMuted)
					ui.Text(c, vault.DisplayTitle(item.Title)).FontSize(13).SingleLine().Ellipsis("…")
				})
				ui.Row(c).Width(168).PaddingStart(12).Gap(6).AlignItems(ui.Center).Children(func() {
					theme.Glyph(c, theme.Icons.Folder, 15, t.TextMuted)
					ui.Text(c, ws.trashLocation(item)).FontSize(13).TextColor(theme.Caption(t)).SingleLine().Ellipsis("…")
				})
				theme.Tabular(ui.Text(c, remaining(item.Deleted, now)).FontSize(13).TextColor(theme.Caption(t)).Width(168).PaddingStart(12))
				ui.Row(c).Width(48).Justify(ui.End).PaddingEnd(4).Children(func() {
					theme.IconButton(c, theme.Icons.Ellipsis, "More").PopoverMenuButton(menu)
				})
			})
		}
	})
}
