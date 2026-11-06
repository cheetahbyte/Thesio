package app

import (
	"fmt"
	"os"
	"slices"
	"strings"
	"time"

	"github.com/cheetahbyte/thesio/internal/app/document"
	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/project"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui/editor"
)

// Open shows a document, toggles a folder, or opens a paper, saving first.
func (ws *Workspace) Open(id string) {
	paper := vault.IsPDF(id)
	if ws.view != host.ViewDoc && !paper {
		ws.setView(host.ViewDoc)
	}
	if ws.Current() == id {
		return
	}
	if e, ok := ws.Entry(id); ok && e.Folder {
		ws.toggleFolder(id)
		return
	}
	ws.WhenSaved(func() { ws.loadDocument(id) })
}

// loadDocument replaces the open document with id, which holds nothing unsaved.
func (ws *Workspace) loadDocument(id string) {
	ws.closeOverlays()
	if vault.IsPDF(id) {
		ws.papers.OpenPaper(id)
		return
	}
	text, err := ws.store.Load(id)
	if err != nil {
		// Binary files belong to the system's apps.
		if path, perr := ws.store.Path(id); perr == nil && !vault.IsDocument(id) {
			ws.OpenExternally(path)
		} else {
			ws.ToastError(fmt.Sprintf("Could not open %s: %v", id, err))
		}
		return
	}
	typ := vault.IsDocument(id)
	main := id
	if typ {
		main = ws.TopDocument(id)
	}
	if ws.doc != nil {
		ws.doc.Close()
	}
	d := document.NewOpen(ws, id, text, main, typ)
	d.OnChange = func(string) {
		ws.outline.stale, ws.outline.changedAt = true, time.Now()
		ws.Invalidate()
	}
	d.OnCaret = ws.markOutline
	d.OnSaved = ws.onSaved
	d.OnFollowLink = ws.FollowLink
	d.Citations = func(query string, labels []typst.Label) []editor.Completion {
		return ws.cites.CompletionItems(query, labels)
	}
	d.NoteLinks = ws.noteLinkItems
	d.OnCitationCompleted = func() { ws.CitationAdded(d.ID()) }
	ws.doc = d
	d.Focus()
	if err := ws.store.Touch(id); err != nil {
		ws.ToastError(fmt.Sprintf("Could not record opening %s: %v", id, err))
	} else if e := ws.entry(id); e != nil {
		e.Meta.Opened = vault.Now()
	}
	ws.KeepVersion(id, text, true)
	ws.refreshOutline()
	ws.revealInTree(id)
	ws.Invalidate()
}

// noteLinkItems is what `[[` offers from the open document, matching titles first.
func (ws *Workspace) noteLinkItems(query string) []editor.Completion {
	file := ws.Current()
	if file == "" {
		return nil
	}
	query = strings.ToLower(query)
	var first, rest []editor.Completion
	for _, e := range ws.entries {
		title := strings.ToLower(e.Title)
		if !e.Typst || e.ID == file || !strings.Contains(title, query) && !strings.Contains(strings.ToLower(e.ID), query) {
			continue
		}
		item := editor.Completion{Label: e.Title, Detail: e.ID, Kind: editor.CompletionText, Insert: noteLink(file, e.ID, e.Title), FilterText: e.Title + " " + e.ID}
		if strings.HasPrefix(title, query) {
			first = append(first, item)
		} else {
			rest = append(rest, item)
		}
	}
	return append(first, rest...)
}

// noteLink is the markup of a link from file to target.
func noteLink(file, target, title string) string {
	return "#link(" + vault.Quoted(vault.RelativeTo(vault.DirParts(file), target)) + ")[" + vault.EscapeMarkup(title) + "]"
}

// onSaved follows a write of the open document: versions, order, bibliographies, outline.
func (ws *Workspace) onSaved(id, text string) {
	ws.committed(id, text)
	ws.KeepVersion(id, text, false)
	ws.cites.SyncBibliographies()
	ws.refreshOutline()
	ws.Invalidate()
}

// committed updates id's entry after text reached its file.
func (ws *Workspace) committed(id, text string) {
	if e := ws.entry(id); e != nil {
		e.Body, e.Modified = text, vault.Now()
	}
	ws.sort()
}

// showText gives text to the editors showing id.
func (ws *Workspace) showText(id, text string, undoable bool) {
	for _, d := range ws.editorsOf(id) {
		d.SetText(text, undoable)
	}
}

// WriteDocument is the one way text reaches a document: the file, its entry,
// the editors showing it, and the preview.
func (ws *Workspace) WriteDocument(id, text string, generated bool) error {
	ws.showText(id, text, !generated)
	editors := ws.editorsOf(id)
	if err := ws.store.Save(id, text); err != nil {
		// The open document keeps the text and retries on its next save.
		for _, d := range editors {
			d.MarkUnsaved()
		}
		return err
	}
	for _, d := range editors {
		d.MarkSaved()
	}
	ws.committed(id, text)
	ws.Invalidate()
	return nil
}

// editorsOf are the open editors showing id.
func (ws *Workspace) editorsOf(id string) []*document.Open {
	var out []*document.Open
	if ws.doc != nil && ws.doc.ID() == id {
		out = append(out, ws.doc)
	}
	if notes := ws.papers.NotesEditor(id); notes != nil {
		out = append(out, notes)
	}
	return out
}

// MarkUnsaved writes the open document again after text changed without an edit.
func (ws *Workspace) MarkUnsaved() {
	if ws.doc == nil {
		return
	}
	ws.doc.MarkUnsaved()
	ws.flushDoc()
	ws.Invalidate()
}

// flushDoc writes the open document if it has unsaved changes; false when that failed.
func (ws *Workspace) flushDoc() bool {
	if ws.doc == nil || ws.doc.Flush() {
		return true
	}
	if ws.view != host.ViewDoc {
		ws.ToastError(ws.saveError())
	}
	return false
}

// SaveAll flushes the Library's panel, a paper's notes and the open document.
func (ws *Workspace) SaveAll() bool {
	ws.cites.SaveInspector()
	notes := ws.papers.FlushNotes()
	return ws.flushDoc() && notes
}

// WhenSaved runs then now if everything saves, or from the Save failed dialog.
func (ws *Workspace) WhenSaved(then func()) {
	if ws.SaveAll() {
		then()
		return
	}
	ws.askSaveFailed(then)
}

// SaveBefore saves before the window closes or the app quits. False when
// that failed: the Save failed dialog runs then after Retry or Discard changes.
func (ws *Workspace) SaveBefore(then func()) bool {
	ws.rememberWindowSize()
	if ws.SaveAll() {
		return true
	}
	ws.askSaveFailed(then)
	ws.Invalidate()
	return false
}

func (ws *Workspace) askSaveFailed(then func()) {
	ws.confirm = confirmState{
		open:   true,
		title:  "Could not save document",
		body:   ws.saveError() + ". Retry, or cancel and export a copy using More → Export as PDF. Discarding changes will lose your unsaved work.",
		action: "Retry",
		then:   func() { ws.resolveSaveFailed(true, then) },
		other:  "Discard changes",
		orElse: func() { ws.resolveSaveFailed(false, then) },
	}
}

// resolveSaveFailed retries the save or gives the changes up, then carries on.
func (ws *Workspace) resolveSaveFailed(retry bool, then func()) {
	if retry {
		if !ws.SaveAll() {
			ws.askSaveFailed(then)
			return
		}
	} else {
		ws.papers.DiscardNotes()
		if ws.doc != nil && ws.doc.Unsaved() {
			ws.doc.Discard()
		}
	}
	then()
}

// saveError is the error of the last failed save, for the Save failed dialog.
func (ws *Workspace) saveError() string {
	if ws.doc != nil {
		if err := docSaveError(ws.doc); err != "" {
			return err
		}
	}
	if err := ws.papers.NotesError(); err != "" {
		return err
	}
	return "Save failed"
}

// KeepVersion keeps text as a version of id; unless now, at most every few minutes.
func (ws *Workspace) KeepVersion(id, text string, now bool) {
	versions, err := ws.store.Versions(id)
	if err == nil {
		if now {
			_, err = versions.Keep(text)
		} else {
			_, err = versions.KeepPaced(text)
		}
	}
	if err != nil {
		ws.ToastError(fmt.Sprintf("Could not keep a version of %s: %v", id, err))
	}
}

// PathChanged follows a rename or move through everything open.
func (ws *Workspace) PathChanged(old, renamed string, rewritten []vault.Rewritten) {
	ws.papers.PathChanged(old, renamed)
	ws.RefreshEntries()
	if ws.doc != nil {
		id := ws.doc.ID()
		if moved, ok := vault.MovedPath(id, old, renamed); ok {
			id = moved
		}
		main := id
		if vault.IsDocument(id) {
			main = ws.TopDocument(id)
		}
		ws.doc.SetPath(id, main)
	}
	for i, id := range ws.sidebar.batch {
		if moved, ok := vault.MovedPath(id, old, renamed); ok {
			ws.sidebar.batch[i] = moved
		}
	}
	for id := range ws.sidebar.expanded {
		if moved, ok := vault.MovedPath(id, old, renamed); ok {
			delete(ws.sidebar.expanded, id)
			ws.sidebar.expanded[moved] = true
		}
	}
	ws.saveExpanded()
	// Their paths changed on disk: open editors take them in.
	for _, r := range rewritten {
		ws.showText(r.ID, r.Text, true)
	}
	ws.refreshOutline()
	ws.Invalidate()
}

// GoToSource goes to a place in a document, opening it if another one is open.
func (ws *Workspace) GoToSource(path string, offset int) {
	if ws.Current() != path {
		ws.Open(path)
	}
	if ws.Current() != path {
		return
	}
	ws.setView(host.ViewDoc)
	ws.doc.GoTo(offset)
}

// FollowLink opens a linked file, or says that it is gone.
func (ws *Workspace) FollowLink(path string) {
	if _, ok := ws.Entry(path); ok {
		ws.Open(path)
		return
	}
	ws.ToastError(path + " does not exist")
}

// firstDocument is the newest document, the one to show when nothing else asks.
func (ws *Workspace) firstDocument() (string, bool) {
	var ids []string
	var modified []int64
	for _, e := range ws.entries {
		if e.Typst {
			ids, modified = append(ids, e.ID), append(modified, e.Modified)
		}
	}
	return project.Newest(ids, modified)
}

// NewDocument makes a document in the folder parent ("" for the vault root) and opens it.
func (ws *Workspace) NewDocument(parent string) {
	ws.WhenSaved(func() { ws.createDoc(parent) })
}

// newChild makes a document in the folder parent, or beside it when it is a file.
func (ws *Workspace) newChild(parent string) {
	if e, ok := ws.Entry(parent); ok && !e.Folder {
		parent = e.Parent
	}
	ws.NewDocument(parent)
}

func (ws *Workspace) createDoc(parent string) {
	id, err := ws.store.Create(parent)
	if err != nil {
		ws.ToastError(fmt.Sprintf("Could not create document: %v", err))
		return
	}
	ws.RefreshEntries()
	if parent != "" {
		ws.setExpanded(parent, true)
	}
	ws.Open(id)
}

// NewFolder makes a folder in parent with its name ready to edit.
func (ws *Workspace) NewFolder(parent string) {
	id, err := ws.store.CreateFolder(parent, "New folder")
	if err != nil {
		ws.ToastError(fmt.Sprintf("Could not create folder: %v", err))
		return
	}
	ws.RefreshEntries()
	if parent != "" {
		ws.setExpanded(parent, true)
	}
	ws.startRename(id)
}

// askDelete asks before moving ids to the Trash; batch for the selected rows.
func (ws *Workspace) askDelete(ids []string, batch bool) {
	if len(ids) == 0 {
		return
	}
	title := fmt.Sprintf("Delete the %d selected items?", len(ids))
	if !batch && len(ids) == 1 {
		title = "Delete " + ws.titleOf(ids[0]) + "?"
	}
	ws.confirm = confirmState{
		open: true, title: title, action: "Delete", danger: true,
		body: fmt.Sprintf("Deleted items will be moved to Trash and permanently deleted after %d days.", vault.TrashDays),
		then: func() {
			if batch {
				ws.sidebar.batch = nil
			}
			ws.deleteDocs(ids)
		},
	}
}

func (ws *Workspace) titleOf(id string) string {
	if e, ok := ws.Entry(id); ok {
		return vault.DisplayTitle(e.Title)
	}
	for _, t := range ws.trash {
		if t.ID == id {
			return vault.DisplayTitle(t.Title)
		}
	}
	return ""
}

// topmost leaves out ids inside another of ids: they go with it.
func topmost(ids []string) []string {
	var out []string
	for _, id := range ids {
		inside := false
		for _, other := range ids {
			if other != id && vault.IsWithin(id, other) {
				inside = true
			}
		}
		if !inside {
			out = append(out, id)
		}
	}
	return out
}

// deleteDocs moves files and folders to the Trash; if the open one goes, opens the next.
func (ws *Workspace) deleteDocs(ids []string) {
	current := ws.Current()
	for _, id := range ids {
		if current != "" && vault.IsWithin(current, id) {
			ws.WhenSaved(func() { ws.trashDocs(ids) })
			return
		}
	}
	ws.trashDocs(ids)
}

func (ws *Workspace) trashDocs(ids []string) {
	var deleted, problems []string
	for _, id := range topmost(ids) {
		warnings, err := ws.store.Delete(id)
		if err != nil {
			problems = append(problems, fmt.Sprintf("Could not delete document: %v", err))
			continue
		}
		deleted = append(deleted, id)
		problems = append(problems, warnings...)
	}
	within := func(path string) bool {
		for _, d := range deleted {
			if vault.IsWithin(path, d) {
				return true
			}
		}
		return false
	}
	if ws.doc != nil && within(ws.doc.ID()) {
		ws.doc.Close()
		ws.doc = nil
	}
	ws.RefreshEntries()
	ws.sidebar.batch = slices.DeleteFunc(ws.sidebar.batch, within)
	if ws.doc == nil {
		// Stay on Home or Trash if that's where the delete came from.
		view := ws.view
		if next, ok := ws.firstDocument(); ok {
			ws.loadDocument(next)
		} else {
			ws.createDoc("")
		}
		ws.setView(view)
	} else {
		ws.followTopDocument()
		ws.refreshOutline()
	}
	if ws.view == host.ViewTrash {
		problems = append(problems, ws.store.PurgeExpired()...)
		ws.trash = ws.store.ListTrash()
	}
	if len(problems) == 0 {
		ws.Toast("Deleted successfully")
	} else {
		ws.ToastError(strings.Join(problems, "; "))
	}
}

// followTopDocument compiles from where the open file and its top document are now.
func (ws *Workspace) followTopDocument() {
	if ws.doc == nil || !vault.IsDocument(ws.doc.ID()) {
		return
	}
	ws.doc.SetPath(ws.doc.ID(), ws.TopDocument(ws.doc.ID()))
}

// exportPDF asks where to save, then compiles the document's current text to a PDF.
func (ws *Workspace) exportPDF(id string) {
	text, err := ws.TextOf(id)
	if err != nil {
		ws.ToastError(err.Error())
		return
	}
	name := vault.Untitled
	if e, ok := ws.Entry(id); ok {
		name = vault.DisplayTitle(e.Title)
	}
	root := ws.store.Root()
	go func() {
		path, err := ws.PickSavePath(root, name+".pdf")
		if err != nil {
			ws.Update(func() { ws.ToastError(fmt.Sprintf("Could not open the file dialog: %v", err)) })
			return
		}
		if path == "" {
			return
		}
		data, err := document.ExportPDF(root, id, text)
		if err == nil {
			err = os.WriteFile(path, data, 0o644)
		}
		ws.Update(func() {
			if err != nil {
				ws.ToastError(fmt.Sprintf("Could not export PDF: %v", err))
			} else {
				ws.Toast("Exported PDF")
			}
		})
	}()
}

// importFiles copies chosen files into the vault and opens the first document.
func (ws *Workspace) importFiles() {
	go func() {
		paths, err := ws.PickFiles("", true)
		ws.Update(func() {
			if err != nil {
				ws.ToastError(fmt.Sprintf("Could not open the file dialog: %v", err))
				return
			}
			first := ""
			for _, path := range paths {
				id, err := ws.store.Import(path)
				if err != nil {
					ws.ToastError(fmt.Sprintf("Could not import %s: %v", path, err))
				} else if first == "" && vault.IsDocument(id) {
					first = id
				}
			}
			ws.RefreshEntries()
			if first != "" {
				ws.Open(first)
			}
		})
	}()
}

// duplicateDoc copies a document beside the original and opens the copy.
func (ws *Workspace) duplicateDoc(id string) {
	if copied, ok := ws.copyDoc(id); ok {
		ws.RefreshEntries()
		ws.Open(copied)
	}
}

func (ws *Workspace) copyDoc(id string) (string, bool) {
	text, err := ws.TextOf(id)
	if err != nil {
		ws.ToastError(err.Error())
		return "", false
	}
	e, ok := ws.Entry(id)
	if !ok {
		return "", false
	}
	copied, err := ws.store.CreateFrom(e.Parent, e.Title+" copy", text)
	if err != nil {
		ws.ToastError(fmt.Sprintf("Could not duplicate document: %v", err))
		return "", false
	}
	return copied, true
}

func (ws *Workspace) togglePin(id string) {
	e := ws.entry(id)
	if e == nil {
		return
	}
	on := !e.Meta.Pinned
	if err := ws.store.SetPinned(id, on); err != nil {
		ws.ToastError(fmt.Sprintf("Could not pin %s: %v", id, err))
		return
	}
	e.Meta.Pinned = on
	if on {
		ws.Toast("Pinned")
	} else {
		ws.Toast("Unpinned")
	}
}

func (ws *Workspace) toggleStar(id string) {
	e := ws.entry(id)
	if e == nil {
		return
	}
	on := e.Meta.Starred == 0
	t, err := ws.store.SetStarred(id, on)
	if err != nil {
		ws.ToastError(fmt.Sprintf("Could not change favorites: %v", err))
		return
	}
	e.Meta.Starred = t
	if on {
		ws.Toast("Added to favorites")
	} else {
		ws.Toast("Removed from favorites")
	}
}

// updatedPaths is the toast after a rename or move rewrote other files.
func updatedPaths(files int) string {
	if files == 1 {
		return "Updated paths in 1 file"
	}
	return fmt.Sprintf("Updated paths in %d files", files)
}

// moveDocs moves files and folders into target ("" for the vault root),
// skipping what is already there and moves of a folder into itself.
func (ws *Workspace) moveDocs(ids []string, target string) {
	ws.flushDoc()
	moved, updated := 0, 0
	var problems []string
	for _, id := range topmost(ids) {
		e, ok := ws.Entry(id)
		if !ok || e.Parent == target || target != "" && vault.IsWithin(target, id) {
			continue
		}
		outcome, err := ws.store.SetParent(id, target)
		if err != nil {
			problems = append(problems, fmt.Sprintf("Could not move: %v", err))
			continue
		}
		ws.PathChanged(id, outcome.ID, outcome.Rewritten)
		moved++
		updated += len(outcome.Rewritten)
		problems = append(problems, outcome.Warnings...)
	}
	if target != "" {
		ws.setExpanded(target, true)
		ws.revealInTree(target)
	}
	if updated > 0 {
		ws.Toast(updatedPaths(updated))
	} else if moved > 0 {
		ws.Toast("Moved successfully")
	}
	ws.Warn(problems)
}

// rename gives id a new title.
func (ws *Workspace) rename(id, title string) {
	e, ok := ws.Entry(id)
	title = strings.TrimSpace(title)
	if !ok || title == strings.TrimSpace(e.Title) {
		return
	}
	ws.flushDoc()
	moved, err := ws.store.Rename(id, title)
	if err != nil {
		ws.ToastError(fmt.Sprintf("Could not rename document: %v", err))
		return
	}
	ws.PathChanged(id, moved.ID, moved.Rewritten)
	if len(moved.Rewritten) > 0 {
		ws.Toast(updatedPaths(len(moved.Rewritten)))
	}
	ws.Warn(moved.Warnings)
}
