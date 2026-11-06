package project

import (
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/cheetahbyte/thesio/internal/vault"
)

func init() {
	Parse = parseStub
	DefinesLabel = definesLabelStub
}

// parseStub stands in for Typst's parser: headings are lines starting with `=`,
// includes are `#include "…"`.
func parseStub(text string) Parsed {
	var parsed Parsed
	offset := 0
	for line := range strings.SplitAfterSeq(text, "\n") {
		trimmed := strings.TrimRight(line, "\n")
		if level := len(trimmed) - len(strings.TrimLeft(trimmed, "=")); level > 0 {
			title, _, _ := strings.Cut(trimmed[level:], " <")
			if title = strings.TrimSpace(title); title != "" {
				parsed.Headings = append(parsed.Headings, Heading{Offset: offset, Level: level, Title: title})
			}
		}
		for at := 0; ; {
			i := strings.Index(trimmed[at:], `#include "`)
			if i < 0 {
				break
			}
			start := at + i + len(`#include "`)
			end := strings.IndexByte(trimmed[start:], '"')
			if end < 0 {
				break
			}
			parsed.Includes = append(parsed.Includes, Include{Offset: offset + at + i, Literal: trimmed[start : start+end]})
			at = start + end
		}
		offset += len(line)
	}
	return parsed
}

// definesLabelStub finds `<key>` at the end of a line that is not a comment.
func definesLabelStub(text, key string) bool {
	for line := range strings.SplitSeq(text, "\n") {
		line = strings.TrimSpace(line)
		if !strings.HasPrefix(line, "//") && strings.HasSuffix(line, "<"+key+">") {
			return true
		}
	}
	return false
}

type files [][2]string

func (f files) read(path string) (string, bool) {
	for _, file := range f {
		if file[0] == path {
			return file[1], true
		}
	}
	return "", false
}

func (f files) ids() []string {
	var ids []string
	for _, file := range f {
		ids = append(ids, file[0])
	}
	return ids
}

func TestLaunchTargetIsTheNewestDocumentWithinItsTopFile(t *testing.T) {
	root := t.TempDir()
	for _, dir := range []string{"Thesis/chapters", ".trash"} {
		if err := os.MkdirAll(filepath.Join(root, dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	write := func(path, text string, age time.Duration) {
		full := filepath.Join(root, path)
		if err := os.WriteFile(full, []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
		at := time.Now().Add(-age)
		if err := os.Chtimes(full, at, at); err != nil {
			t.Fatal(err)
		}
	}
	write("old.typ", "old", 500*time.Second)
	write("Thesis/thesis.typ", `#include "chapters/one.typ"`, 300*time.Second)
	write("Thesis/chapters/one.typ", "newest", 100*time.Second)
	write(".trash/hidden.typ", "hidden", 0)
	got, ok := Launch(root)
	if want := (LaunchTarget{Document: "Thesis/chapters/one.typ", Main: "Thesis/thesis.typ", Text: "newest"}); !ok || got != want {
		t.Fatal(got, ok)
	}
	if _, ok := Launch(t.TempDir()); ok {
		t.Fatal("an empty vault has a launch target")
	}
}

func TestAnOutlineReadsThroughIncludesInOrder(t *testing.T) {
	f := files{
		{"main.typ", "= One <one>\n#include \"ch/a.typ\"\n== Two\n#include \"ch/b.typ\"\n// = Not a heading\n`= Nor this`\n"},
		{"ch/a.typ", "= In A\nText\n#include \"../main.typ\"\n"},
		{"ch/b.typ", "=== In *B*\n"},
	}
	var summary []Heading
	for _, h := range Open("main.typ", f.read).Outline() {
		summary = append(summary, Heading{Path: h.Path, Level: h.Level, Title: h.Title})
	}
	want := []Heading{{"main.typ", 0, 1, "One"}, {"ch/a.typ", 0, 1, "In A"}, {"main.typ", 0, 2, "Two"}, {"ch/b.typ", 0, 3, "In *B*"}}
	if !slices.Equal(summary, want) {
		t.Fatal(summary)
	}
}

func TestALabelInAnyFileOfADocumentCountsForAllOfThem(t *testing.T) {
	f := files{
		{"main.typ", "#include \"a.typ\"\n#include \"b.typ\"\n"},
		{"a.typ", "= One <here>\n"},
		{"b.typ", "See @here and #ref(<named>). `<raw>`\n#show <shown>: strong\n// <comment>\n"},
		{"other.typ", "See @here.\n"},
	}
	documents := LoadDocuments(f.ids(), f.read)
	if !documents.HasLabel("b.typ", "here") {
		t.Fatal("from a sibling chapter")
	}
	if !documents.HasLabel("main.typ", "here") {
		t.Fatal("main")
	}
	if documents.HasLabel("other.typ", "here") {
		t.Fatal("another document")
	}
	for _, name := range []string{"named", "raw", "shown", "comment"} {
		if documents.HasLabel("b.typ", name) {
			t.Fatal(name)
		}
	}
}

func TestARootPathMeansWhatTheCompilerMeansByIt(t *testing.T) {
	f := files{
		{"shared.typ", "= Wrong folder\n"},
		{"Thesis/shared.typ", "= Right folder\n"},
		{"Thesis/thesis.typ", "#include \"/shared.typ\"\n"},
	}
	project := Open("Thesis/thesis.typ", f.read)
	if !project.Contains("Thesis/shared.typ") || project.Contains("shared.typ") {
		t.Fatal(project.Files())
	}
	if title := project.Outline()[0].Title; title != "Right folder" {
		t.Fatal(title)
	}
	documents := LoadDocuments(f.ids(), f.read)
	if got := documents.Includers("Thesis/shared.typ"); !slices.Equal(got, []string{"Thesis/thesis.typ"}) {
		t.Fatal(got)
	}
	if got := documents.TopDocument("shared.typ"); got != "shared.typ" {
		t.Fatal(got)
	}
	if got := documents.TopDocument("Thesis/shared.typ"); got != "Thesis/thesis.typ" {
		t.Fatal(got)
	}
}

func TestCandidatesRunFromTheMainFolderUpward(t *testing.T) {
	if got := Candidates("a/b/main.typ", "x.typ"); !slices.Equal(got, []string{"a/b/x.typ", "a/x.typ", "x.typ"}) {
		t.Fatal(got)
	}
}

func TestTheNewestIsTheFirstOfThoseModifiedTogether(t *testing.T) {
	if id, ok := Newest([]string{"a", "b", "c"}, []int64{1, 3, 3}); !ok || id != "b" {
		t.Fatal(id)
	}
	if _, ok := Newest(nil, nil); ok {
		t.Fatal("newest of none")
	}
}

func TestTheRewriterGivesEachMovedFileItsTopDocument(t *testing.T) {
	store, _, err := vault.OpenAt(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	store.SetRewriter(NewRewriter())
	mains := map[string]string{}
	RewritePaths = func(text, oldFile, _, main string, _ []string, from, to string) (string, bool) {
		mains[oldFile] = main
		updated := strings.ReplaceAll(text, vault.Quoted(from), vault.Quoted(to))
		return updated, updated != text
	}
	t.Cleanup(func() { RewritePaths = nil })
	for _, f := range [][2]string{{"main", `#include "a.typ"`}, {"a", "= A"}, {"other", ""}} {
		if _, cerr := store.CreateFrom("", f[0], f[1]); cerr != nil {
			t.Fatal(cerr)
		}
	}
	moved, err := store.Rename("a.typ", "c")
	if err != nil {
		t.Fatal(err)
	}
	if len(moved.Rewritten) != 1 || moved.Rewritten[0] != (vault.Rewritten{ID: "main.typ", Text: `#include "c.typ"`}) {
		t.Fatal(moved)
	}
	if want := map[string]string{"a.typ": "main.typ", "main.typ": "main.typ", "other.typ": "other.typ"}; !maps.Equal(mains, want) {
		t.Fatal(mains)
	}
}
