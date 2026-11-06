package vault

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"testing"
)

func vault(t *testing.T) *Store {
	store, _, err := OpenAt(t.TempDir())
	if err != nil { t.Fatal(err) }
	store.SetRewriter(fakeRewriter{})
	return store
}

// fakeRewriter stands in for Typst's parser with string logic over
// `#include "…"`, `#import "…"` and `image("…")` literals.
type fakeRewriter struct{}

var literalPattern = regexp.MustCompile(`(#include |#import |image\()"([^"]*)"`)

func literals(text string) []string {
	var out []string
	for _, m := range literalPattern.FindAllStringSubmatch(text, -1) { out = append(out, m[2]) }
	return out
}

// resolve finds what literal in file means when main is compiled: a rooted path
// is sought from main's folder upward, then at the vault root.
func resolve(literal, file, main string, exists func(string) bool) (string, bool) {
	rest, rooted := strings.CutPrefix(literal, "/")
	if !rooted {
		path, ok := ResolveLiteral(DirParts(file), literal)
		return path, ok && exists(path)
	}
	dir := DirParts(main)
	for i := len(dir); i >= 0; i-- {
		if path := Join(strings.Join(dir[:i], "/"), rest); exists(path) { return path, true }
	}
	return "", false
}

func (fakeRewriter) TopDocument(ids []string, read func(string) (string, bool), id string) string {
	exists := func(path string) bool { return slices.Contains(ids, path) }
	top, seen := id, map[string]bool{id: true}
	for {
		next := ""
		for _, other := range ids {
			text, _ := read(other)
			for _, literal := range literals(text) {
				if path, ok := resolve(literal, other, other, exists); ok && path == top && !seen[other] { next = other }
			}
		}
		if next == "" { return top }
		top, seen[next] = next, true
	}
}

func (fakeRewriter) RewritePaths(text, oldFile, newFile, main string, files []string, from, to string) (string, bool) {
	exists := func(path string) bool { return slices.Contains(files, path) }
	moved := func(path string) string { if p, ok := MovedPath(path, from, to); ok { return p }; return path }
	out := literalPattern.ReplaceAllStringFunc(text, func(match string) string {
		m := literalPattern.FindStringSubmatch(match)
		target, ok := resolve(m[2], oldFile, main, exists)
		if !ok { return match }
		target = moved(target)
		literal := RelativeTo(DirParts(newFile), target)
		if strings.HasPrefix(m[2], "/") {
			literal = "/" + target
			dir := DirParts(moved(main))
			for i := len(dir); i > 0; i-- {
				if rest, ok := strings.CutPrefix(target, strings.Join(dir[:i], "/")+"/"); ok { literal = "/" + rest; break }
			}
		}
		return m[1] + Quoted(literal)
	})
	return out, out != text
}

func rewrittenIDs(moved Moved) []string {
	var ids []string
	for _, r := range moved.Rewritten { ids = append(ids, r.ID) }
	return ids
}

func TestASaveShowsInTheListEvenWithinTheSameSecondAndSize(t *testing.T) {
	store := vault(t)
	id := must(store.CreateFrom("", "doc", "aaa"))
	if body := store.List()[0].Body; body != "aaa" { t.Fatal(body) }
	if err := store.Save(id, "bbb"); err != nil { t.Fatal(err) }
	if body := store.List()[0].Body; body != "bbb" { t.Fatal(body) }
}

// projectVault holds a project in `paper/`, and a file at the root with the same name as one in it.
func projectVault(t *testing.T) *Store {
	store := vault(t)
	must(store.CreateFolder("", "paper"))
	must(store.CreateFolder("paper", "lib"))
	must(store.CreateFolder("paper", "tools"))
	must(store.CreateFolder("", "lib"))
	must(store.CreateFrom("paper", "main", "#include \"chapter.typ\"\n"))
	must(store.CreateFrom("paper", "chapter", "#import \"/lib/shared.typ\"\n"))
	must(store.CreateFrom("paper/lib", "shared", ""))
	must(store.CreateFrom("lib", "shared", ""))
	return store
}

func TestARootedPathFollowsTheFileTheCompilerFinds(t *testing.T) {
	store := projectVault(t)
	moved := must(store.SetParent("paper/lib/shared.typ", "paper/tools"))
	if ids := rewrittenIDs(moved); !slices.Equal(ids, []string{"paper/chapter.typ"}) { t.Fatal(ids) }
	if text := must(store.Load("paper/chapter.typ")); text != "#import \"/tools/shared.typ\"\n" { t.Fatal(text) }
}

func TestARootedPathIgnoresAFileItDoesNotReach(t *testing.T) {
	store := projectVault(t)
	moved := must(store.SetParent("lib/shared.typ", "paper/tools"))
	if len(moved.Rewritten) != 0 { t.Fatal(rewrittenIDs(moved)) }
	if text := must(store.Load("paper/chapter.typ")); text != "#import \"/lib/shared.typ\"\n" { t.Fatal(text) }
}

func TestARootedPathNamesTheVaultRootWhenItsProjectIsLeftBehind(t *testing.T) {
	store := projectVault(t)
	must(store.CreateFrom("paper", "note", "#import \"/lib/shared.typ\"\n"))
	must(store.SetParent("paper/note.typ", ""))
	if text := must(store.Load("note.typ")); text != "#import \"/paper/lib/shared.typ\"\n" { t.Fatal(text) }
}

func TestListingTheTrashDeletesNothing(t *testing.T) {
	store := vault(t)
	id := must(store.Create(""))
	must(store.Delete(id))
	index := filepath.Join(store.Root(), StateDir, trashFile)
	var trash map[string]map[string]json.RawMessage
	if err := json.Unmarshal(must(os.ReadFile(index)), &trash); err != nil { t.Fatal(err) }
	for _, entry := range trash { entry["deleted"] = json.RawMessage("1") }
	if err := os.WriteFile(index, must(json.Marshal(trash)), 0o644); err != nil { t.Fatal(err) }
	store, _, err := OpenAt(store.Root())
	if err != nil { t.Fatal(err) }
	if len(store.ListTrash()) != 1 { t.Fatal(store.ListTrash()) }
	if warnings := store.PurgeExpired(); len(warnings) != 0 { t.Fatal(warnings) }
	if len(store.ListTrash()) != 0 { t.Fatal(store.ListTrash()) }
}

func TestARenameUpdatesTheDocumentsThatIncludeIt(t *testing.T) {
	store := vault(t)
	id := must(store.CreateFrom("", "a", "x"))
	must(store.CreateFrom("", "b", "#include \"a.typ\""))
	moved := must(store.Rename(id, "c"))
	if moved.ID != "c.typ" || !slices.Equal(rewrittenIDs(moved), []string{"b.typ"}) { t.Fatal(moved) }
	if text := must(store.Load("b.typ")); text != "#include \"c.typ\"" { t.Fatal(text) }
}

func TestAMoveWithoutARewriterStillCarriesState(t *testing.T) {
	store := vault(t)
	store.SetRewriter(nil)
	id := must(store.CreateFrom("", "a", "x"))
	must(store.CreateFrom("", "b", "#include \"a.typ\""))
	if err := store.SetPinned(id, true); err != nil { t.Fatal(err) }
	must(must(store.Versions(id)).Keep("x"))
	moved := must(store.Rename(id, "c"))
	if len(moved.Rewritten) != 0 || must(store.Load("b.typ")) != "#include \"a.typ\"" { t.Fatal(moved) }
	if !store.index["c.typ"].Pinned || len(must(store.Versions("c.typ")).List()) != 1 { t.Fatal("state left behind") }
}

func TestAMoveWarnsAboutADocumentItCannotRead(t *testing.T) {
	store := vault(t)
	id := must(store.CreateFrom("", "a", "x"))
	if err := os.WriteFile(filepath.Join(store.Root(), "b.typ"), []byte{0xff, 0xfe}, 0o644); err != nil { t.Fatal(err) }
	moved := must(store.Rename(id, "c"))
	if len(moved.Warnings) != 1 || !strings.Contains(moved.Warnings[0], "b.typ") { t.Fatal(moved.Warnings) }
}

func TestADocumentNameGetsOneExtension(t *testing.T) {
	store := vault(t)
	id := must(store.CreateFrom("", "a", ""))
	bare := must(store.Rename(id, "flaky2024")).ID
	if bare != "flaky2024.typ" { t.Fatal(bare) }
	if id := must(store.Rename(bare, "other.typ")).ID; id != "other.typ" { t.Fatal(id) }
}

func TestAnotherFileKeepsTheNameItIsGiven(t *testing.T) {
	store := vault(t)
	if err := os.WriteFile(filepath.Join(store.Root(), "paper.pdf"), []byte(PDFHeader), 0o644); err != nil { t.Fatal(err) }
	if id := must(store.Rename("paper.pdf", "notes.pdf")).ID; id != "notes.pdf" { t.Fatal(id) }
}

func TestAPathCannotLeaveTheVault(t *testing.T) {
	store := vault(t)
	for _, id := range []string{"", "../x", "a/../../x", "/etc/passwd", "a//b", "./a"} {
		if _, err := store.Path(id); err == nil || !strings.Contains(err.Error(), "Invalid vault path") { t.Errorf("%q: %v", id, err) }
	}
}

func TestTheListPutsParentsBeforeChildrenInNameOrder(t *testing.T) {
	store := vault(t)
	must(store.CreateFolder("", "b"))
	must(store.CreateFrom("b", "z", ""))
	must(store.CreateFrom("b", "A", ""))
	must(store.CreateFrom("", "a", ""))
	if err := os.Mkdir(filepath.Join(store.Root(), ".hidden"), 0o755); err != nil { t.Fatal(err) }
	var ids []string
	for _, e := range store.List() { ids = append(ids, e.ID) }
	if want := []string{"a.typ", "b", "b/A.typ", "b/z.typ"}; !slices.Equal(ids, want) { t.Fatal(ids) }
}

func TestAMoveIntoItselfAndOntoATakenNameIsRefused(t *testing.T) {
	store := vault(t)
	must(store.CreateFolder("", "a"))
	must(store.CreateFolder("a", "b"))
	if _, err := store.SetParent("a", "a/b"); err == nil || err.Error() != "A folder cannot move into itself" { t.Fatal(err) }
	must(store.CreateFrom("", "x", ""))
	must(store.CreateFrom("", "y", ""))
	if _, err := store.Rename("x.typ", "y"); err == nil || err.Error() != "y.typ already exists" { t.Fatal(err) }
}

func TestTheTrashKeepsStateAndRestoresUnderAFreeName(t *testing.T) {
	store := vault(t)
	must(store.CreateFolder("", "f"))
	id := must(store.CreateFrom("f", "a", "one"))
	if err := store.SetPinned(id, true); err != nil { t.Fatal(err) }
	must(store.Delete("f"))
	if _, ok := store.index[id]; ok { t.Fatal("index kept the deleted file") }
	trash := store.ListTrash()
	if len(trash) != 1 || trash[0].Title != "f" || trash[0].Parent != "" { t.Fatal(trash) }
	must(store.CreateFolder("", "f"))
	restored := must(store.Restore(trash[0].ID))
	if !restored.Renamed || must(store.Load("f 1/a.typ")) != "one" || !store.index["f 1/a.typ"].Pinned { t.Fatal(restored, store.index) }
	if _, err := store.Restore(trash[0].ID); err == nil || err.Error() != "Not in Trash" { t.Fatal(err) }
	if err := store.Purge("nope"); err == nil { t.Fatal("purged nothing") }
}

func TestStateFilesKeepTheirFormat(t *testing.T) {
	store := vault(t)
	if err := store.SetPageOffset("p.pdf", -2); err != nil { t.Fatal(err) }
	must(store.SetStarred("a.typ", false))
	if err := store.SetPinned("b.typ", true); err != nil { t.Fatal(err) }
	data := string(must(os.ReadFile(filepath.Join(store.Root(), StateDir, indexFile))))
	want := "{\n  \"a.typ\": {},\n  \"b.typ\": {\n    \"pinned\": true\n  },\n  \"p.pdf\": {\n    \"page_offset\": -2\n  }\n}"
	if data != want { t.Fatalf("%s", data) }
	if format := string(must(os.ReadFile(filepath.Join(store.Root(), StateDir, formatFile)))); format != "{\n  \"format\": 1\n}" { t.Fatal(format) }
	width := float32(240)
	if err := store.UpdateSettings(func(s *Settings) { s.SidebarWidth = &width; s.Expanded = []string{"a"} }); err != nil { t.Fatal(err) }
	if settings := store.Settings(); *settings.SidebarWidth != 240 || settings.WindowSize != nil || settings.Expanded[0] != "a" { t.Fatal(settings) }
}

func TestADamagedIndexComesBackFromItsBackup(t *testing.T) {
	store := vault(t)
	if err := store.SetPinned("a.typ", true); err != nil { t.Fatal(err) }
	if err := store.SetPinned("b.typ", true); err != nil { t.Fatal(err) }
	index := filepath.Join(store.Root(), StateDir, indexFile)
	if err := os.WriteFile(index, []byte("{"), 0o644); err != nil { t.Fatal(err) }
	store, warnings, err := OpenAt(store.Root())
	if err != nil { t.Fatal(err) }
	if len(warnings) != 1 || !strings.Contains(warnings[0], "restored from backup") || !store.index["a.typ"].Pinned { t.Fatal(warnings, store.index) }
	if found, _ := filepath.Glob(index + ".corrupt-*"); len(found) != 1 { t.Fatal(found) }
}
