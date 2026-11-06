package app

import (
	"testing"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

// workspace is a window's workspace over a new vault, without a document open.
func workspace(t *testing.T) (*Workspace, *ui.Tester) {
	t.Helper()
	root := t.TempDir()
	t.Setenv("THESIO_HOME", root)
	Wire()
	store, warnings, err := vault.Open()
	if err != nil {
		t.Fatal(err)
	}
	ws := New(nil, store, warnings)
	return ws, ui.NewTester(ws.View, 1200, 800)
}

func TestANewVaultShowsTheEmptyHome(t *testing.T) {
	_, tt := workspace(t)
	if !tt.HasText("Nothing here yet") || !tt.HasText("New Document") {
		t.Errorf("texts %q", tt.Texts())
	}
}

func TestADocumentIsListedInTheSidebar(t *testing.T) {
	ws, tt := workspace(t)
	if _, err := ws.Store().CreateFrom("", "Thesis", "= Intro"); err != nil {
		t.Fatal(err)
	}
	ws.RefreshEntries()
	tt.Frame()
	if !tt.HasText("Thesis") || tt.HasText("Nothing here yet") {
		t.Errorf("texts %q", tt.Texts())
	}
}

func TestANewFolderIsRenamedInPlace(t *testing.T) {
	ws, tt := workspace(t)
	ws.NewFolder("")
	tt.Frame()
	tt.Type("Chapters")
	tt.Key(0, ui.KeyEnter)
	if _, ok := ws.Entry("Chapters"); !ok || !tt.HasText("Chapters") {
		t.Errorf("entries %v, texts %q", ws.Entries(), tt.Texts())
	}
}

func TestTheTrashListsWhatWasDeleted(t *testing.T) {
	ws, tt := workspace(t)
	ws.ShowView(host.ViewTrash)
	tt.Frame()
	if !tt.HasText("Trash is empty") {
		t.Errorf("texts %q", tt.Texts())
	}
	id, err := ws.Store().CreateFrom("", "Draft", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ws.Store().Delete(id); err != nil {
		t.Fatal(err)
	}
	ws.RefreshEntries()
	ws.ShowView(host.ViewTrash)
	tt.Frame()
	if !tt.HasText("Draft") || !tt.HasText("30 days") || !tt.HasText("Files") {
		t.Errorf("texts %q", tt.Texts())
	}
}

func TestAVisitReadsAsTheListsShowIt(t *testing.T) {
	now := int64(1_800_000_000)
	for secs, want := range map[int64]string{now - 30: "just now", now - 300: "5 min ago"} {
		if got := formatVisit(secs, now); got != want {
			t.Errorf("formatVisit(%d) = %q, want %q", now-secs, got, want)
		}
	}
	if got := formatCount(1234567); got != "1,234,567" {
		t.Errorf("formatCount = %q", got)
	}
}

func TestASnippetStartsShortlyBeforeItsHit(t *testing.T) {
	text, hits, ok := snippet("first line\nsome long prefix text before the Needle here\nnext", "needle")
	if !ok || text[hits[0][0]:hits[0][1]] != "Needle" || text[:len("…")] != "…" || text[len(text)-4:] != "next" {
		t.Errorf("snippet %q %v", text, hits)
	}
}
