// Package vault is the vault on disk: a plain folder of `.typ` files, by default
// ~/Documents/Thesio. A document's id is its vault-relative path with `/` between
// folders, so renaming or moving a file changes its id. App state lives beside the
// files in `.thesio/`, and deleted files move to `.trash/`.
package vault

import (
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"unicode"
)

// StateDir holds app state beside the files.
const StateDir = ".thesio"
const Extension = "typ"

// LibraryFile is the citation library, at the vault root.
const LibraryFile = "library.yaml"

// PDFHeader is how every PDF starts.
const PDFHeader = "%PDF-"

// WriteAtomic writes data to path through a temporary file, so a crash leaves
// either the old or the new contents.
func WriteAtomic(path string, data []byte) error {
	tmp := path + ".tmp"
	file, err := os.Create(tmp)
	if err != nil { return err }
	if _, err := file.Write(data); err != nil { file.Close(); return err }
	if err := file.Sync(); err != nil { file.Close(); return err }
	if err := file.Close(); err != nil { return err }
	return os.Rename(tmp, path)
}

// NaturalCompare orders names as people read them: ignoring case, with runs of
// digits compared by value, so "Chapter 2" sorts before "Chapter 10".
func NaturalCompare(a, b string) int {
	x, y := []rune(a), []rune(b)
	i, j := 0, 0
	for {
		switch {
		case i == len(x) && j == len(y): return strings.Compare(a, b)
		case i == len(x): return -1
		case j == len(y): return 1
		}
		p, q := x[i], y[j]
		if isDigit(p) && isDigit(q) {
			m, n := digits(x, &i), digits(y, &j)
			if len(m) != len(n) { if len(m) < len(n) { return -1 }; return 1 }
			if order := strings.Compare(m, n); order != 0 { return order }
			continue
		}
		if p, q := unicode.ToLower(p), unicode.ToLower(q); p != q { if p < q { return -1 }; return 1 }
		i++
		j++
	}
}

// NaturalLess is NaturalCompare(a, b) < 0.
func NaturalLess(a, b string) bool { return NaturalCompare(a, b) < 0 }

func isDigit(c rune) bool { return c >= '0' && c <= '9' }

// digits takes the run of digits at *i, without leading zeros.
func digits(s []rune, i *int) string {
	start := *i
	for *i < len(s) && isDigit(s[*i]) { *i++ }
	return strings.TrimLeft(string(s[start:*i]), "0")
}

func IsPDF(name string) bool { return strings.HasSuffix(name, ".pdf") }

// ParentOf is the folder holding path, false at the vault root.
func ParentOf(path string) (string, bool) {
	i := strings.LastIndexByte(path, '/')
	if i < 0 { return "", false }
	return path[:i], true
}

func BaseName(path string) string { return path[strings.LastIndexByte(path, '/')+1:] }

// Join puts name in parent; parent "" is the vault root.
func Join(parent, name string) string {
	if parent == "" { return name }
	return parent + "/" + name
}

func joinParent(parent string, has bool, name string) string {
	if !has { return name }
	return parent + "/" + name
}

// DirParts are the folders above path, as segments from the top.
func DirParts(path string) []string {
	parts := strings.Split(path, "/")
	return parts[:len(parts)-1]
}

// IsWithin reports whether path is ancestor or lies inside it.
func IsWithin(path, ancestor string) bool {
	rest, ok := strings.CutPrefix(path, ancestor)
	return ok && (rest == "" || rest[0] == '/')
}

// MovedPath is where path ends up when from moves to to, if that moves it.
func MovedPath(path, from, to string) (string, bool) {
	if !IsWithin(path, from) { return "", false }
	return to + path[len(from):], true
}

func documentStem(name string) (string, bool) {
	i := strings.LastIndexByte(name, '.')
	if i < 0 || name[i+1:] != Extension { return "", false }
	return name[:i], true
}

func IsDocument(name string) bool { _, ok := documentStem(name); return ok }

// DocumentTitle is a file's title: its name, without the extension if it is a document.
func DocumentTitle(name string) string {
	if stem, ok := documentStem(name); ok { return stem }
	return name
}

func DocumentName(stem string) string { return stem + "." + Extension }

// splitName splits a file name as a path's stem and extension, a leading dot
// belonging to the stem.
func splitName(name string) (stem, ext string, ok bool) {
	i := strings.LastIndexByte(name, '.')
	if i <= 0 || name == ".." { return name, "", false }
	return name[:i], name[i+1:], true
}

// Item is a visible file or folder of a vault.
type Item struct {
	ID, Name  string
	Parent    string
	HasParent bool
	Path      string
	Info      fs.FileInfo
}

func (i Item) IsDocument() bool { return !i.Info.IsDir() && IsDocument(i.Name) }

// Modified is the file's time in Unix seconds, 0 when unknown.
func (i Item) Modified() int64 { return max(i.Info.ModTime().Unix(), 0) }

// Walk visits every visible file and folder under root, parents before
// children, the children of a folder in byte order of their names.
func Walk(root string, visit func(item Item)) { walk(root, "", false, visit) }

func walk(dir, parent string, has bool, visit func(Item)) {
	items, _ := os.ReadDir(dir)
	for _, item := range items {
		name := item.Name()
		info, err := item.Info()
		if err != nil || strings.HasPrefix(name, ".") { continue }
		id, path := joinParent(parent, has, name), filepath.Join(dir, name)
		visit(Item{ID: id, Name: name, Parent: parent, HasParent: has, Path: path, Info: info})
		if info.IsDir() { walk(path, id, true, visit) }
	}
}
