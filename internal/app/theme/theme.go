// Package theme holds what the app's views share beyond MyGo's theme: the icon
// set (Lucide, embedded from icons/), the monospace font, and a few colors
// MyGo's theme has no name for. Everything else comes from c.Theme().
package theme

import (
	"embed"

	"github.com/egoist/mygo/ui"
)

//go:embed icons/*.svg
var files embed.FS

func icon(name string) *ui.SVG {
	data, err := files.ReadFile("icons/" + name + ".svg")
	if err != nil {
		panic(err)
	}
	return ui.MustParseSVG(data)
}

// Mono names the system's monospaced font for ui.Font and editor.State.Font.
const Mono = "monospace"

// Icons are drawn in the text color at the font size, as ui.Icon does.
var Icons = struct {
	Home, HomeFilled, Library, Paper, Doc, Gear, Folder, FolderNew       *ui.SVG
	ChevronRight, ChevronDown, ChevronUp, Plus, Ellipsis, Search, Clear  *ui.SVG
	Delete, Edit, Move, Copy, Pin, Unpin, Star, Unstar, Export, Import   *ui.SVG
	Restore, Quote, Notes, Tag, Check, Minus, Close, Show, Highlight     *ui.SVG
	External, Panel, Back, Palette, Storage, Info, Select, Expand, Sidebar *ui.SVG
	Dot                                                                  *ui.SVG
}{
	Home: icon("home"), HomeFilled: icon("home-filled"), Library: icon("library"), Paper: icon("doc"), Doc: icon("doc"),
	Gear: icon("settings-gear"), Folder: icon("folder"), FolderNew: icon("folder-plus"),
	ChevronRight: icon("chevron-right"), ChevronDown: icon("chevron-down"), ChevronUp: icon("chevron-up"),
	Plus: icon("plus"), Ellipsis: icon("ellipsis"), Search: icon("search"), Clear: icon("search-clear"),
	Delete: icon("bin"), Edit: icon("pen-line"), Move: icon("folder-input"), Copy: icon("copy"),
	Pin: icon("pin"), Unpin: icon("pin-off"), Star: icon("star"), Unstar: icon("star-off"),
	Export: icon("export"), Import: icon("import"), Restore: icon("restore"), Quote: icon("quote"),
	Notes: icon("square-pen"), Tag: icon("tag"), Check: icon("check"), Minus: icon("minus"), Close: icon("x"),
	Show: icon("eye"), Highlight: icon("square-pen"), External: icon("export"), Panel: icon("panel-right"),
	Back: icon("arrow-left"), Palette: icon("palette"), Storage: icon("storage"), Info: icon("info"),
	Select: icon("batch"), Expand: icon("expand"), Sidebar: icon("panel-left-close"), Dot: icon("circle-filled"),
}

// Caption is secondary text: dates, meta, captions of controls.
func Caption(t *ui.Theme) ui.Color { return t.TextMuted }

// Danger colors destructive actions and errors.
func Danger(t *ui.Theme) ui.Color { return t.Danger }

// Hairline is one device pixel at the window's seams, as Finder's separators.
func Hairline(t *ui.Theme) ui.Color { return t.Border }

// Selected tints the chosen row of a list that does not have the focus.
func Selected(t *ui.Theme) ui.Color { return t.SurfacePressed }

// Group tints grouped rows, as settings and forms, set off by tone instead of an outline.
func Group(t *ui.Theme) ui.Color { return t.Surface }
