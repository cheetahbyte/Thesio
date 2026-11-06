package app

import (
	"fmt"
	"slices"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

const (
	sidebarDefault = 280
	sidebarMin     = 240
	sidebarMax     = 800
	// contentMin is the room the main pane keeps when the sidebar is dragged wider.
	contentMin = 540
	batchLimit = 50
	widthSave  = 500 * time.Millisecond
	outlineLag = 400 * time.Millisecond
	// sidebarInset is the room between the sidebar's edges and its rows, as Finder's.
	sidebarInset = 10
	// treeIndent is a level of nesting: a folder's glyph and the gap after it,
	// so that what a folder holds starts under its title.
	treeIndent = 24
)

// section is one of the sidebar's lists, each folded on its own.
type section int

const (
	secPinned section = iota
	secOutline
	secLinkedFrom
	secFiles
)

var sectionTitles = [...]string{"Pinned", "Outline", "Linked From", "Files"}

type sidebarState struct {
	open       bool
	width      float32
	widthDirty bool
	widthAt    time.Time
	// expanded are the open folders of Files, kept in the vault's settings.
	expanded map[string]bool
	// selecting is selection mode; batch are the checked rows, in check order.
	selecting bool
	batch     []string
	folded    [4]bool
	// renaming is the row whose title is being edited.
	renaming    string
	renameText  string
	renameFresh bool
	renameFocus bool
}

func newSidebarState(settings vault.Settings) sidebarState {
	s := sidebarState{open: true, width: sidebarDefault, expanded: map[string]bool{}}
	if settings.SidebarWidth != nil {
		s.width = max(*settings.SidebarWidth, sidebarMin)
	}
	for _, id := range settings.Expanded {
		s.expanded[id] = true
	}
	return s
}

// outlineState is what the sidebar shows about the open document.
type outlineState struct {
	headings []project.Heading
	// active is the heading the caret is under, -1 for none.
	active    int
	backlinks []backlink
	stale     bool
	changedAt time.Time
}

type backlink struct {
	id, title string
	offset    int
}

// dragRow is a Files row being dragged onto a folder.
type dragRow struct{ id string }

// refreshOutline reads the outline again: the open file from its editor, others from memory.
func (ws *Workspace) refreshOutline() {
	ws.outline.stale = false
	ws.outline.headings, ws.outline.backlinks = nil, nil
	if ws.doc == nil || !vault.IsDocument(ws.doc.ID()) {
		ws.outline.active = -1
		return
	}
	current := ws.doc.ID()
	for _, e := range ws.entries {
		if !e.Typst || e.ID == current {
			continue
		}
		for _, link := range typst.NoteLinks(e.Body, e.ID) {
			if link.Target == current {
				ws.outline.backlinks = append(ws.outline.backlinks, backlink{e.ID, e.Title, link.Start})
				break
			}
		}
	}
	ws.outline.headings = project.Open(ws.TopDocument(current), ws.read).Outline()
	ws.markOutline(ws.doc.Caret())
}

// markOutline marks the heading the caret is under.
func (ws *Workspace) markOutline(caret int) {
	active := -1
	current := ws.Current()
	for i, h := range ws.outline.headings {
		if h.Path == current && h.Offset <= caret {
			active = i
		}
	}
	if active != ws.outline.active {
		ws.outline.active = active
		ws.Invalidate()
	}
}

// listedEntries is what Files shows and Select all takes: a hidden file must
// not be caught by a batch delete or move the user cannot see.
func (ws *Workspace) listedEntries() []vault.Entry {
	if !ws.prefs.HideLibraryFiles {
		return ws.entries
	}
	// A sources folder holding anything else is the user's own.
	papersOnly := true
	for _, e := range ws.entries {
		if strings.HasPrefix(e.ID, library.PapersDir+"/") && !e.Folder && !vault.IsPDF(e.ID) {
			papersOnly = false
		}
	}
	var out []vault.Entry
	for _, e := range ws.entries {
		if e.ID == vault.LibraryFile || papersOnly && vault.IsWithin(e.ID, library.PapersDir) {
			continue
		}
		out = append(out, e)
	}
	return out
}

// tree is entries as the sidebar nests them: folders first, then names in natural order.
type tree struct {
	roots []vault.Entry
	kids  map[string][]vault.Entry
}

func newTree(entries []vault.Entry) tree {
	ids := make(map[string]bool, len(entries))
	for _, e := range entries {
		ids[e.ID] = true
	}
	t := tree{kids: map[string][]vault.Entry{}}
	for _, e := range entries {
		if e.Parent != "" && ids[e.Parent] {
			t.kids[e.Parent] = append(t.kids[e.Parent], e)
		} else {
			t.roots = append(t.roots, e)
		}
	}
	order := func(a, b vault.Entry) int {
		if a.Folder != b.Folder {
			if a.Folder {
				return -1
			}
			return 1
		}
		return vault.NaturalCompare(a.Title, b.Title)
	}
	slices.SortStableFunc(t.roots, order)
	for _, v := range t.kids {
		slices.SortStableFunc(v, order)
	}
	return t
}

func (t tree) children(id string) []vault.Entry {
	if id == "" {
		return t.roots
	}
	return t.kids[id]
}

type treeRow struct {
	entry vault.Entry
	depth int
}

// rows are the entries in pre-order, descending into the folders open says yes to.
func (t tree) rows(open func(id string) bool) []treeRow {
	var out []treeRow
	var walk func(parent string, depth int)
	walk = func(parent string, depth int) {
		for _, e := range t.children(parent) {
			out = append(out, treeRow{e, depth})
			if e.Folder && open(e.ID) {
				walk(e.ID, depth+1)
			}
		}
	}
	walk("", 0)
	return out
}

// subtree is id and every file under it.
func (t tree) subtree(id string) []string {
	out := []string{id}
	for i := 0; i < len(out); i++ {
		for _, e := range t.kids[out[i]] {
			out = append(out, e.ID)
		}
	}
	return out
}

// ancestors are id's folders, the top level first.
func (ws *Workspace) ancestors(id string) []vault.Entry {
	var out []vault.Entry
	seen := map[string]bool{id: true}
	for e, ok := ws.Entry(id); ok && e.Parent != ""; {
		if seen[e.Parent] {
			break
		}
		seen[e.Parent] = true
		if e, ok = ws.Entry(e.Parent); ok {
			out = append(out, e)
		}
	}
	slices.Reverse(out)
	return out
}

// revealInTree opens the folders down to id.
func (ws *Workspace) revealInTree(id string) {
	for _, e := range ws.ancestors(id) {
		ws.setExpanded(e.ID, true)
	}
}

// setExpanded opens or closes a folder and remembers it for the next launch.
func (ws *Workspace) setExpanded(id string, open bool) {
	if ws.sidebar.expanded[id] == open {
		return
	}
	if open {
		ws.sidebar.expanded[id] = true
	} else {
		delete(ws.sidebar.expanded, id)
	}
	ws.saveExpanded()
}

func (ws *Workspace) toggleFolder(id string) { ws.setExpanded(id, !ws.sidebar.expanded[id]) }

func (ws *Workspace) saveExpanded() {
	expanded := make([]string, 0, len(ws.sidebar.expanded))
	for id := range ws.sidebar.expanded {
		expanded = append(expanded, id)
	}
	slices.Sort(expanded)
	if err := ws.store.UpdateSettings(func(s *vault.Settings) { s.Expanded = expanded }); err != nil {
		ws.ToastError(fmt.Sprintf("Could not remember open folders: %v", err))
	}
}

// saveWidth remembers the dragged sidebar width once dragging has paused.
func (ws *Workspace) saveWidth(c *ui.Context) {
	if !ws.sidebar.widthDirty {
		return
	}
	if wait := widthSave - c.Now().Sub(ws.sidebar.widthAt); wait > 0 {
		c.After(wait)
		return
	}
	ws.sidebar.widthDirty = false
	width := ws.sidebar.width
	if err := ws.store.UpdateSettings(func(s *vault.Settings) { s.SidebarWidth = &width }); err != nil {
		ws.ToastError(fmt.Sprintf("Could not remember the sidebar width: %v", err))
	}
}

// startRename turns a row's title into a field; Enter or leaving it saves, Esc cancels.
func (ws *Workspace) startRename(id string) {
	e, ok := ws.Entry(id)
	if !ok {
		return
	}
	ws.sidebar.renaming, ws.sidebar.renameText = id, strings.TrimSpace(e.Title)
	ws.sidebar.renameFresh, ws.sidebar.renameFocus = true, false
	// From Home the row may be hidden: show it in the sidebar.
	ws.sidebar.open = true
	ws.sidebar.folded[secFiles] = false
	if ws.view == host.ViewSettings || ws.view == host.ViewTrash {
		ws.setView(host.ViewHome)
	}
	ws.revealInTree(id)
	ws.Invalidate()
}

func (ws *Workspace) finishRename(commit bool) {
	id, title := ws.sidebar.renaming, ws.sidebar.renameText
	if id == "" {
		return
	}
	ws.sidebar.renaming = ""
	if commit {
		ws.rename(id, title)
	}
	ws.Invalidate()
}

// toggleCheck checks or unchecks a row of Files, entering selection mode; false when the limit blocks it.
func (ws *Workspace) toggleCheck(id string) bool {
	ws.sidebar.folded[secFiles] = false
	ws.sidebar.selecting = true
	if i := slices.Index(ws.sidebar.batch, id); i >= 0 {
		ws.sidebar.batch = slices.Delete(ws.sidebar.batch, i, i+1)
		return true
	}
	if len(ws.sidebar.batch) >= batchLimit {
		return false
	}
	ws.sidebar.batch = append(ws.sidebar.batch, id)
	return true
}

// checked are the checked rows, in list order.
func (ws *Workspace) checked() []string {
	var out []string
	for _, e := range ws.entries {
		if slices.Contains(ws.sidebar.batch, e.ID) {
			out = append(out, e.ID)
		}
	}
	return out
}

func (ws *Workspace) exitSelection() { ws.sidebar.selecting, ws.sidebar.batch = false, nil }

func (ws *Workspace) batchCopy() {
	for _, id := range ws.checked() {
		if vault.IsDocument(id) {
			ws.copyDoc(id)
		}
	}
	ws.RefreshEntries()
	ws.exitSelection()
}

func (ws *Workspace) batchMove() {
	if ids := ws.checked(); len(ids) > 0 {
		ws.openMove(ids)
	}
}

func (ws *Workspace) batchDelete() { ws.askDelete(ws.checked(), true) }

// sidebarView is the left pane: the nav, the outline, the files, and selection mode.
func (ws *Workspace) sidebarView(c *ui.Context) {
	if ws.view == host.ViewSettings || ws.view == host.ViewTrash {
		ws.settingsSidebar(c)
		return
	}
	t := c.Theme()
	if ws.outline.stale {
		if wait := outlineLag - time.Since(ws.outline.changedAt); wait > 0 {
			c.After(wait)
		} else {
			ws.refreshOutline()
		}
	}
	sidebarColumn(c, func() {
		bar := c.TitleBar()
		ui.Row(c).Height(theme.BarHeight).Shrink(0).AlignItems(ui.Center).PaddingStart(bar.Left + 4).PaddingEnd(12).DragWindow().Children(func() {
			ws.sidebarToggle(c)
		})
		ui.Column(c).Padding(4, sidebarInset, 4, sidebarInset).Children(func() {
			home := theme.Icons.Home
			if ws.view == host.ViewHome {
				home = theme.Icons.HomeFilled
			}
			if navRow(c, home, "Home", ws.view == host.ViewHome).Clicked() {
				ws.ShowView(host.ViewHome)
			}
			if navRow(c, theme.Icons.Library, "Library", ws.view == host.ViewLibrary).Clicked() {
				ws.ShowView(host.ViewLibrary)
			}
			if navRow(c, theme.Icons.Paper, "Sources", ws.view == host.ViewSources || ws.view == host.ViewPaper).Clicked() {
				ws.ShowView(host.ViewSources)
			}
		})
		list := ui.Scroll(c.Key("doc-list")).Grow(1).Padding(0, sidebarInset, 8, sidebarInset)
		if row, ok := ui.Drop[dragRow](list); ok {
			// A drop that no row takes lands in the vault root.
			ws.after(func() { ws.moveDocs([]string{row.id}, "") })
		}
		if _, ok := ui.DragOver[dragRow](list); ok {
			list.Background(theme.Hover(t))
		}
		list.PopoverMenu(ws.filesMenu)
		list.Children(func() { ws.sidebarLists(c) })
		ws.sidebarFooter(c)
	})
}

// sidebarColumn is the sidebar's column: over the window's material where it shows, else on a gray.
func sidebarColumn(c *ui.Context, fn func()) {
	col := ui.Column(c).Fill()
	if !c.Vibrancy() {
		col.Background(theme.Group(c.Theme()))
	}
	col.Children(fn)
}

func (ws *Workspace) sidebarLists(c *ui.Context) {
	entries := ws.listedEntries()
	var pinned []vault.Entry
	for _, e := range entries {
		if e.Meta.Pinned {
			pinned = append(pinned, e)
		}
	}
	if len(pinned) > 0 {
		ws.sectionHeader(c, secPinned)
		if !ws.sidebar.folded[secPinned] {
			for _, e := range pinned {
				ws.docRow(c.Key("pinned:"+e.ID), e, 0, false)
			}
		}
		ui.Box(c).Height(10)
	}
	ws.sectionHeader(c, secFiles)
	if !ws.sidebar.folded[secFiles] {
		for _, r := range newTree(entries).rows(func(id string) bool { return ws.sidebar.expanded[id] }) {
			ws.docRow(c.Key("doc:"+r.entry.ID), r.entry, r.depth, true)
		}
	}
	ui.Box(c).Height(10)
	if ws.view == host.ViewDoc && len(ws.outline.headings) > 0 {
		ws.sectionHeader(c, secOutline)
		if !ws.sidebar.folded[secOutline] {
			top := ws.outline.headings[0].Level
			for _, h := range ws.outline.headings {
				top = min(top, h.Level)
			}
			for i, h := range ws.outline.headings {
				if jumpRow(c.Key(fmt.Sprint("outline", i)), h.Title, float32(treeIndent*(h.Level-top)), i == ws.outline.active).Clicked() {
					path, offset := h.Path, h.Offset
					ws.after(func() { ws.GoToSource(path, offset) })
				}
			}
		}
		ui.Box(c).Height(10)
	}
	if ws.view == host.ViewDoc && len(ws.outline.backlinks) > 0 {
		ws.sectionHeader(c, secLinkedFrom)
		if !ws.sidebar.folded[secLinkedFrom] {
			for i, b := range ws.outline.backlinks {
				if jumpRow(c.Key(fmt.Sprint("backlink", i)), vault.DisplayTitle(b.title), 0, false).Clicked() {
					ws.after(func() { ws.GoToSource(b.id, b.offset) })
				}
			}
		}
		ui.Box(c).Height(10)
	}
}

// jumpRow is a row that goes somewhere in a document.
func jumpRow(c *ui.Context, title string, indent float32, active bool) ui.Element {
	t := c.Theme()
	row := sidebarRow(ui.ButtonBase(c).Label(title).Padding(0, 8, 0, 8+indent), t, false)
	row.Children(func() {
		text := ui.Text(c, title).FontSize(13).SingleLine().Ellipsis("…").Grow(1)
		if active {
			text.FontWeight(500)
		} else {
			text.TextColor(t.TextMuted)
		}
	})
	return row
}

// sectionHeader is a list's title, folding the list on a click. Files in
// selection mode shows Select all, the count and Exit instead.
func (ws *Workspace) sectionHeader(c *ui.Context, s section) {
	t := c.Theme()
	if s == secFiles && ws.sidebar.selecting {
		n := len(ws.sidebar.batch)
		all := n > 0 && n >= min(len(ws.listedEntries()), batchLimit)
		ui.Row(c).Height(34).Padding(6, 0, 0, 6).Gap(8).AlignItems(ui.Center).Children(func() {
			box := ui.CheckboxBase(c, &all).Label("Select all")
			if n > 0 && !all {
				box.Mixed()
			}
			if box.Changed() {
				ws.sidebar.batch = nil
				if all {
					for _, e := range ws.listedEntries() {
						if len(ws.sidebar.batch) == batchLimit {
							break
						}
						ws.sidebar.batch = append(ws.sidebar.batch, e.ID)
					}
				}
			}
			theme.Tabular(ui.Text(c, fmt.Sprintf("Selected: %d", n)).FontSize(13).Grow(1))
			if ui.Button(c, "Exit").Tooltip("Exit selection mode").FontSize(12).Clicked() {
				ws.exitSelection()
			}
		})
		return
	}
	row := ui.ButtonBase(c).Label(sectionTitles[s]).Height(26).Margin(6, 0, 0, 0).Padding(0, 10, 0, 8).AlignItems(ui.Center)
	row.Children(func() {
		ui.Text(c, sectionTitles[s]).FontSize(11).FontWeight(600).TextColor(t.TextMuted).Grow(1)
		chevron := theme.Icons.ChevronDown
		if ws.sidebar.folded[s] {
			chevron = theme.Icons.ChevronRight
		}
		theme.Glyph(c, chevron, 10, t.TextMuted)
	})
	if row.Clicked() {
		ws.sidebar.folded[s] = !ws.sidebar.folded[s]
	}
}

// docRow is a file or folder. Only Files (files) selects, drags, and shows folders open.
func (ws *Workspace) docRow(c *ui.Context, e vault.Entry, depth int, files bool) {
	t := c.Theme()
	selected := ws.view == host.ViewDoc && ws.Current() == e.ID
	checked := files && slices.Contains(ws.sidebar.batch, e.ID)
	renaming := files && ws.sidebar.renaming == e.ID
	row := sidebarRow(ui.Row(c), t, selected || checked).Gap(6).PaddingStart(6 + treeIndent*float32(depth)).PaddingEnd(6)
	row.Label(vault.DisplayTitle(e.Title))
	if files && !renaming && !ws.sidebar.selecting {
		// Dropping on a file means the folder it sits in.
		target := e.Parent
		if e.Folder {
			target = e.ID
		}
		row.Drag(dragRow{e.ID})
		if dragged, ok := ui.DragOver[dragRow](row); ok && dragged.id != e.ID {
			row.Background(theme.Selected(t))
		}
		if dragged, ok := ui.Drop[dragRow](row); ok {
			ws.after(func() { ws.moveDocs([]string{dragged.id}, target) })
		}
	}
	if row.RightClicked() && files && ws.sidebar.selecting && !checked {
		ws.toggleCheck(e.ID)
	}
	id := e.ID
	row.PopoverMenu(func(m *ui.Menu) {
		if files && ws.sidebar.selecting {
			ws.batchMenu(m)
		} else {
			ws.rowMenu(m, id)
		}
	})
	if row.Clicked() && !renaming {
		// In selection mode a click checks the row; Shift-click enters it.
		if files && (ws.sidebar.selecting || row.ClickModifiers()&ui.Shift != 0) {
			ws.toggleCheck(id)
		} else {
			ws.after(func() { ws.Open(id) })
		}
	}
	row.Children(func() {
		// A guide under each folder this row sits in, through the middle of its glyph.
		for d := range depth {
			ui.Box(c).Absolute().Left(6 + treeIndent*float32(d) + 10).Top(0).Bottom(0).Width(1).Background(theme.Hairline(t))
		}
		if files && ws.sidebar.selecting {
			on := checked
			box := ui.CheckboxBase(c, &on).Label("Select " + vault.DisplayTitle(e.Title))
			if !checked && len(ws.sidebar.batch) >= batchLimit {
				box.Disabled(true).Tooltip("Selection limit reached")
			}
			if box.Changed() {
				ws.toggleCheck(id)
			}
		}
		if e.Folder {
			ui.Box(c).Size(20, 20).Shrink(0).Center().Children(func() { theme.Glyph(c, theme.Icons.Folder, 16, t.TextMuted) })
		}
		if renaming {
			ws.renameField(c)
			return
		}
		title := ui.Text(c, vault.DisplayTitle(e.Title)).FontSize(13).SingleLine().Ellipsis("…").Grow(1)
		if !e.Folder {
			// Where a folder's glyph starts, so a file lines up under its folder's title.
			title.PaddingStart(2)
		}
		if files && e.Folder {
			chevron := theme.Icons.ChevronRight
			if ws.sidebar.expanded[id] {
				chevron = theme.Icons.ChevronDown
			}
			arrow := ui.ButtonBase(c).Label("Expand").Size(20, 20).Radius(4).Center().Children(func() {
				theme.Glyph(c, chevron, 10, t.TextMuted)
			})
			if arrow.Clicked() {
				ws.toggleFolder(id)
			}
		}
	})
}

func (ws *Workspace) renameField(c *ui.Context) {
	input := ui.TextInput(c.Key("rename"), &ws.sidebar.renameText).Placeholder(vault.Untitled).AutoFocus().Grow(1).FontSize(13)
	input.OnTextKey(func(k ui.TextKey) bool {
		switch k.Key {
		case ui.KeyEscape:
			ws.finishRename(false)
			return true
		case ui.KeyEnter:
			ws.finishRename(true)
			return true
		}
		return false
	})
	if ws.sidebar.renameFresh {
		input.SetTextSelection(0, utf8.RuneCountInString(ws.sidebar.renameText))
		ws.sidebar.renameFresh = false
	}
	// Clicking away saves, as Enter does.
	focused := input.Focused()
	if ws.sidebar.renameFocus && !focused {
		ws.after(func() { ws.finishRename(true) })
	}
	ws.sidebar.renameFocus = focused
}

// rowMenu is a row's menu, on a right click.
func (ws *Workspace) rowMenu(m *ui.Menu, id string) {
	e, ok := ws.Entry(id)
	if !ok {
		return
	}
	if e.Folder {
		if m.Item("New document").Chosen() {
			ws.after(func() { ws.newChild(id) })
		}
		if m.Item("New folder").Chosen() {
			ws.after(func() { ws.NewFolder(id) })
		}
		m.Separator()
		if m.Item("Rename").Chosen() {
			ws.after(func() { ws.startRename(id) })
		}
		if m.Item("Move to").Chosen() {
			ws.after(func() { ws.openMove([]string{id}) })
		}
		if m.Item(revealLabel()).Chosen() {
			ws.revealPath(id)
		}
		m.Separator()
		if m.Item("Delete").Chosen() {
			ws.after(func() { ws.askDelete([]string{id}, false) })
		}
		return
	}
	if e.Typst && m.Item("Duplicate").Chosen() {
		ws.after(func() { ws.duplicateDoc(id) })
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
	m.Separator()
	if e.Typst && m.Item("Export as PDF").Chosen() {
		ws.after(func() { ws.exportPDF(id) })
	}
	if m.Item("Rename").Chosen() {
		ws.after(func() { ws.startRename(id) })
	}
	if m.Item(revealLabel()).Chosen() {
		ws.revealPath(id)
	}
	m.Separator()
	if m.Item("Delete").Chosen() {
		ws.after(func() { ws.askDelete([]string{id}, false) })
	}
}

// batchMenu is the menu of the checked rows in selection mode.
func (ws *Workspace) batchMenu(m *ui.Menu) {
	m.Item(fmt.Sprintf("Selected: %d", len(ws.sidebar.batch))).Disabled(true)
	if m.Item("Duplicate").Chosen() {
		ws.after(ws.batchCopy)
	}
	if m.Item("Move to").Chosen() {
		ws.after(ws.batchMove)
	}
	if m.Item("Delete").Chosen() {
		ws.after(ws.batchDelete)
	}
}

// filesMenu is the list's own menu: everything that adds to it.
func (ws *Workspace) filesMenu(m *ui.Menu) {
	if m.Item("New document").Chosen() {
		ws.after(func() { ws.NewDocument("") })
	}
	if m.Item("New folder").Chosen() {
		ws.after(func() { ws.NewFolder("") })
	}
	if m.Item("Select documents").Chosen() {
		ws.sidebar.selecting, ws.sidebar.batch = true, nil
		ws.sidebar.folded[secFiles] = false
	}
}

func (ws *Workspace) sidebarFooter(c *ui.Context) {
	t := c.Theme()
	ui.Box(c).Height(1).MarginX(sidebarInset + 6).Background(theme.Hairline(t))
	if !ws.sidebar.selecting {
		ui.Column(c).Padding(6, sidebarInset, 8, sidebarInset).Children(func() {
			if navRow(c, theme.Icons.Gear, "Settings", false).Clicked() {
				ws.ShowView(host.ViewSettings)
			}
		})
		return
	}
	some := len(ws.sidebar.batch) > 0
	ui.Row(c).Height(44).PaddingX(sidebarInset).Gap(4).AlignItems(ui.Center).Children(func() {
		if iconButton(c, theme.Icons.Copy, "Duplicate").Disabled(!some).Clicked() {
			ws.after(ws.batchCopy)
		}
		if iconButton(c, theme.Icons.Move, "Move to").Disabled(!some).Clicked() {
			ws.after(ws.batchMove)
		}
		if iconButton(c, theme.Icons.Delete, "Delete").Disabled(!some).Clicked() {
			ws.after(ws.batchDelete)
		}
	})
}
