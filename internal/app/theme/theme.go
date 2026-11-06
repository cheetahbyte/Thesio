// Package theme holds what the app's views share beyond MyGo's theme: the icon
// set (Lucide, embedded from icons/), the monospace font, and a few colors
// MyGo's theme has no name for. Everything else comes from c.Theme().
package theme

import (
	"embed"

	"github.com/egoist/mygo/plugins/sfsymbols"
	"github.com/egoist/mygo/ui"
)

//go:embed icons/*.svg
var files embed.FS

// Icon is a glyph: an SF Symbol on macOS, the Lucide SVG elsewhere.
type Icon struct {
	symbol string
	svg    *ui.SVG
}

func icon(symbol, file string) Icon {
	data, err := files.ReadFile("icons/" + file + ".svg")
	if err != nil {
		panic(err)
	}
	return Icon{symbol, ui.MustParseSVG(data)}
}

// Glyph draws i size DIPs tall in col.
func Glyph(c *ui.Context, i Icon, size float32, col ui.Color) ui.Element {
	if sfsymbols.Available() {
		return sfsymbols.Symbol(c, i.symbol).Natural().Size(size).Color(col).Fallback(i.svg).Element
	}
	return ui.Icon(c, i.svg).FontSize(size).TextColor(col)
}

// SidebarGlyph draws i as Finder's sidebar draws its symbols: 23 DIP, and a
// medium weight, since a symbol rasterized large and scaled down keeps a
// thinner stroke than one drawn at that size.
func SidebarGlyph(c *ui.Context, i Icon, col ui.Color) ui.Element {
	const size = 23
	if sfsymbols.Available() {
		return sfsymbols.Symbol(c, i.symbol).Natural().Weight(sfsymbols.Medium).Size(size).Color(col).Fallback(i.svg).Element
	}
	return ui.Icon(c, i.svg).FontSize(size).TextColor(col)
}

// Mono names the system's monospaced font for ui.Font and editor.State.Font.
const Mono = "monospace"

// Icons are the app's glyphs, drawn with Glyph.
var Icons = struct {
	Home, HomeFilled, Library, Paper, Doc, Gear, Folder, FolderNew         Icon
	ChevronRight, ChevronDown, ChevronUp, Plus, Ellipsis, Search, Clear    Icon
	Delete, Edit, Move, Copy, Pin, Unpin, Star, Unstar, Export, Import     Icon
	Restore, Quote, Notes, Tag, Check, Minus, Close, Show, Highlight       Icon
	External, Panel, Back, Palette, Storage, Info, Select, Expand, Sidebar Icon
	Dot                                                                    Icon
}{
	Home: icon("house", "home"), HomeFilled: icon("house.fill", "home-filled"), Library: icon("books.vertical", "library"),
	Paper: icon("doc.text", "doc"), Doc: icon("doc.text", "doc"),
	Gear: icon("gearshape", "settings-gear"), Folder: icon("folder", "folder"), FolderNew: icon("folder.badge.plus", "folder-plus"),
	ChevronRight: icon("chevron.right", "chevron-right"), ChevronDown: icon("chevron.down", "chevron-down"), ChevronUp: icon("chevron.up", "chevron-up"),
	Plus: icon("plus", "plus"), Ellipsis: icon("ellipsis", "ellipsis"), Search: icon("magnifyingglass", "search"), Clear: icon("xmark.circle.fill", "search-clear"),
	Delete: icon("trash", "bin"), Edit: icon("pencil", "pen-line"), Move: icon("arrowshape.turn.up.right", "folder-input"), Copy: icon("doc.on.doc", "copy"),
	Pin: icon("pin", "pin"), Unpin: icon("pin.slash", "pin-off"), Star: icon("star", "star"), Unstar: icon("star.slash", "star-off"),
	Export: icon("square.and.arrow.up", "export"), Import: icon("square.and.arrow.down", "import"), Restore: icon("clock.arrow.circlepath", "restore"),
	Quote: icon("quote.opening", "quote"), Notes: icon("square.and.pencil", "square-pen"), Tag: icon("tag", "tag"), Check: icon("checkmark", "check"),
	Minus: icon("minus", "minus"), Close: icon("xmark", "x"), Show: icon("eye", "eye"), Highlight: icon("highlighter", "square-pen"),
	External: icon("arrow.up.forward.app", "export"), Panel: icon("sidebar.trailing", "panel-right"), Back: icon("arrow.left", "arrow-left"),
	Palette: icon("paintpalette", "palette"), Storage: icon("archivebox", "storage"), Info: icon("info.circle", "info"),
	Select: icon("checklist", "batch"), Expand: icon("arrow.up.left.and.arrow.down.right", "expand"), Sidebar: icon("sidebar.leading", "panel-left-close"),
	Dot: icon("circle.fill", "circle-filled"),
}

// BarHeight is the height of every top bar, the traffic lights centered in it.
const BarHeight = 52

// TableInset keeps a table off the pane's edges, so its cells' text lines up with the bar's title.
const TableInset = 10

// TrafficLights is where the close button's corner sits so that the lights
// center in a BarHeight bar, as in Finder's unified toolbar.
const TrafficLights = 18

// Caption is secondary text: dates, meta, captions of controls.
func Caption(t *ui.Theme) ui.Color { return t.TextMuted }

// Danger colors destructive actions and errors.
func Danger(t *ui.Theme) ui.Color { return t.Danger }

// ink is the text color at alpha: translucent, so it reads over the sidebar material too.
func ink(t *ui.Theme, light, dark float32) ui.Color {
	if t.Dark {
		return ui.RGBA(255, 255, 255, dark)
	}
	return ui.RGBA(0, 0, 0, light)
}

// Hairline is one line at the window's seams, as Finder's separators.
func Hairline(t *ui.Theme) ui.Color { return ink(t, 0.1, 0.1) }

// Hover tints a row or a toolbar button under the pointer.
func Hover(t *ui.Theme) ui.Color { return ink(t, 0.05, 0.07) }

// Selected tints the chosen row of a list that does not have the focus.
func Selected(t *ui.Theme) ui.Color { return ink(t, 0.09, 0.12) }

// Group tints grouped rows, as settings and forms, set off by tone instead of an outline.
func Group(t *ui.Theme) ui.Color { return t.Surface }

// Ring outlines an image or a raised panel at 10%, so its edge holds on any background.
func Ring(t *ui.Theme) ui.Color { return ink(t, 0.1, 0.1) }

// Elevate lifts a panel (a dialog, the search panel) with a ring and two soft layers.
func Elevate(e ui.Element, t *ui.Theme) ui.Element {
	return e.Shadow(0, 0, 0, 1, Ring(t)).Shadow(0, 2, 6, 0, ui.RGBA(0, 0, 0, 0.06)).Shadow(0, 12, 32, 0, ui.RGBA(0, 0, 0, 0.14))
}

// Dialog is a modal's panel as every dialog shows it.
func Dialog(panel ui.Element, t *ui.Theme) ui.Element { return Elevate(panel.Radius(14), t) }

// Tabular sets digits in equal widths, for counts and dates that change or line up.
func Tabular(e ui.Element) ui.Element { return e.FontFeatures("tnum") }

// IconButton is a 28 DIP square toolbar button showing glyph, named label for
// the tooltip and assistive technology.
func IconButton(c *ui.Context, glyph Icon, label string) ui.Element {
	t := c.Theme()
	b := ui.ButtonBase(c).Label(label).Tooltip(label).Size(28, 28).Shrink(0).Center().Radius(6).TextColor(t.TextMuted)
	if b.Hovered() {
		b.Background(Hover(t))
	}
	b.Children(func() { Glyph(c, glyph, 17, t.TextMuted) })
	return b
}

// Title is a bar's title: 15 DIP semibold, one line.
func Title(c *ui.Context, text string) ui.Element {
	return ui.Text(c, text).FontSize(15).FontWeight(600).SingleLine().Ellipsis("…").Shrink(1)
}

// Count is the muted count beside a bar's title.
func Count(c *ui.Context, n string) ui.Element {
	return Tabular(ui.Text(c, n).FontSize(15).TextColor(c.Theme().TextMuted).SingleLine())
}

// EmptyState fills a page that has nothing to show, more under the text.
func EmptyState(c *ui.Context, glyph Icon, title, text string, more func()) {
	t := c.Theme()
	ui.Column(c).Grow(1).Center().Gap(4).Padding(24, 32, 64).Children(func() {
		Glyph(c, glyph, 32, t.TextMuted).Margin(0, 0, 8, 0)
		ui.Text(c, title).FontSize(15).FontWeight(600)
		ui.Text(c, text).FontSize(13).TextColor(Caption(t)).TextAlign(ui.Center)
		if more != nil {
			more()
		}
	})
}

// AddButton is a bar's primary action: a plus and label in an accent capsule, as macOS 26's toolbars.
func AddButton(c *ui.Context, label string) ui.Element {
	t := c.Theme()
	return ui.PrimaryButton(c, "").Label(label).Height(28).Padding(0, 12, 0, 10).Gap(5).Radius(14).Children(func() {
		Glyph(c, Icons.Plus, 12, t.AccentText)
		ui.Text(c, label).FontSize(13).FontWeight(500).TextColor(t.AccentText).SingleLine()
	})
}
