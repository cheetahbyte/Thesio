package vault

import (
	"slices"
	"testing"
)

func TestAFolderContainsOnlyItsOwnChildren(t *testing.T) {
	if !IsWithin("a/b.typ", "a") || !IsWithin("a", "a") || IsWithin("ab/c.typ", "a") {
		t.Fatal("IsWithin")
	}
}

func TestAMoveCarriesAFolderAndWhatItHoldsAndNothingElse(t *testing.T) {
	for _, c := range []struct {
		path, from, to, want string
		ok                   bool
	}{
		{"a/b.typ", "a", "x/y", "x/y/b.typ", true},
		{"a", "a", "b", "b", true},
		{"ab", "a", "b", "", false},
	} {
		if got, ok := MovedPath(c.path, c.from, c.to); got != c.want || ok != c.ok {
			t.Errorf("MovedPath(%q, %q, %q) = %q, %v", c.path, c.from, c.to, got, ok)
		}
	}
}

func TestADocumentIsNamedByItsExtension(t *testing.T) {
	if !IsDocument("a/b.typ") || IsDocument("b.pdf") || IsDocument("typ") {
		t.Fatal("IsDocument")
	}
	if DocumentTitle("b.typ") != "b" || DocumentTitle("b.pdf") != "b.pdf" {
		t.Fatal("DocumentTitle")
	}
}

func TestNamesSortAsPeopleReadThem(t *testing.T) {
	names := []string{"chapter 10", "Chapter 2", "chapter 02", "b", "A"}
	slices.SortFunc(names, NaturalCompare)
	if want := []string{"A", "b", "Chapter 2", "chapter 02", "chapter 10"}; !slices.Equal(names, want) {
		t.Fatalf("%q", names)
	}
}

func TestALiteralResolvesInsideTheVaultOnly(t *testing.T) {
	for _, c := range []struct {
		dir           []string
		literal, want string
		ok            bool
	}{
		{[]string{"a", "b"}, "../c.typ", "a/c.typ", true},
		{[]string{"a"}, "/x/./y.typ", "x/y.typ", true},
		{nil, "../x.typ", "", false},
		{nil, "@preview/a:1.0.0", "", false},
		{nil, "https://x.org/a.png", "", false},
	} {
		if got, ok := ResolveLiteral(c.dir, c.literal); got != c.want || ok != c.ok {
			t.Errorf("ResolveLiteral(%q, %q) = %q, %v", c.dir, c.literal, got, ok)
		}
	}
	if got := RelativeTo([]string{"a", "b"}, "a/c/d.typ"); got != "../c/d.typ" {
		t.Fatal(got)
	}
}

func TestMarkupEscapesWhatWouldParse(t *testing.T) {
	for in, want := range map[string]string{"1. a": `1\. a`, "- x": `\- x`, "a -- b": `a \-\- b`, "#x <y>": `\#x \<y\>`, "a...": `a\...`, "a/b": "a/b", "// c": `\// c`, "a // c": `a \// c`} {
		if got := EscapeMarkup(in); got != want {
			t.Errorf("EscapeMarkup(%q) = %q, want %q", in, got, want)
		}
	}
	if Quoted(`a "b" \c`) != `"a \"b\" \\c"` {
		t.Fatal("Quoted")
	}
}
