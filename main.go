// Thesio is Obsidian for Typst: a plain-file vault of .typ notes and papers.
package main

import (
	_ "embed"
	"log"
	"runtime"

	"github.com/cheetahbyte/thesio/internal/app"
	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

//go:embed resources/icon.png
var icon []byte

func main() {
	mygo.App.SetName("Thesio")
	app.AppIcon = icon
	app.Wire()
	store, warnings, err := vault.Open()
	if err != nil {
		log.Fatalf("Could not open the vault: %v", err)
	}
	app.ApplyAppearance(host.LoadPreferences(store.Root()).Appearance)

	var ws *app.Workspace
	open := func() {
		width, height := 1200, 800
		if size := store.Settings().WindowSize; size != nil {
			width, height = max(int(size[0]), 480), max(int(size[1]), 480)
		}
		var next *app.Workspace
		win := mygo.NewWindow(mygo.WindowOptions{
			Title:         "Thesio",
			Width:         width,
			Height:        height,
			MinWidth:      480,
			MinHeight:     480,
			StateKey:      "main",
			TitleBarStyle: mygo.TitleBarHidden,
			// Centered in the 52 DIP bars, as Finder's unified toolbar.
			TrafficLightPosition: &mygo.Point{X: theme.TrafficLights, Y: theme.TrafficLights},
			Vibrancy:             mygo.VibrancySidebar,
			Content: ui.View(func(c *ui.Context) {
				if next != nil {
					next.View(c)
				}
			}),
		})
		next = app.New(win, store, warnings)
		warnings = nil
		ws = next
		win.OnClose(func(e *mygo.CloseEvent) {
			if !next.SaveBefore(win.Destroy) {
				e.PreventDefault()
			}
		})
		win.OnClosed(func() {
			if ws == next {
				ws = nil
			}
		})
		target, _ := project.Launch(store.Root())
		next.Start(target.Document)
	}

	run := func(cmd app.Command) func(*mygo.MenuItem, *mygo.Window) {
		return func(*mygo.MenuItem, *mygo.Window) {
			if ws != nil {
				ws.Run(cmd)
			}
		}
	}
	mygo.App.SetMenu(mygo.NewMenu([]*mygo.MenuItem{
		{Role: mygo.RoleAppMenu, Submenu: []*mygo.MenuItem{
			{Label: "About Thesio", Click: run(app.CmdAbout)},
			mygo.Separator(),
			{Label: "Settings…", Accelerator: "CmdOrCtrl+,", Click: run(app.CmdSettings)},
			mygo.Separator(),
			{Role: mygo.RoleServices},
			mygo.Separator(),
			{Role: mygo.RoleHide},
			{Role: mygo.RoleHideOthers},
			{Role: mygo.RoleUnhide},
			mygo.Separator(),
			{Role: mygo.RoleQuit},
		}},
		{Label: "File", Submenu: []*mygo.MenuItem{
			{Label: "New Document", Accelerator: "CmdOrCtrl+N", Click: run(app.CmdNewDocument)},
			{Label: "New Folder", Accelerator: "Shift+CmdOrCtrl+N", Click: run(app.CmdNewFolder)},
			{Label: "Insert Citation…", Accelerator: "Shift+CmdOrCtrl+C", Click: run(app.CmdInsertCitation)},
			{Label: "Import as Document…", Accelerator: "Shift+CmdOrCtrl+I", Click: run(app.CmdImport)},
			mygo.Separator(),
			{Role: mygo.RoleClose, Label: "Close Window"},
		}},
		{Role: mygo.RoleEditMenu},
		{Label: "View", Submenu: []*mygo.MenuItem{
			{Label: "Toggle Sidebar", Accelerator: `CmdOrCtrl+\`, Click: run(app.CmdToggleSidebar)},
			{Label: "Home", Accelerator: "Alt+CmdOrCtrl+Home", Click: run(app.CmdHome)},
			{Label: "Search", Accelerator: "CmdOrCtrl+J", Click: run(app.CmdSearch)},
			mygo.Separator(),
			{Role: mygo.RoleToggleFullScreen, Label: "Enter Full Screen"},
		}},
		{Role: mygo.RoleWindowMenu},
	}))
	// On macOS the app keeps running with its Dock icon, and a click on it opens a window again.
	mygo.App.OnWindowAllClosed(func() {
		if runtime.GOOS != "darwin" {
			mygo.App.Quit()
		}
	})
	mygo.App.OnActivate(func(hasVisibleWindows bool) {
		if !hasVisibleWindows && ws == nil {
			open()
		}
	})
	// The open document and notes are saved first; when that fails the
	// Save failed dialog asks, and quits after Retry or Discard changes.
	mygo.App.OnBeforeQuit(func(e *mygo.QuitEvent) {
		if ws != nil && !ws.SaveBefore(mygo.App.Quit) {
			e.PreventDefault()
		}
	})
	mygo.App.WhenReady(open)
	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
