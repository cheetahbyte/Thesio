package app

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// AppIcon is the app's icon as PNG, for the About page; main sets it.
var AppIcon []byte

type settingsPage int

const (
	pageAppearance settingsPage = iota
	pageDocuments
	pageAbout
)

var pageTitles = [...]string{"Appearance", "Documents", "About"}

type settingsState struct {
	page  settingsPage
	email *string
}

// OpenSettings shows a page of Settings: "about" for About, else Appearance.
func (ws *Workspace) OpenSettings(page string) {
	ws.settings.page = pageAppearance
	if page == "about" {
		ws.settings.page = pageAbout
	}
	if ws.view != host.ViewSettings {
		ws.ShowView(host.ViewSettings)
	}
	ws.Invalidate()
}

// closeSettings goes back to where Settings was opened from.
func (ws *Workspace) closeSettings() {
	if ws.doc != nil {
		ws.ShowView(host.ViewDoc)
	} else {
		ws.ShowView(host.ViewHome)
	}
}

// settingsSidebar is the sidebar of Settings and the Trash.
func (ws *Workspace) settingsSidebar(c *ui.Context) {
	t := c.Theme()
	sidebarColumn(c, func() {
		bar := c.TitleBar()
		ui.Row(c).Height(theme.BarHeight).Shrink(0).AlignItems(ui.Center).Gap(2).PaddingStart(bar.Left + 4).DragWindow().Children(func() {
			ws.sidebarToggle(c)
			if iconButton(c, theme.Icons.Home, "Home").Clicked() {
				ws.after(func() { ws.ShowView(host.ViewHome) })
			}
		})
		ui.Column(c).Padding(4, sidebarInset, 0, sidebarInset).Children(func() {
			ui.Text(c, "Settings").FontSize(11).FontWeight(600).TextColor(t.TextMuted).Padding(12, 8, 6, 8)
			nav := func(page settingsPage, glyph theme.Icon) {
				if navRow(c, glyph, pageTitles[page], ws.view == host.ViewSettings && ws.settings.page == page).Clicked() {
					ws.settings.page = page
					ws.after(func() { ws.ShowView(host.ViewSettings) })
				}
			}
			nav(pageAppearance, theme.Icons.Palette)
			nav(pageDocuments, theme.Icons.Storage)
			if navRow(c, theme.Icons.Delete, "Trash", ws.view == host.ViewTrash).Clicked() {
				ws.after(func() { ws.ShowView(host.ViewTrash) })
			}
			nav(pageAbout, theme.Icons.Info)
		})
		ui.Spacer(c)
		ui.Box(c).Height(1).MarginX(sidebarInset + 6).Background(theme.Hairline(t))
		ui.Column(c).Padding(6, sidebarInset, 8, sidebarInset).Children(func() {
			if navRow(c, theme.Icons.Back, "Back", false).Tooltip("Back (Esc)").Clicked() {
				ws.after(ws.closeSettings)
			}
		})
	})
}

func (ws *Workspace) settingsView(c *ui.Context) {
	ws.pageHeader(c, pageTitles[ws.settings.page], nil)
	ui.Scroll(c.Key("settings-content")).Grow(1).Children(func() {
		ui.Column(c).FillWidth().MaxWidth(640).Margin(28, ui.Auto, 64, ui.Auto).PaddingX(32).Gap(28).Children(func() {
			switch ws.settings.page {
			case pageAppearance:
				ws.appearancePage(c)
			case pageDocuments:
				ws.documentsPage(c)
			case pageAbout:
				ws.aboutPage(c)
			}
		})
	})
}

// group is settings rows in one rounded card, split by hairlines.
func group(c *ui.Context, rows ...func()) {
	t := c.Theme()
	ui.Column(c).Radius(10).Background(theme.Group(t)).Clip().Gap(1).Dividers(1, theme.Hairline(t)).Children(func() {
		for _, row := range rows {
			row()
		}
	})
}

// settingRow is a setting: its title and description on the left, the control on the right.
func settingRow(c *ui.Context, title, desc string, control func()) func() {
	return func() {
		t := c.Theme()
		ui.Row(c).MinHeight(44).Padding(8, 14).Gap(24).AlignItems(ui.Center).Children(func() {
			ui.Column(c).Grow(1).Basis(0).Gap(2).Children(func() {
				ui.Text(c, title).FontSize(13)
				if desc != "" {
					ui.Text(c, desc).FontSize(11).TextColor(theme.Caption(t))
				}
			})
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(control)
		})
	}
}

func (ws *Workspace) appearancePage(c *ui.Context) {
	appearances := []string{"", "light", "dark"}
	at := 0
	for i, a := range appearances {
		if ws.prefs.Appearance == a {
			at = i
		}
	}
	group(c, settingRow(c, "Appearance", "", func() {
		if ui.Segmented(c, &at, "System", "Light", "Dark").Label("Appearance").Changed() {
			ws.prefs.Appearance = appearances[at]
			ws.SavePrefs()
			ApplyAppearance(ws.prefs.Appearance)
		}
	}))
}

// vaultLocation is the vault's folder as the user reads it, under ~ where it is.
func (ws *Workspace) vaultLocation() string {
	root := ws.store.Root()
	if home, err := os.UserHomeDir(); err == nil && runtime.GOOS != "windows" {
		if rel, err := filepath.Rel(home, root); err == nil && !strings.HasPrefix(rel, "..") {
			return "~/" + filepath.ToSlash(rel)
		}
	}
	return root
}

func (ws *Workspace) documentsPage(c *ui.Context) {
	if ws.settings.email == nil {
		email := ws.prefs.UnpaywallEmail
		ws.settings.email = &email
	}
	group(c,
		settingRow(c, "Vault folder", ws.vaultLocation(), func() {
			if ui.Button(c, revealLabel()).Clicked() {
				ws.Reveal(ws.store.Root())
			}
		}),
		settingRow(c, "Hide library files", "Keeps "+vault.LibraryFile+" and the "+library.PapersDir+" folder out of the sidebar. They stay in the vault.", func() {
			if ui.Switch(c, &ws.prefs.HideLibraryFiles).Label("Hide library files").Changed() {
				ws.SavePrefs()
			}
		}),
		settingRow(c, "Unpaywall email", "Finds open-access PDFs for sources with a DOI. Your email and each DOI are sent to Unpaywall.", func() {
			if ui.TextInput(c, ws.settings.email).Placeholder("name@university.edu").Label("Unpaywall email").Width(220).Changed() {
				ws.prefs.UnpaywallEmail = strings.TrimSpace(*ws.settings.email)
				ws.SavePrefs()
			}
		}),
		settingRow(c, "Trash", fmt.Sprintf("Deleted documents are kept for %d days.", vault.TrashDays), func() {
			if ui.Button(c, "Open").Clicked() {
				ws.after(func() { ws.ShowView(host.ViewTrash) })
			}
		}),
	)
}

func (ws *Workspace) aboutPage(c *ui.Context) {
	t := c.Theme()
	if ws.icon == nil && len(AppIcon) > 0 {
		// A broken icon only leaves the About page without it.
		if icon, err := ui.DecodeImage(AppIcon); err == nil {
			ws.icon = icon
		}
	}
	version := mygo.App.Version()
	if version == "" {
		version = library.AppVersion
	}
	ui.Column(c).Padding(24, 0, 0, 0).Gap(10).AlignItems(ui.Center).Children(func() {
		if ws.icon != nil {
			ui.Image(c, ws.icon).Size(88, 88)
		}
		ui.Text(c, "Thesio").FontSize(20).FontWeight(600)
		ui.Text(c, "Version "+version).FontSize(13).TextColor(theme.Caption(t))
		ui.Text(c, "A local-first workspace for academic research and writing.").FontSize(13).TextColor(theme.Caption(t))
	})
}
