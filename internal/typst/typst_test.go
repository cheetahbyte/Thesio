package typst

import (
	"bytes"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func vault(t *testing.T, files map[string]string) string {
	t.Helper()
	root := t.TempDir()
	for path, text := range files {
		full := filepath.Join(root, filepath.FromSlash(path))
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(full, []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	return root
}

func compiled(t *testing.T, main string, files map[string]string) *Compilation {
	t.Helper()
	compiler := NewCompiler(vault(t, files))
	t.Cleanup(compiler.Close)
	k, err := compiler.Compile(main)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(k.Close)
	return k
}

func errorsOf(k *Compilation) []string {
	var found []string
	for _, d := range k.Diagnostics {
		if d.Error {
			found = append(found, d.Path+": "+d.Message)
		}
	}
	return found
}

func chapters(t *testing.T) *Compilation {
	k := compiled(t, "main.typ", map[string]string{
		"main.typ":           "#include \"chapters/one.typ\"\n#pagebreak()\n#include \"chapters/two.typ\"\n#pagebreak()\n#include \"chapters/three.typ\"\n",
		"chapters/one.typ":   "First chapter.\n",
		"chapters/two.typ":   "Second chapter.\n\n\n" + strings.Repeat("Word ", 600) + "\n",
		"chapters/three.typ": "= Third\n",
	})
	if found := errorsOf(k); len(found) > 0 {
		t.Fatal(found)
	}
	return k
}

func TestAVaultCompilesRendersAndExports(t *testing.T) {
	k := chapters(t)
	if !k.OK || k.Main != "main.typ" || len(k.Pages) < 3 {
		t.Fatalf("ok %v main %q pages %d", k.OK, k.Main, len(k.Pages))
	}
	page := k.Pages[0]
	if page.Width < 595 || page.Width > 596 || len(page.Hash) != 32 {
		t.Fatalf("page %+v", page)
	}
	if k.Pages[0].Hash == k.Pages[1].Hash {
		t.Fatal("different pages share a hash")
	}
	if !slices.Contains(k.Words, FileWords{Path: "chapters/one.typ", Words: 2}) {
		t.Fatalf("words %+v", k.Words)
	}
	img, err := k.Render(0, 2)
	if err != nil {
		t.Fatal(err)
	}
	if b := img.Bounds(); b.Dx() < 1189 || b.Dx() > 1192 || len(img.Pix) != b.Dx()*b.Dy()*4 {
		t.Fatalf("image %v", b)
	}
	if px := img.RGBAAt(0, 0); px.R != 255 || px.A != 255 {
		t.Fatalf("corner %+v, want white", px)
	}
	if _, rerr := k.Render(99, 1); rerr == nil {
		t.Fatal("rendered a page that does not exist")
	}
	pdf, err := k.PDF()
	if err != nil || !bytes.HasPrefix(pdf, []byte("%PDF-")) {
		t.Fatalf("pdf %v %q", err, pdf[:min(len(pdf), 8)])
	}
}

func TestTheSourceAndThePreviewPointAtEachOther(t *testing.T) {
	k := chapters(t)
	page, y, ok := k.PositionOf("chapters/two.typ", 3)
	if !ok || page != 1 {
		t.Fatalf("position %d %v %v", page, y, ok)
	}
	target, ok := k.TargetAt(page, 75, y-3)
	if !ok || target.Path != "chapters/two.typ" || target.Offset >= len("Second chapter.") || target.Note != "" {
		t.Fatalf("target %+v %v", target, ok)
	}
	if _, ok := k.TargetAt(1, 1, 1); ok {
		t.Fatal("a click on the margin landed")
	}
	if page, _, _ := k.PositionOf("chapters/three.typ", 0); page != 2 {
		t.Fatalf("heading on page %d", page)
	}
	if got := k.PagesFrom("chapters/two.typ"); !slices.Equal(got, []bool{false, true, false}) {
		t.Fatalf("pages from %v", got)
	}
}

func TestANoteLinkIsATarget(t *testing.T) {
	k := compiled(t, "main.typ", map[string]string{
		"main.typ":       "#include \"ch/one.typ\"\n",
		"ch/one.typ":     "#link(\"../notes/idea.typ\")[The idea] and more.\n",
		"notes/idea.typ": "An idea.\n",
	})
	page, y, _ := k.PositionOf("ch/one.typ", 30)
	if target, ok := k.TargetAt(page, 75, y-3); !ok || target.Note != "notes/idea.typ" {
		t.Fatalf("target %+v", target)
	}
}

func TestADiagnosticMovesOntoNewerText(t *testing.T) {
	text := "é #nope\n"
	k := compiled(t, "main.typ", map[string]string{"main.typ": text})
	if k.OK || len(k.Diagnostics) != 1 {
		t.Fatalf("diagnostics %+v", k.Diagnostics)
	}
	d := k.Diagnostics[0]
	if !d.Error || d.Path != "main.typ" || d.Line != 0 || text[d.Start:d.End] != "nope" {
		t.Fatalf("diagnostic %+v", d)
	}
	above := "日本語 " + text
	placed := k.Placed("main.typ", above)
	if len(placed) != 1 || above[placed[0].Start:placed[0].End] != "nope" {
		t.Fatalf("placed %+v", placed)
	}
	if len(k.Placed("main.typ", "é\n")) != 0 {
		t.Fatal("a deleted diagnostic stayed")
	}
	if got := k.Moved("main.typ", 4, "日 "+text); got != 8 {
		t.Fatalf("moved to %d", got)
	}
}

func TestUnsavedTextShadowsTheFile(t *testing.T) {
	compiler := NewCompiler(vault(t, map[string]string{"main.typ": "Saved.\n"}))
	defer compiler.Close()
	if err := compiler.SetBuffer("main.typ", "#nope\n"); err != nil {
		t.Fatal(err)
	}
	k, err := compiler.Compile("main.typ")
	if err != nil || k.OK {
		t.Fatalf("buffer not read: %v %v", err, k.OK)
	}
	k.Close()
	compiler.ClearBuffer("main.typ")
	k, err = compiler.Compile("main.typ")
	if err != nil || !k.OK {
		t.Fatalf("buffer not cleared: %v %+v", err, k.Diagnostics)
	}
	k.Close()
	k.Close()
	if _, err := k.Render(0, 1); err == nil {
		t.Fatal("a closed compilation rendered")
	}
	if _, err := compiler.Compile("../outside.typ"); err == nil {
		t.Fatal("compiled a path outside the vault")
	}
}

func TestLabelsAreWhatAtCanReference(t *testing.T) {
	text := "= Plain <plain>\n#set heading(numbering: \"1.\")\n= Numbered <numbered>\nText <text>\n$ x $ <equation>\n#figure(rect(), caption: [A]) <figure>\n"
	k := compiled(t, "main.typ", map[string]string{"main.typ": text})
	want := []Label{{Key: "numbered", Title: "Numbered", Kind: "Heading", Page: 1}, {Key: "figure", Title: "A", Kind: "Figure", Page: 1}}
	if !slices.Equal(k.Labels, want) {
		t.Fatalf("labels %+v", k.Labels)
	}
}

func TestCompletionAndHover(t *testing.T) {
	k := compiled(t, "main.typ", map[string]string{"main.typ": "Text.\n", "figures/plot.png": ""})
	found := k.Complete("main.typ", "#headi", 6, false)
	if len(found) == 0 || found[0].Title != "heading" || found[0].From != 1 || found[0].Kind != "Function" {
		t.Fatalf("completion %+v", found)
	}
	for _, s := range found {
		if !strings.Contains(s.Title, "headi") {
			t.Fatalf("%q does not match", s.Title)
		}
	}
	params := k.Complete("main.typ", "#text(fi)", 8, true)
	i := slices.IndexFunc(params, func(s Suggestion) bool { return s.Title == "fill" })
	if i < 0 || params[i].Insert != "fill: " || !slices.Equal(params[i].Fields, [][2]int{{6, 6}}) {
		t.Fatalf("parameters %+v", params)
	}
	paths := k.Complete("main.typ", "#image(\"\")", 8, true)
	if len(paths) != 1 || paths[0].Title != "figures/plot.png" || paths[0].From != 7 {
		t.Fatalf("paths %+v", paths)
	}
	text := "#let total = 1 + 2\n#total\n"
	hover, ok := k.Hover("main.typ", text, strings.LastIndex(text, "total"))
	if !ok || hover != (Hover{Text: "3", Code: true}) {
		t.Fatalf("hover %+v %v", hover, ok)
	}
	hover, ok = k.Hover("main.typ", "#heading[One]\n", 2)
	if !ok || hover.Code || !strings.Contains(hover.Text, "heading") {
		t.Fatalf("hover %+v %v", hover, ok)
	}
	if _, ok := k.Hover("main.typ", "Plain words.\n", 3); ok {
		t.Fatal("plain text has a hover")
	}
}

func TestHighlightAndWords(t *testing.T) {
	text := "= Head\n#let x = 1 // note\n"
	spans := Highlight(text)
	tags := map[string]string{}
	for i, s := range spans {
		if i > 0 && s.Start < spans[i-1].End {
			t.Fatalf("overlap at %+v", s)
		}
		if _, seen := tags[text[s.Start:s.End]]; !seen {
			tags[text[s.Start:s.End]] = s.Tag
		}
	}
	for word, tag := range map[string]string{"=": "heading", "Head": "heading", "let": "keyword", "1": "number", "// note": "comment"} {
		if tags[word] != tag {
			t.Fatalf("%q is %q, want %q: %+v", word, tags[word], tag, spans)
		}
	}
	if got := ProseWords("= A title\nDon't count // this\n*two* words, well-known.\n"); got != 7 {
		t.Fatalf("prose words %d", got)
	}
}

func TestParseDocument(t *testing.T) {
	parsed := ParseDocument("= One <one>\n#include \"../ch/a.typ\"\n== Two\n// = Not\n")
	want := Parsed{Headings: []Heading{{Offset: 0, Level: 1, Title: "One"}, {Offset: 35, Level: 2, Title: "Two"}}, Includes: []Include{{Offset: 21, Literal: "../ch/a.typ"}}}
	if !slices.Equal(parsed.Headings, want.Headings) || !slices.Equal(parsed.Includes, want.Includes) {
		t.Fatalf("parsed %+v", parsed)
	}
}

func TestNoteLinksAndCitations(t *testing.T) {
	text := "#link(\"../b.typ\")[B] #link(\"https://x.org/a.typ\")[web] #link(\"c.pdf\")[pdf]\n#link(\"d.typ\", \"e.typ\")\n`#link(\"raw.typ\")`\n"
	links := NoteLinks(text, "notes/a.typ")
	if len(links) != 2 || links[0].Target != "b.typ" || links[1].Target != "notes/d.typ" || text[links[0].Start:links[0].End] != "#link(\"../b.typ\")[B]" {
		t.Fatalf("links %+v", links)
	}
	if s, e, ok := NoteLinkAt("see [[fla]] x", 9); !ok || s != 4 || e != 11 {
		t.Fatalf("note link at %d %d %v", s, e, ok)
	}
	if _, _, ok := NoteLinkAt("// [[a", 6); ok {
		t.Fatal("a note link in a comment")
	}
	if s, e, ok := CitationAt("See @fig:plot. Then", 8); !ok || s != 4 || e != 13 {
		t.Fatalf("citation at %d %d %v", s, e, ok)
	}
	if _, _, ok := CitationAt("a@fla", 5); ok {
		t.Fatal("an email address is a citation")
	}
	cites := "See @old, @older[p. 2], @old[p. 1] and #cite(<old>). <old>\n`@old` // @old\n"
	if got := CitedKeys(cites); !slices.Equal(got, []string{"old", "older", "old", "old"}) {
		t.Fatalf("cited keys %v", got)
	}
	if got := CitationsOf(cites, "old"); !slices.Equal(got, []int{5, 25, 46}) {
		t.Fatalf("citations of %v", got)
	}
	renamed, ok := RenameCitation(cites, "old", "new", false)
	if !ok || renamed != "See @new, @older[p. 2], @new[p. 1] and #cite(<new>). <old>\n`@old` // @old\n" {
		t.Fatalf("renamed %q", renamed)
	}
	if renamed, _ := RenameCitation("See @old and #cite(<old>).\n", "old", "new", true); renamed != "See @old and #cite(<new>).\n" {
		t.Fatalf("labelled %q", renamed)
	}
	if _, ok := RenameCitation("nothing here", "old", "new", false); ok {
		t.Fatal("renamed nothing")
	}
	if !DefinesLabel("= One <here>\n", "here") || DefinesLabel("See #ref(<here>).\n", "here") {
		t.Fatal("defines label")
	}
}

func TestRewritePathsAfterAMove(t *testing.T) {
	text := "#link(\"../b.typ\")[B] #link(\"https://x.org/a.typ\")[web]\n"
	moved, ok := RewritePaths(text, "notes/a.typ", "notes/a.typ", "notes/a.typ", nil, "b.typ", "archive/b.typ")
	if !ok || !strings.HasPrefix(moved, "#link(\"../archive/b.typ\")[B] #link(\"https://x.org/a.typ\")") {
		t.Fatalf("moved %q", moved)
	}
	files := []string{"Thesis/thesis.typ", "Thesis/template.typ", "Thesis/ch/one.typ", "Thesis/images/a.png"}
	chapter := "#import \"/template.typ\": x\n#image(\"../images/a.png\")\nThe file \"../images/a.png\".\n"
	moved, ok = RewritePaths(chapter, "Thesis/ch/one.typ", "Thesis/one.typ", "Thesis/thesis.typ", files, "Thesis/ch/one.typ", "Thesis/one.typ")
	if !ok || moved != "#import \"/template.typ\": x\n#image(\"images/a.png\")\nThe file \"../images/a.png\".\n" {
		t.Fatalf("moved %q", moved)
	}
	if _, ok := RewritePaths(chapter, "Thesis/ch/one.typ", "Thesis/ch/one.typ", "Thesis/thesis.typ", files, "x", "y"); ok {
		t.Fatal("rewrote with nothing moved")
	}
}

func TestBibliography(t *testing.T) {
	text := EmbedBibliography("Text\n", "a:\n  type: Book\n")
	if BibliographyMode(text, "doc.typ") != BibEmbedded {
		t.Fatal("not embedded")
	}
	styled, ok := SetBibliographyStyle(text, "apa")
	if style, _ := BibliographyStyle(styled); !ok || style != "apa" {
		t.Fatalf("style %q", style)
	}
	synced, ok := SyncBibliographyBlock(styled, "b:\n  type: Book\n")
	if style, _ := BibliographyStyle(synced); !ok || style != "apa" || !strings.Contains(synced, "b:\n") {
		t.Fatalf("synced %q", synced)
	}
	if _, again := SyncBibliographyBlock(synced, "b:\n  type: Book\n"); again {
		t.Fatal("synced an unchanged block")
	}
	start, end, ok := BibliographyBlockRange(synced)
	if !ok || start != len("Text\n\n") || end != len(synced) {
		t.Fatalf("block %d %d %v", start, end, ok)
	}
	if got := WithoutBibliographyBlock(synced); got != "Text\n\n" {
		t.Fatalf("without block %q", got)
	}
	if got := RemoveBibliography(synced, "doc.typ"); got != "Text\n" {
		t.Fatalf("removed %q", got)
	}
	linked := LinkBibliography("Text\n", "ch/doc.typ")
	if linked != "Text\n\n#bibliography(\"../library.yaml\")\n" || BibliographyMode(linked, "ch/doc.typ") != BibLinked {
		t.Fatalf("linked %q", linked)
	}
	if got := RemoveBibliography(linked, "ch/doc.typ"); got != "Text\n\n" {
		t.Fatalf("unlinked %q", got)
	}
	custom := "Text\n#bibliography(\"refs.bib\")\n"
	if BibliographyMode(custom, "doc.typ") != BibCustom || RemoveBibliography(custom, "doc.typ") != custom || BibliographyMode("Text", "doc.typ") != BibNone {
		t.Fatal("custom")
	}
	if got, _ := SetBibliographyStyle("#bibliography(\"a.bib\",)", "apa"); got != "#bibliography(\"a.bib\", style: \"apa\")" {
		t.Fatalf("styled %q", got)
	}
}

func TestFiguresCodeAndImages(t *testing.T) {
	text := "Before\n\n#figure(\n  image(\"a.png\"),\n  caption: [],\n)\n\nAfter"
	caption := strings.Index(text, "[]") + 1
	if got := FigurePlace(text, caption); got != strings.Index(text, ")\n\nAfter")+1 {
		t.Fatalf("figure place %d", got)
	}
	if FigurePlace(text, 3) != 3 {
		t.Fatal("moved outside a figure")
	}
	if !CodeOpens("#calc.", 6) || CodeOpens("The end.", 8) || CodeOpens("// #", 4) {
		t.Fatal("code opens")
	}
	for data, want := range map[string]string{"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR": "png", "GIF89a": "gif", `<svg xmlns="http://www.w3.org/2000/svg"></svg>`: "svg", "%PDF-1.4\n": "pdf"} {
		if got, ok := ImageFormat([]byte(data)); !ok || got != want {
			t.Fatalf("%q read as %q, want %q", data, got, want)
		}
	}
	if _, ok := ImageFormat([]byte("text")); ok {
		t.Fatal("text is an image")
	}
	if _, ok := ImageFormat([]byte{0x1f, 0x8b, 8, 0}); ok {
		t.Fatal("gzip is an image")
	}
}

func TestLibrary(t *testing.T) {
	if CheckLibrary("") != nil || CheckLibrary("a:\n  type: Book\n  title: A\n") != nil {
		t.Fatal("a good library was refused")
	}
	for _, bad := range []string{"a: [unclosed\n", "a:\n  type: Book\n  date: not a date\n"} {
		if CheckLibrary(bad) == nil {
			t.Fatalf("accepted %q", bad)
		}
	}
	entries, err := LibraryEntries("a:\n  type: Article\n  title: T\n  author: [\"Doe, Jane\", Plato]\n  date: 2020-03\n  parent:\n    type: Periodical\n    title: J\n")
	if err != nil || len(entries) != 1 {
		t.Fatal(err, entries)
	}
	e := entries[0]
	if e.Key != "a" || e.Title != "T" || !slices.Equal(e.Authors, []string{"Doe, Jane", "Plato"}) || e.Date != "2020-03" || e.ParentTitle != "J" {
		t.Fatalf("entry %+v", e)
	}
	if _, err := LibraryEntries("a: [unclosed\n"); err == nil {
		t.Fatal("entries of a broken library")
	}
}

func TestImportBiblatex(t *testing.T) {
	imported, err := ImportBiblatex(`
@article{parry2022, title = {Flaky}, author = {Parry, Owain}, journal = {TOSEM}, year = {2022}, language = {en}, keywords = {testing, flaky tests}}
@book{knuth1984, title = {TeXbook}, author = {Knuth, Donald}, publisher = {AW}, year = {1984}}
`)
	if err != nil {
		t.Fatal(err)
	}
	entries, err := LibraryEntries(imported.YAML)
	if err != nil || len(entries) != 2 {
		t.Fatal(err, entries)
	}
	if got := imported.Verbatim["parry2022"]; got["keywords"] != "testing, flaky tests" || got["language"] != "en" {
		t.Fatalf("verbatim %+v", imported.Verbatim)
	}
	if _, ok := imported.Verbatim["knuth1984"]; ok {
		t.Fatal("fields for an entry without them")
	}
	if _, err := ImportBiblatex("@book{a, title = {A}"); err == nil {
		t.Fatal("imported broken BibLaTeX")
	}
}
