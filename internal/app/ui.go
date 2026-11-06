package app

import (
	"fmt"
	"runtime"
	"strconv"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

// revealLabel is what the action that shows a file in the system's file manager is called.
func revealLabel() string {
	switch runtime.GOOS {
	case "darwin":
		return "Show in Finder"
	case "windows":
		return "Show in File Explorer"
	}
	return "Show in File Manager"
}

// iconButton is a square button showing glyph, named label for the tooltip and assistive technology.
func iconButton(c *ui.Context, glyph theme.Icon, label string) ui.Element {
	return theme.IconButton(c, glyph, label)
}

// sidebarRow is a row of the sidebar: 28 DIP, rounded, tinted when chosen or hovered.
func sidebarRow(row ui.Element, t *ui.Theme, on bool) ui.Element {
	row.Height(28).Radius(6).AlignItems(ui.Center)
	switch {
	case on:
		row.Background(theme.Selected(t))
	case row.Hovered():
		row.Background(theme.Hover(t))
	}
	return row
}

// navRow is a row of the sidebar's nav or footer: an accent glyph, as Finder's, and a label.
func navRow(c *ui.Context, glyph theme.Icon, label string, on bool) ui.Element {
	t := c.Theme()
	row := sidebarRow(ui.ButtonBase(c).Label(label).Padding(0, 8, 0, 6).Gap(6).Justify(ui.Start), t, on)
	row.Children(func() {
		ui.Box(c).Size(20, 20).Center().Children(func() { theme.Glyph(c, glyph, 16, t.Accent) })
		ui.Text(c, label).FontSize(13).SingleLine()
	})
	return row
}

// topBar is a page's bar under the window controls; it drags the window.
// With the sidebar closed it keeps clear of the traffic lights and shows the sidebar toggle.
func (ws *Workspace) topBar(c *ui.Context, fn func()) {
	bar := c.TitleBar()
	row := ui.Row(c).Height(theme.BarHeight).Shrink(0).AlignItems(ui.Center).Gap(6).PaddingEnd(bar.Right + 12).DragWindow()
	row.BorderWidth(0, 0, 1, 0).BorderColor(theme.Hairline(c.Theme()))
	if ws.sidebar.open {
		row.PaddingStart(20)
	} else {
		row.PaddingStart(bar.Left + 8)
	}
	row.Children(func() {
		if !ws.sidebar.open {
			ws.sidebarToggle(c)
			if iconButton(c, theme.Icons.Home, "Home").Clicked() {
				ws.ShowView(host.ViewHome)
			}
			ui.Box(c).Width(6)
		}
		fn()
	})
}

// Bar is topBar for the feature packages' pages.
func (ws *Workspace) Bar(c *ui.Context, content func()) { ws.topBar(c, content) }

// pageHeader is a page's top bar with its title and the actions actions builds.
func (ws *Workspace) pageHeader(c *ui.Context, title string, actions func()) {
	ws.topBar(c, func() {
		theme.Title(c, title)
		ui.Spacer(c)
		if actions != nil {
			actions()
		}
	})
}

func (ws *Workspace) sidebarToggle(c *ui.Context) {
	label := "Hide sidebar"
	if !ws.sidebar.open {
		label = "Show sidebar"
	}
	if iconButton(c, theme.Icons.Sidebar, label).Clicked() {
		ws.sidebar.open = !ws.sidebar.open
	}
}

func (ws *Workspace) searchButton(c *ui.Context) {
	if iconButton(c, theme.Icons.Search, "Search").Clicked() {
		ws.openSearch()
	}
}

// emptyState fills a page that has nothing to show.
func emptyState(c *ui.Context, glyph theme.Icon, title, text string, more func()) {
	theme.EmptyState(c, glyph, title, text, more)
}

// formatVisit is a time as the lists show it: "just now", "5 min ago",
// "Today 14:05", "03-14 09:30", or a date in another year.
func formatVisit(secs, now int64) string {
	ago := max(now-secs, 0)
	switch {
	case ago < 60:
		return "just now"
	case ago < 3600:
		return fmt.Sprintf("%d min ago", ago/60)
	}
	y, mo, d, h, mi := vault.Civil(secs)
	for back, name := range []string{"Today", "Yesterday", "Two days ago"} {
		by, bmo, bd, _, _ := vault.Civil(now - int64(back)*vault.Day)
		if by == y && bmo == mo && bd == d {
			return fmt.Sprintf("%s %02d:%02d", name, h, mi)
		}
	}
	if ny, _, _, _, _ := vault.Civil(now); ny == y {
		return fmt.Sprintf("%02d-%02d %02d:%02d", mo, d, h, mi)
	}
	return fmt.Sprintf("%d-%02d-%02d", y, mo, d)
}

// formatCount groups a count's digits by thousands.
func formatCount(n int) string {
	digits := strconv.Itoa(n)
	out := make([]byte, 0, len(digits)+len(digits)/3)
	for i := range len(digits) {
		if i > 0 && (len(digits)-i)%3 == 0 {
			out = append(out, ',')
		}
		out = append(out, digits[i])
	}
	return string(out)
}
