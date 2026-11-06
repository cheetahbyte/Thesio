package vault

import (
	"bytes"
	"cmp"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

const (
	trashDir = ".trash"
	// maxNumbered is how far numbering a taken name goes before giving up.
	maxNumbered  = 10_000
	formatFile   = "format.json"
	indexFile    = "index.json"
	trashFile    = "trash.json"
	settingsFile = "settings.json"
)

const Untitled = "Untitled"

// TrashDays is how long the Trash keeps an item.
const TrashDays = 30

// Format is the version of `.thesio/`. Bump it when older builds would misread the vault.
const Format = 1

// DisplayTitle is how a title reads: trimmed, and never blank.
func DisplayTitle(title string) string {
	if title = strings.TrimSpace(title); title == "" {
		return Untitled
	}
	return title
}

// Meta is app state for a vault path. Timestamps come from the file; this holds
// what a plain file cannot.
type Meta struct {
	Pinned bool  `json:"pinned,omitempty"`
	Opened int64 `json:"opened,omitempty"`
	// Starred is the star time, 0 when not starred.
	Starred int64 `json:"starred,omitempty"`
	// PageOffset is a PDF's printed page numbers minus the ones it reports.
	PageOffset *int `json:"page_offset,omitempty"`
}

// Settings is window-level UI state (`.thesio/settings.json`).
type Settings struct {
	// SidebarWidth is the dragged sidebar width; nil is the default.
	SidebarWidth *float32    `json:"sidebar_width,omitempty"`
	WindowSize   *[2]float32 `json:"window_size,omitempty"`
	// Expanded are the folders open in the sidebar.
	Expanded []string `json:"expanded,omitempty"`
}

// trashed is a trashed file or folder, as kept in `.thesio/trash.json`.
type trashed struct {
	Meta    Meta  `json:"meta"`
	Deleted int64 `json:"deleted"`
	// Original is its vault path before deletion.
	Original string `json:"original"`
	// Inner is app state of the files inside a folder, by path relative to it.
	Inner map[string]Meta `json:"inner,omitempty"`
}

// TrashEntry is a row of the Trash page; ID is its key in the Trash, not a vault path.
type TrashEntry struct {
	ID, Title string
	Deleted   int64
	// Parent is the folder it was in, "" for the vault root.
	Parent string
}

type Entry struct {
	ID   string
	Meta Meta
	// Modified is the file's own time.
	Modified  int64
	Parent    string
	HasParent bool
	Title     string
	Folder    bool
	// Typst is a `.typ` document, as opposed to a folder or another kind of file.
	Typst bool
	// Body is a document's text, for search and snippets.
	Body string
}

// Moved is a rename or move that went through.
type Moved struct {
	ID string
	// Rewritten are the documents whose paths were updated, with their new text.
	Rewritten []Rewritten
	// Warnings are what went wrong without stopping it.
	Warnings []string
}

type Rewritten struct{ ID, Text string }

type Restored struct {
	// Renamed: it came back under a new name because its old one was taken.
	Renamed  bool
	Warnings []string
}

// Rewriter rewrites paths after a move, which needs Typst's parser; the app wires it in.
type Rewriter interface {
	// TopDocument is the top-level document id belongs to, among ids, reading texts with read.
	TopDocument(ids []string, read func(id string) (string, bool), id string) string
	// RewritePaths as typst.RewritePaths.
	RewritePaths(text, oldFile, newFile, main string, files []string, from, to string) (string, bool)
}

type cachedBody struct {
	modified, size int64
	text           string
}

// Store is an open vault. It is not safe for concurrent use.
type Store struct {
	root     string
	index    map[string]Meta
	trash    map[string]trashed
	bodies   map[string]cachedBody
	rewriter Rewriter
}

type formatMarker struct {
	Format int `json:"format"`
}

// loadJSON reads a file written by saveJSON into v. One that does not parse is
// set aside as `<name>.corrupt-<time>` and its `.bak` is used instead.
func loadJSON[T any](path, what string, warnings *[]string) T {
	var v T
	data, err := os.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return v
	}
	if err != nil {
		*warnings = append(*warnings, fmt.Sprintf("Could not read %s: %v", what, err))
		return v
	}
	if json.Unmarshal(data, &v) == nil {
		return v
	}
	aside := fmt.Sprintf("%s.corrupt-%d", path, Now())
	os.Rename(path, aside)
	name := filepath.Base(aside)
	var backup T
	if data, err := os.ReadFile(path + ".bak"); err == nil && json.Unmarshal(data, &backup) == nil {
		*warnings = append(*warnings, fmt.Sprintf("%s was damaged and restored from backup (original kept as %s)", what, name))
		return backup
	}
	*warnings = append(*warnings, fmt.Sprintf("%s was damaged with no usable backup, so pins and stars were reset (original kept as %s)", what, name))
	var zero T
	return zero
}

func saveJSON(path string, v any) error {
	var out bytes.Buffer
	encoder := json.NewEncoder(&out)
	encoder.SetEscapeHTML(false)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(v); err != nil {
		return err
	}
	if old, err := os.ReadFile(path); err == nil {
		if err := WriteAtomic(path+".bak", old); err != nil {
			return err
		}
	} else if !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	return WriteAtomic(path, bytes.TrimSuffix(out.Bytes(), []byte("\n")))
}

// fileStem is a file name for a title: no separators or leading dots.
func fileStem(title string) string {
	clean := strings.Map(func(c rune) rune {
		if strings.ContainsRune(`/\:*?"<>|`, c) || unicode.IsControl(c) {
			return '-'
		}
		return c
	}, title)
	clean = strings.TrimSpace(strings.TrimLeft(strings.TrimSpace(clean), "."))
	if clean == "" {
		return Untitled
	}
	return clean
}

// renamedFile is a file name from a title. A document's title may leave out
// the extension; other names are whole.
func renamedFile(title string, typst bool) string {
	stem := fileStem(title)
	if !typst {
		return stem
	}
	return DocumentName(DocumentTitle(stem))
}

// numberedName is stem's nth name, for when the ones before it are taken.
func numberedName(stem, ext string, n int) string {
	name := stem
	if n > 0 {
		name = fmt.Sprintf("%s %d", stem, n)
	}
	if ext != "" {
		name += "." + ext
	}
	return name
}

func exists(path string) (bool, error) {
	_, err := os.Stat(path)
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	return err == nil, err
}

func copyFile(from, to string) error {
	in, err := os.Open(from)
	if err != nil {
		return err
	}
	defer in.Close()
	info, err := in.Stat()
	if err != nil {
		return err
	}
	out, err := os.OpenFile(to, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, info.Mode().Perm())
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
		out.Close()
		return err
	}
	return out.Close()
}

// RootDir is THESIO_HOME, else a folder of its own in the user's Documents.
func RootDir() string {
	if home := os.Getenv("THESIO_HOME"); home != "" {
		return home
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "Thesio"
	}
	return filepath.Join(home, "Documents", "Thesio")
}

// Open opens the vault at RootDir, with problems found opening it, for the window to show once.
func Open() (*Store, []string, error) { return OpenAt(RootDir()) }

func OpenAt(root string) (*Store, []string, error) {
	if err := os.MkdirAll(filepath.Join(root, StateDir), 0o755); err != nil {
		return nil, nil, err
	}
	state := func(file string) string { return filepath.Join(root, StateDir, file) }
	var warnings []string
	switch marker := loadJSON[*formatMarker](state(formatFile), "the vault format", &warnings); {
	case marker == nil:
		if err := saveJSON(state(formatFile), formatMarker{Format: Format}); err != nil {
			return nil, nil, err
		}
	case marker.Format > Format:
		warnings = append(warnings, "This vault was created by a newer version of Thesio. Update the app before editing it.")
	}
	s := &Store{
		root:   root,
		index:  loadJSON[map[string]Meta](state(indexFile), "the vault index", &warnings),
		trash:  loadJSON[map[string]trashed](state(trashFile), "the Trash index", &warnings),
		bodies: map[string]cachedBody{},
	}
	if s.index == nil {
		s.index = map[string]Meta{}
	}
	if s.trash == nil {
		s.trash = map[string]trashed{}
	}
	return s, warnings, nil
}

// SetRewriter wires in path rewriting after a move; nil skips it.
func (s *Store) SetRewriter(r Rewriter) { s.rewriter = r }

func (s *Store) Root() string { return s.root }

// Path is the file behind id, refusing paths that leave the vault.
func (s *Store) Path(id string) (string, error) {
	if id == "" {
		return "", fmt.Errorf("Invalid vault path: %s", id)
	}
	for _, segment := range strings.Split(id, "/") {
		if segment == "" || segment == "." || segment == ".." {
			return "", fmt.Errorf("Invalid vault path: %s", id)
		}
	}
	return filepath.Join(s.root, filepath.FromSlash(id)), nil
}

func (s *Store) stateFile(file string) string { return filepath.Join(s.root, StateDir, file) }
func (s *Store) saveIndex() error             { return saveJSON(s.stateFile(indexFile), s.index) }
func (s *Store) saveTrash() error             { return saveJSON(s.stateFile(trashFile), s.trash) }

// List is every visible file and folder, parents before children.
func (s *Store) List() []Entry {
	var out []Entry
	Walk(s.root, func(item Item) { out = append(out, s.entry(item)) })
	return out
}

func (s *Store) entry(item Item) Entry {
	e := Entry{ID: item.ID, Meta: s.index[item.ID], Modified: item.Modified(), Parent: item.Parent, HasParent: item.HasParent, Title: item.Name, Folder: item.Info.IsDir()}
	if !item.IsDocument() {
		return e
	}
	e.Typst, e.Title = true, DocumentTitle(item.Name)
	size := item.Info.Size()
	if cached, ok := s.bodies[item.ID]; ok && cached.modified == e.Modified && cached.size == size {
		e.Body = cached.text
		return e
	}
	if data, err := os.ReadFile(item.Path); err == nil && utf8.Valid(data) {
		e.Body = string(data)
	}
	s.bodies[item.ID] = cachedBody{modified: e.Modified, size: size, text: e.Body}
	return e
}

func (s *Store) Load(id string) (string, error) {
	path, err := s.Path(id)
	if err != nil {
		return "", err
	}
	data, err := os.ReadFile(path)
	if err == nil && !utf8.Valid(data) {
		err = errors.New("not valid UTF-8")
	}
	if err != nil {
		return "", fmt.Errorf("Could not read %s: %w", id, err)
	}
	return string(data), nil
}

func (s *Store) Save(id, text string) error {
	// A second write can leave the time and size List compares unchanged.
	delete(s.bodies, id)
	path, err := s.Path(id)
	if err != nil {
		return err
	}
	return WriteAtomic(path, []byte(text))
}

// Versions is id's earlier versions.
func (s *Store) Versions(id string) (*Versions, error) {
	if _, err := s.Path(id); err != nil {
		return nil, err
	}
	return VersionsOf(s.root, id), nil
}

// freeID is a free name for stem in parent; ext "" for none.
func (s *Store) freeID(parent, stem, ext string) (string, error) {
	for n := range maxNumbered {
		id := Join(parent, numberedName(stem, ext, n))
		path, err := s.Path(id)
		if err != nil {
			return "", err
		}
		taken, err := exists(path)
		if err != nil {
			return "", err
		}
		if !taken {
			return id, nil
		}
	}
	return "", fmt.Errorf("Too many files named %s", stem)
}

// Create makes an empty document in parent, "" for the vault root.
func (s *Store) Create(parent string) (string, error) { return s.CreateFrom(parent, Untitled, "") }

func (s *Store) CreateFrom(parent, title, text string) (string, error) {
	id, err := s.freeID(parent, fileStem(title), Extension)
	if err != nil {
		return "", err
	}
	return id, s.Save(id, text)
}

func (s *Store) CreateFolder(parent, title string) (string, error) {
	id, err := s.freeID(parent, fileStem(title), "")
	if err != nil {
		return "", err
	}
	path, err := s.Path(id)
	if err != nil {
		return "", err
	}
	return id, os.Mkdir(path, 0o755)
}

// layout is the vault as the compiler saw it before a move.
type layout struct {
	documents []string
	bodies    map[string]string
	files     []string
}

func (s *Store) layout() layout {
	l := layout{bodies: map[string]string{}}
	for _, e := range s.List() {
		if e.Typst {
			l.documents = append(l.documents, e.ID)
			l.bodies[e.ID] = e.Body
		}
		if !e.Folder {
			l.files = append(l.files, e.ID)
		}
	}
	return l
}

// relocate moves old to new, carrying app state, including for a folder's
// files, and updates the paths in vault documents.
func (s *Store) relocate(old, new string) (Moved, error) {
	outcome := Moved{ID: new}
	if old == new {
		return outcome, nil
	}
	from, err := s.Path(old)
	if err != nil {
		return outcome, err
	}
	target, err := s.Path(new)
	if err != nil {
		return outcome, err
	}
	// A change of case alone names the same file on case-insensitive disks.
	if strings.ToLower(old) != strings.ToLower(new) {
		taken, err := exists(target)
		if err != nil {
			return outcome, err
		}
		if taken {
			return outcome, fmt.Errorf("%s already exists", new)
		}
	}
	var before layout
	if s.rewriter != nil {
		before = s.layout()
	}
	if err := os.Rename(from, target); err != nil {
		return outcome, err
	}
	// A file's versions follow it. Losing them must not fail the move.
	if err := VersionsOf(s.root, old).moveTo(VersionsOf(s.root, new)); err != nil {
		outcome.Warnings = append(outcome.Warnings, fmt.Sprintf("Could not move the history of %s: %v", old, err))
	}
	for key, meta := range maps.Clone(s.index) {
		if renamed, ok := MovedPath(key, old, new); ok {
			delete(s.index, key)
			s.index[renamed] = meta
		}
	}
	if err := s.saveIndex(); err != nil {
		return outcome, err
	}
	if s.rewriter != nil {
		s.rewriteReferences(old, new, before, &outcome)
	}
	return outcome, nil
}

// rewriteReferences fixes paths that the move from old to new broke. A file
// that cannot be rewritten keeps its text and raises a warning.
func (s *Store) rewriteReferences(old, new string, before layout, outcome *Moved) {
	read := func(id string) (string, bool) { body, ok := before.bodies[id]; return body, ok }
	for _, e := range s.List() {
		if !e.Typst {
			continue
		}
		text, err := s.Load(e.ID)
		if err != nil {
			outcome.Warnings = append(outcome.Warnings, fmt.Sprintf("Could not update paths in %s: %v", e.ID, err))
			continue
		}
		previous, ok := MovedPath(e.ID, new, old)
		if !ok {
			previous = e.ID
		}
		main := s.rewriter.TopDocument(before.documents, read, previous)
		updated, ok := s.rewriter.RewritePaths(text, previous, e.ID, main, before.files, old, new)
		if !ok {
			continue
		}
		if err := s.Save(e.ID, updated); err != nil {
			outcome.Warnings = append(outcome.Warnings, fmt.Sprintf("Could not update paths in %s: %v", e.ID, err))
			continue
		}
		outcome.Rewritten = append(outcome.Rewritten, Rewritten{ID: e.ID, Text: updated})
	}
}

// Attach copies the outside file from to id, never over an existing file.
func (s *Store) Attach(from, id string) error {
	target, err := s.Path(id)
	if err != nil {
		return err
	}
	taken, err := exists(target)
	if err != nil {
		return err
	}
	if taken {
		return fmt.Errorf("%s already exists", id)
	}
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return err
	}
	return copyFile(from, target)
}

// AddImage copies image into `images/` beside document and returns its path as
// the document writes it. A file with the same bytes is used as it is, and no
// other file is replaced.
func (s *Store) AddImage(document string, image Incoming) (string, error) {
	img, err := image.read()
	if err != nil {
		return "", err
	}
	parent, _ := ParentOf(document)
	folder := Join(parent, ImagesDir)
	stem := fileStem(img.stem)
	dir, err := s.Path(folder)
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return "", err
	}
	for n := range maxNumbered {
		id := Join(folder, numberedName(stem, img.ext, n))
		path, err := s.Path(id)
		if err != nil {
			return "", err
		}
		same := false
		found, err := os.Lstat(path)
		switch {
		case errors.Is(err, fs.ErrNotExist):
			if err := WriteAtomic(path, img.data); err != nil {
				return "", err
			}
			same = true
		case err != nil:
			return "", err
		case found.Mode().IsRegular() && found.Size() == int64(len(img.data)):
			data, err := os.ReadFile(path)
			if err != nil {
				return "", err
			}
			same = bytes.Equal(data, img.data)
		}
		if same {
			return RelativeTo(DirParts(document), id), nil
		}
	}
	return "", fmt.Errorf("Too many files named %s", stem)
}

// Rename gives id a new name from title, in the same folder.
func (s *Store) Rename(id, title string) (Moved, error) {
	path, err := s.Path(id)
	if err != nil {
		return Moved{}, err
	}
	info, err := os.Stat(path)
	typst := err == nil && info.Mode().IsRegular() && IsDocument(id)
	parent, _ := ParentOf(id)
	return s.relocate(id, Join(parent, renamedFile(title, typst)))
}

// SetParent moves id into parent, "" for the vault root.
func (s *Store) SetParent(id, parent string) (Moved, error) {
	if parent != "" && IsWithin(parent, id) {
		return Moved{}, errors.New("A folder cannot move into itself")
	}
	return s.relocate(id, Join(parent, BaseName(id)))
}

// Delete moves a file or folder to the Trash. Returns what went wrong without stopping it.
func (s *Store) Delete(id string) ([]string, error) {
	from, err := s.Path(id)
	if err != nil {
		return nil, err
	}
	stamp := NowMillis()
	// Items deleted together can share a millisecond.
	for {
		key := strconv.FormatInt(stamp, 10)
		_, listed := s.trash[key]
		taken, _ := exists(filepath.Join(s.root, trashDir, key))
		if !listed && !taken {
			break
		}
		stamp++
	}
	key := strconv.FormatInt(stamp, 10)
	dir := filepath.Join(s.root, trashDir, key)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	// Record it first: an interrupted move leaves an extra entry, never a lost file.
	inner := map[string]Meta{}
	for k, m := range s.index {
		if rest, ok := strings.CutPrefix(k, id+"/"); ok {
			inner[rest] = m
		}
	}
	s.trash[key] = trashed{Meta: s.index[id], Deleted: Now(), Original: id, Inner: inner}
	if err := s.saveTrash(); err != nil {
		return nil, err
	}
	if err := os.Rename(from, filepath.Join(dir, BaseName(id))); err != nil {
		delete(s.trash, key)
		s.saveTrash()
		return nil, err
	}
	maps.DeleteFunc(s.index, func(k string, _ Meta) bool { return IsWithin(k, id) })
	if err := s.saveIndex(); err != nil {
		return []string{fmt.Sprintf("Moved to Trash, but the vault index could not be cleaned up: %v", err)}, nil
	}
	return nil, nil
}

// PurgeExpired permanently deletes what is past its time in the Trash. Returns what could not be.
func (s *Store) PurgeExpired() []string {
	cutoff := max(Now()-TrashDays*Day, 0)
	var warnings []string
	for key, t := range maps.Clone(s.trash) {
		if t.Deleted >= cutoff {
			continue
		}
		if err := s.Purge(key); err != nil {
			warnings = append(warnings, fmt.Sprintf("Could not empty Trash: %v", err))
		}
	}
	return warnings
}

// ListTrash is the Trash contents, newest first.
func (s *Store) ListTrash() []TrashEntry {
	out := make([]TrashEntry, 0, len(s.trash))
	for key, t := range s.trash {
		parent, _ := ParentOf(t.Original)
		out = append(out, TrashEntry{ID: key, Title: DocumentTitle(BaseName(t.Original)), Deleted: t.Deleted, Parent: parent})
	}
	slices.SortFunc(out, func(a, b TrashEntry) int { return cmp.Or(cmp.Compare(b.Deleted, a.Deleted), cmp.Compare(a.ID, b.ID)) })
	return out
}

// Restore puts a trashed item back, under a new name when its old one is taken.
func (s *Store) Restore(key string) (Restored, error) {
	entry, ok := s.trash[key]
	if !ok {
		return Restored{}, errors.New("Not in Trash")
	}
	original := entry.Original
	parent, hasParent := ParentOf(original)
	path, err := s.Path(original)
	if err != nil {
		return Restored{}, err
	}
	taken, err := exists(path)
	if err != nil {
		return Restored{}, err
	}
	target := original
	if taken {
		name := BaseName(original)
		stem, ext := name, ""
		if i := strings.LastIndexByte(name, '.'); i >= 0 {
			stem, ext = name[:i], name[i+1:]
		}
		if target, err = s.freeID(parent, stem, ext); err != nil {
			return Restored{}, err
		}
	}
	if hasParent {
		dir, err := s.Path(parent)
		if err != nil {
			return Restored{}, err
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return Restored{}, err
		}
	}
	dir := filepath.Join(s.root, trashDir, key)
	to, err := s.Path(target)
	if err != nil {
		return Restored{}, err
	}
	if err := os.Rename(filepath.Join(dir, BaseName(original)), to); err != nil {
		return Restored{}, err
	}
	os.Remove(dir)
	for relative, meta := range entry.Inner {
		s.index[target+"/"+relative] = meta
	}
	s.index[target] = entry.Meta
	delete(s.trash, key)
	if err := s.saveIndex(); err != nil {
		return Restored{}, err
	}
	restored := Restored{Renamed: taken}
	if err := s.saveTrash(); err != nil {
		restored.Warnings = append(restored.Warnings, fmt.Sprintf("Restored, but the Trash index could not be cleaned up: %v", err))
	}
	return restored, nil
}

func (s *Store) Purge(key string) error {
	if _, ok := s.trash[key]; !ok {
		return errors.New("Not in Trash")
	}
	if err := os.RemoveAll(filepath.Join(s.root, trashDir, key)); err != nil {
		return err
	}
	delete(s.trash, key)
	return s.saveTrash()
}

// Import copies a file into the vault root and returns its vault path.
func (s *Store) Import(path string) (string, error) {
	stem, ext, _ := splitName(filepath.Base(path))
	id, err := s.freeID("", fileStem(stem), ext)
	if err != nil {
		return "", err
	}
	to, err := s.Path(id)
	if err != nil {
		return "", err
	}
	return id, copyFile(path, to)
}

func (s *Store) UpdateMeta(id string, f func(*Meta)) error {
	meta := s.index[id]
	f(&meta)
	s.index[id] = meta
	return s.saveIndex()
}

func (s *Store) Settings() Settings {
	var warnings []string
	return loadJSON[Settings](s.stateFile(settingsFile), "settings", &warnings)
}

// UpdateSettings changes the settings as they are on disk: other windows and copies write them too.
func (s *Store) UpdateSettings(f func(*Settings)) error {
	settings := s.Settings()
	f(&settings)
	return saveJSON(s.stateFile(settingsFile), settings)
}

// Touch records that a document was opened.
func (s *Store) Touch(id string) error {
	now := Now()
	return s.UpdateMeta(id, func(m *Meta) { m.Opened = now })
}

func (s *Store) SetStarred(id string, on bool) (int64, error) {
	var time int64
	if on {
		time = Now()
	}
	return time, s.UpdateMeta(id, func(m *Meta) { m.Starred = time })
}

func (s *Store) SetPageOffset(id string, n int) error {
	return s.UpdateMeta(id, func(m *Meta) {
		m.PageOffset = nil
		if n != 0 {
			m.PageOffset = &n
		}
	})
}

func (s *Store) SetPinned(id string, on bool) error {
	return s.UpdateMeta(id, func(m *Meta) { m.Pinned = on })
}
