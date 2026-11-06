# Architecture

Thesio is a Go desktop application built on [MyGo](https://mygo.egoist.dev) native UI,
with Typst compiled as a Rust static library behind a C ABI. It edits plain `.typ`
files in a vault and shows a live preview compiled by Typst.

```
main.go                       the app: window, menus, launch
mygo.json                     app name, identifier, bundle settings
typst/                        Rust crate `thesio-typst`: Typst, typst-ide, Hayagriva, behind a C ABI
  include/thesio_typst.h      the ABI, the one place its functions are declared
internal/typst                Go bindings (cgo) over that ABI: Compiler, Compilation, parser helpers
internal/vault                the vault on disk: files, folders, Trash, app state, versions, images
internal/project              a document and the files it includes: outline, includers, launch target
internal/library              citations: library.yaml, source types, BibLaTeX, identifiers, lookup, quotes
internal/pdfkit               macOS PDFKit through purego: page labels, text, highlight annotations
internal/app                  the workspace: views, sidebar, home, search, trash, settings, theme, icons
internal/app/document         the open document: editor, preview, compile session, completion, images
internal/app/citations        the Library table, inspector, tags, picker, lookup, bibliographies
internal/app/papers           Sources, the paper viewer, notes, quotes
scripts/check.sh              cargo test (typst/) + go vet + go test
```

Dependencies point downward: `internal/app/*` depends on the engine packages, the engine
packages depend on `internal/typst` and the standard library only, and `internal/typst`
depends on the Rust crate. Nothing under `internal/` except `internal/app` imports MyGo.

## Why a Rust library

Typst is a Rust project. The crate in `typst/` is the smallest Rust surface that still
owns everything that needs Typst's parser or compiler: compiling, rendering pages,
exporting PDF, `typst-ide` completion and hover, syntax highlighting, and the source
rewrites that must understand Typst syntax (paths after a move, citations, note links,
the bibliography block). Hayagriva lives there too, for validating `library.yaml`,
reading entries as Typst reads them, and parsing BibLaTeX. The crate is built once
(`cargo build --release` in `typst/`) into `libthesio_typst.a`; Go links it with cgo,
so Go rebuilds stay fast while the Rust side does not change.

Rules of the ABI (`typst/include/thesio_typst.h`):

- Every `const char*` going in is NUL-terminated UTF-8. Every `char*` coming out is
  owned by the caller and freed with `thesio_string_free`; byte buffers with
  `thesio_bytes_free`.
- Structured results are JSON. A function that can fail returns a JSON object with
  either its result or `{"error": "..."}`. Offsets are bytes of UTF-8.
- Handles (`ThesioCompiler*`, `ThesioCompilation*`) are opaque. A `Compilation` is
  immutable and safe to use from any thread; the `Compiler` is used from one goroutine
  at a time (the Go side serializes).
- Nothing in the crate reaches the network except `thesio_compilation_fetch`
  (package downloads) which the caller invokes deliberately.

## Package contracts

These are the APIs the packages expose to one another. Names here are binding; a
package may add more.

### internal/typst

```go
type Compiler struct{ /* opaque */ }
func NewCompiler(root string) *Compiler
func (c *Compiler) Close()
func (c *Compiler) SetBuffer(path, text string)   // unsaved text shadows the file at vault path
func (c *Compiler) ClearBuffer(path string)
func (c *Compiler) Compile(main string) (*Compilation, error) // error only for an invalid path

type Diagnostic struct {
    Error   bool     // false for warnings and "Downloading @preview/x…" statuses
    Message string
    Hints   []string
    Path    string   // vault path, "" when none
    Start, End int   // byte range in Path; Start == End == -1 when none
    Line    int      // zero-based line of Start, -1 when none
}
type Label struct{ Key, Title, Kind string; Page int } // Page one-based, 0 when unknown
type FileWords struct{ Path string; Words int }
type PageInfo struct{ Width, Height float32; Hash string } // points; hash of content

type Compilation struct {
    Main        string
    OK          bool          // a document was produced
    Diagnostics []Diagnostic
    Words       []FileWords
    Labels      []Label       // what `@` can reference, document order
    Missing     []string      // package specs to Fetch, then compile again
    Pages       []PageInfo
    // handle, private
}
func (k *Compilation) Close()                                // idempotent; a finalizer also frees
func (k *Compilation) Fetch(spec string)                     // blocking download of one of Missing
func (k *Compilation) Render(page int, scale float32) (*image.RGBA, error) // premultiplied RGBA, scale = pixels per point
func (k *Compilation) PDF() ([]byte, error)
func (k *Compilation) Complete(path, text string, caret int, explicit bool) []Suggestion
func (k *Compilation) Hover(path, text string, offset int) (Hover, bool)
func (k *Compilation) PositionOf(path string, offset int) (page int, y float32, ok bool) // page zero-based, y in points
func (k *Compilation) TargetAt(page int, x, y float32) (Target, bool)
func (k *Compilation) PagesFrom(path string) []bool
func (k *Compilation) Placed(path, text string) []Diagnostic   // this compile's diagnostics in path, moved onto newer text
func (k *Compilation) Moved(path string, offset int, text string) int

type Suggestion struct {
    Title, Kind, Detail string
    From, Tail int        // replaces text[From:caret] and Tail bytes after the caret
    Insert string         // text to insert
    Fields [][2]int       // placeholder ranges in Insert, in fill order; empty = caret at end
}
type Hover struct{ Text string; Code bool }
type Target struct{ Path string; Offset int; Note string } // Note != "" for a note link, else Path/Offset

// Stateless helpers over Typst's parser.
type Span struct{ Start, End int; Tag string } // Tag: comment, punctuation, escape, strong, emph, link, raw, label, ref, heading, list-marker, list-term, math-delimiter, math-operator, keyword, operator, number, string, function, interpolated, error
func Highlight(text string) []Span
func ProseWords(text string) int
type Heading struct{ Offset, Level int; Title string }
type Include struct{ Offset int; Literal string }
type Parsed struct{ Headings []Heading; Includes []Include }
func ParseDocument(text string) Parsed
type NoteLink struct{ Literal, Target string; Start, End int }
func NoteLinks(text, file string) []NoteLink
func NoteLinkAt(text string, caret int) (start, end int, ok bool)
func CitationAt(text string, caret int) (start, end int, ok bool)
func CitedKeys(text string) []string
func CitationsOf(text, key string) []int
func RenameCitation(text, old, new string, labelled bool) (string, bool)
func DefinesLabel(text, key string) bool
// RewritePaths: the file moved from oldFile to newFile (equal when it stayed); main is its
// top-level document before the move; files are every file's vault path before the move;
// from→to is the move (a path inside from moves inside to).
func RewritePaths(text, oldFile, newFile, main string, files []string, from, to string) (string, bool)
type BibMode int
const ( BibNone BibMode = iota; BibEmbedded; BibLinked; BibCustom )
func BibliographyMode(text, file string) BibMode
func BibliographyStyle(text string) (string, bool)
func SetBibliographyStyle(text, style string) (string, bool)
func BibliographyBlockRange(text string) (start, end int, ok bool)
func WithoutBibliographyBlock(text string) string
func SyncBibliographyBlock(text, yaml string) (string, bool)
func EmbedBibliography(text, yaml string) string
func LinkBibliography(text, file string) string
func RemoveBibliography(text, file string) string
func FigurePlace(text string, at int) int
func CodeOpens(text string, start int) bool   // ide.rs CodeStart::opens
func ImageFormat(data []byte) (ext string, ok bool) // what Typst reads it as: png jpg gif webp svg pdf
// Hayagriva
func CheckLibrary(yaml string) error
type ParsedEntry struct{ Key, Title string; Authors []string; Date string; ParentTitle string } // Date "YYYY[-MM[-DD]]"
func LibraryEntries(yaml string) ([]ParsedEntry, error)
type BiblatexImport struct{ YAML string; Verbatim map[string]map[string]string } // per key: keywords, language
func ImportBiblatex(bibtex string) (BiblatexImport, error)
```

### internal/vault

Port of `store.rs`, `paths.rs`, `history.rs`, `images.rs` (copying), `time.rs`. See
[FORMAT.md](FORMAT.md) for what goes on disk; the format is unchanged (`FORMAT = 1`).

```go
const StateDir = ".thesio"; const Extension = "typ"; const LibraryFile = "library.yaml"
func RootDir() string                       // THESIO_HOME, else ~/Documents/Thesio
func WriteAtomic(path string, data []byte) error
func NaturalLess(a, b string) bool
func ParentOf(path string) (string, bool); func BaseName(path string) string; func Join(parent, name string) string
func DirParts(path string) []string; func IsWithin(path, ancestor string) bool; func MovedPath(path, from, to string) (string, bool)
func IsDocument(name string) bool; func DocumentTitle(name string) string; func DocumentName(stem string) string
func IsPDF(name string) bool
func ResolveLiteral(dir []string, literal string) (string, bool) // typst_source::vault_path
func RelativeTo(dir []string, target string) string              // typst_source::relative
func EscapeMarkup(text string) string; func Quoted(text string) string; func EscapeString(text string) string
func DisplayTitle(title string) string      // trimmed, "Untitled" when blank

type Meta struct{ Pinned bool; Opened int64; Starred int64; PageOffset *int }   // json tags as in FORMAT.md
type Settings struct{ SidebarWidth *float32; WindowSize *[2]float32; Expanded []string }
type Entry struct{ ID string; Meta Meta; Modified int64; Parent string; HasParent bool; Title string; Folder, Typst bool; Body string }
type TrashEntry struct{ ID, Title string; Deleted int64; Parent string }
type Moved struct{ ID string; Rewritten []Rewritten; Warnings []string }
type Rewritten struct{ ID, Text string }
type Restored struct{ Renamed bool; Warnings []string }

// Rewriting paths after a move needs Typst's parser; the app wires it in.
type Rewriter interface {
    // TopDocument: the top-level document id belongs to, among ids, reading texts with read.
    TopDocument(ids []string, read func(id string) (string, bool), id string) string
    // RewritePaths as typst.RewritePaths.
    RewritePaths(text, oldFile, newFile, main string, files []string, from, to string) (string, bool)
}

type Store struct{ /* root, index, trash, bodies cache; Rewriter */ }
func Open() (*Store, []string, error)
func OpenAt(root string) (*Store, []string, error)
func (s *Store) SetRewriter(r Rewriter)
func (s *Store) Root() string
func (s *Store) Path(id string) (string, error)      // rejects anything leaving the vault
func (s *Store) List() []Entry                        // parents before children, natural order within a folder
func (s *Store) Load(id string) (string, error)
func (s *Store) Save(id, text string) error
func (s *Store) Versions(id string) (*Versions, error)
func (s *Store) Create(parent string) (string, error)                 // parent "" = root
func (s *Store) CreateFrom(parent, title, text string) (string, error)
func (s *Store) CreateFolder(parent, title string) (string, error)
func (s *Store) Attach(from, id string) error
func (s *Store) AddImage(document string, image Incoming) (string, error) // path as the document writes it
func (s *Store) Rename(id, title string) (Moved, error)
func (s *Store) SetParent(id, parent string) (Moved, error)
func (s *Store) Delete(id string) ([]string, error)   // to Trash; warnings
func (s *Store) PurgeExpired() []string
func (s *Store) ListTrash() []TrashEntry
func (s *Store) Restore(key string) (Restored, error)
func (s *Store) Purge(key string) error
func (s *Store) Import(path string) (string, error)
func (s *Store) UpdateMeta(id string, f func(*Meta)) error
func (s *Store) Touch(id string) error; SetStarred(id string, on bool) (int64, error); SetPageOffset(id string, n int) error; SetPinned(id string, on bool) error
func (s *Store) Settings() Settings
func (s *Store) UpdateSettings(f func(*Settings)) error

type Versions struct{ /* dir */ }
func (v *Versions) List() []int64; Read(t int64) (string, error); Keep(text string) (bool, error); KeepPaced(text string) (bool, error)

type Incoming struct{ File string; Pasted []byte } // one of
func NamedAsImage(path string) bool
func Figures(paths []string, before, after string) (text string, captions []int)
func BlockGlue(before string) string
// time
func Now() int64; func NowMillis() int64; func Civil(secs int64) (y int, m, d, hh, mm int)
```

`Store.AddImage` needs `typst.ImageFormat` to tell what Typst reads bytes as; it takes it
through a package variable `vault.DetectImage func([]byte) (string, bool)` the app sets
(tests set a stub), so the package itself does not import `internal/typst`.

### internal/project

Port of `project.rs` over `vault.ResolveLiteral` and a parser the app injects, so the
package does not import `internal/typst` and tests run with a stub:

```go
type Heading struct{ Path string; Offset, Level int; Title string }
type Include struct{ Offset int; Literal string }
type Parsed struct{ Headings []Heading; Includes []Include } // Heading.Path left empty by the parser
var Parse func(text string) Parsed       // the app sets it to an adapter over typst.ParseDocument
var RewritePaths func(text, oldFile, newFile, main string, files []string, from, to string) (string, bool) // typst.RewritePaths
var DefinesLabel func(text, key string) bool // typst.DefinesLabel, for HasLabel
func Candidates(main, file string) []string
type Read func(path string) (string, bool)
type Project struct{ /* ... */ }
func Open(main string, read Read) *Project
func Within(main, start string, read Read) *Project
func (p *Project) Contains(path string) bool
func (p *Project) Files() []FileText          // {Path, Text}
func (p *Project) Outline() []Heading
type Documents struct{ /* ... */ }
func LoadDocuments(ids []string, read Read) *Documents
func (d *Documents) Includers(id string) []string
func (d *Documents) TopDocument(id string) string
func (d *Documents) HasLabel(id, key string) bool
func Newest(ids []string, modified []int64) (string, bool)
type LaunchTarget struct{ Document, Main, Text string }
func Launch(root string) (LaunchTarget, bool)
// Rewriter for the vault, built from the above and RewritePaths:
func NewRewriter() vault.Rewriter
```

### internal/library

Port of `library.rs`, `source_type.rs`, `biblatex.rs` (export in Go, import through
`typst.ImportBiblatex`), `identifier.rs`, `lookup.rs` (net/http), `quote.rs`. YAML goes
through `gopkg.in/yaml.v3` nodes so that key order and fields the form does not show
survive a save. A library file that does not parse is never overwritten. Hayagriva is
reached through an interface the app sets, so the package does not import
`internal/typst` and tests run with a stub:

```go
type Engine interface {
    Check(yaml string) error
    Entries(yaml string) ([]ParsedEntry, error)       // ParsedEntry as typst.ParsedEntry
    ImportBiblatex(bibtex string) (BiblatexImport, error) // as typst.BiblatexImport
}
var Hayagriva Engine   // set by the app before Load
const PapersDir = "sources"; const NotesDir = "notes"
func NoteFile(paper string) string; func PaperFile(key string) string; func PaperNamed(stem string) string
type Row struct{ Key, Kind, Title, Authors, Year string; Tags []string }
func (r Row) Label() string; func (r Row) Matches(query string) bool
func RowOfPaper(rows []Row, id string) (Row, bool)
type Added struct{ Key string; Duplicate bool }
type Field struct{ Label, Hint string; Long bool; Slot Slot; Biblatex string } // Biblatex "" = not exported
type SourceType struct{ Label string; Fields []Field /* entry type, parent, genre private */ }
var SourceTypes []*SourceType; var JournalArticle, Preprint, Other *SourceType
var Title, Authors, Date, Publisher, Pages, DOI, ISBN, Arxiv, Version, URL, Accessed, Language Field
var CitationStyles = [][2]string{{"IEEE","ieee"}, ...}; const DefaultStyle = "ieee"
type InvalidLibrary struct{ Path, Reason string }; func (e *InvalidLibrary) Error() string

type Library struct{ /* path, yaml nodes, parsed entries */ }
func Load(root string) (*Library, error)
func (l *Library) Versions() *vault.Versions
func (l *Library) Text() string
func (l *Library) Restore(text string) error
func (l *Library) Len() int; Contains(key string) bool
func (l *Library) PaperIDs(key string) PaperIDs
func (l *Library) Rows() []Row
func (l *Library) Tags() []TagCount     // {Name string; Count int}, natural order
func (l *Library) TagLike(name string) (string, bool)
func (l *Library) SetTag(keys []string, tag string, on bool) error
func (l *Library) SourceType(key string) (*SourceType, bool)
func (l *Library) FieldText(key string, f Field) string
func (l *Library) AddBlank(kind *SourceType) (string, error)
func (l *Library) TypeChangeDropsParent(key string, kind *SourceType) bool
func (l *Library) SetSourceType(key string, kind *SourceType) error
func (l *Library) SaveFields(key, newKey string, fields []FieldValue) (bool, error) // {Field, Text}
func (l *Library) RenameTag(old, new string) error; DeleteTag(tag string) error
func (l *Library) DeleteEntries(keys []string) error
func (l *Library) ImportBiblatex(text string) (added, skipped int, err error)
func (l *Library) AddFetched(entry Entry) (Added, error)   // Entry = a yaml mapping node wrapper
func (l *Library) ExportBiblatex() string
func (l *Library) Subset(keys []string) (string, error)   // YAML without tags, library order

type Identifier struct{ Kind IDKind; Value string } // DOI, Arxiv, PMID, ISBN
func Identify(token string) (Identifier, bool)
func Identifiers(text string) []IdentifierOrText   // {ID Identifier; Text string; OK bool}
type PaperIDs struct{ DOI, Arxiv string }
func Fetch(id Identifier) (Entry, error)             // network
func FetchPaper(ids PaperIDs, email, dest string) (bool, error) // arXiv, then Unpaywall with an email
func CleanQuote(raw string) string; PrintedPage(label string, index int, offset int) string
func PageOffset(label string, index int, entered string) (int, bool); QuoteMarkup(text, key, page string) (string, bool)
```

### internal/pdfkit

macOS only (purego + objc), stubs elsewhere; `Available()` reports it.

```go
func Available() bool
type Document struct{ /* PDFDocument */ }
func Open(path string) (*Document, error); func (d *Document) Close()
func (d *Document) PageCount() int; PageLabel(i int) string; PageText(i int) string
type Highlight struct{ Page int; Text, Comment string; Index int }
func (d *Document) Highlights() []Highlight
func (d *Document) AddHighlight(page int, text, comment string) error // finds text on the page, annotates, saves
func (d *Document) SetComment(h Highlight, comment string) error; RemoveHighlight(h Highlight) error
```

## The app

`internal/app` holds `Workspace`, the state of one window, and its view function. A
feature's state lives in a struct the feature owns (`document.Open`, `citations.State`,
`papers.State`); `Workspace` holds one of each and routes between them. Views are MyGo
view functions over that state, built every frame; nothing of `ui.Element` is stored.
Long work (compiles, downloads, lookups) runs on goroutines and reaches the state through
`host.Host.Update`.

Feature packages never import `internal/app`. They reach the workspace through
`internal/app/host.Host` (see `host.go`, which is the contract), and the workspace
implements it. Shared looks live in `internal/app/theme`: the icon set (`theme.Icons.*`,
Lucide SVGs embedded from `assets/icons/`), the monospace font name (`theme.Mono`), and
the few tokens MyGo's theme lacks (`theme.Caption(t)`, `theme.Danger(t)`, `theme.Hairline(t)`,
`theme.Selected(t)`); everything else is `c.Theme()`.

### internal/app/document

```go
type Open struct{ /* id, main, buffer, editor state, session, preview, save state */ }
// NewOpen opens id with text. main is the top-level document it compiles within;
// withPreview is false for paper notes, which compile for completion and hover only.
func NewOpen(h host.Host, id, text, main string, withPreview bool) *Open
func (d *Open) ID() string
func (d *Open) View(c *ui.Context)        // editor and preview side by side, diagnostics under the preview
func (d *Open) EditorView(c *ui.Context)  // the editor alone, for the paper panel
func (d *Open) Focus()
func (d *Open) Text() string
func (d *Open) SetText(text string, undoable bool) // as the Rust reload (undoable) and sync_text
func (d *Open) Caret() int
func (d *Open) GoTo(offset int)
func (d *Open) InsertCitation(key string)
func (d *Open) InsertQuote(markup string, block bool)
func (d *Open) InsertFigures(paths []string)
func (d *Open) SetPath(id, main string)   // after a rename or move; recompiles when changed
func (d *Open) Unsaved() bool
func (d *Open) Status() SaveStatus         // Idle, Saved, Saving, Failed with its error text
func (d *Open) Flush() bool                // writes now if unsaved; false when that failed
func (d *Open) Discard()                   // gives unsaved changes up
func (d *Open) Words() []typst.FileWords   // of the last good compile
func (d *Open) Labels() []typst.Label
func (d *Open) BlockShown() bool; ToggleBlock()
func (d *Open) Close()
// Hooks the workspace sets:
//   OnChange func(text string)   after each edit (the shell schedules outline refresh)
//   OnCaret func(offset int)
//   OnSaved func(id, text string) after a successful write (versions, sort, bibliographies, outline)
//   OnFollowLink func(path string)
//   Citations func(query string, labels []typst.Label) []editor.Completion  (the @ provider)
//   NoteLinks func(query string) []editor.Completion                         (the [[ provider)
//   OnCitationCompleted func()  (a citation was accepted: ensure a bibliography, mark unsaved)
```

Images dropped on the editor (`DroppedFiles`) or pasted (`OnPaste` with an empty text and
`mygo.Clipboard.ReadImage`) go through `vault.Store.AddImage` and `InsertFigures`.

### internal/app/citations

```go
type State struct{ /* table, selection, sort, tag filter, inspector, dialogs */ }
func New(h host.Host) *State
func (s *State) LibraryView(c *ui.Context)      // the whole Library page: header, tag chips, table, inspector
func (s *State) Overlays(c *ui.Context)         // its dialogs and popovers, built every frame
func (s *State) DocMenuItems(m *ui.Menu)        // Citation Style…, Insert citation, Show/Hide bibliography data, Embed/Link/Remove bibliography
func (s *State) OpenPicker(); OpenLookup(); OpenEntry(key string); OpenNewEntry(); OpenSourcePicker(purpose PickFor)
func (s *State) TableChanged()
func (s *State) CloseInspector(); SaveInspector()
func (s *State) VisibleRows(query string) []library.Row  // by title ascending
func (s *State) CompletionItems(query string, labels []typst.Label) []editor.Completion
func (s *State) CitedIn(id string) []string
func (s *State) SyncBibliographies(); EnsureBibliography(id string); RenameCitations(old, new string)
func (s *State) BibliographyMode(id string) typst.BibMode; SetBibliography(mode typst.BibMode); SetStyle(style string)
func (s *State) LibraryRestored(text string)
func (s *State) OpenLibraryHistory()
type PickFor struct{ Cite bool; Attach string; Link string }  // one of
```

### internal/app/papers

```go
type State struct{ /* current paper, notes (a document.Open without preview), highlights, panel, fetching */ }
func New(h host.Host) *State
func (s *State) SourcesView(c *ui.Context)
func (s *State) PaperView(c *ui.Context)        // the WebView showing the PDF, and the notes/highlights panel
func (s *State) Overlays(c *ui.Context)         // the quote dialog
func (s *State) Current() string
func (s *State) OpenPaper(id string)            // saves first through the host
func (s *State) ClosePaper()
func (s *State) PaperTitle(id string) string; PaperRow(id string) (library.Row, bool)
func (s *State) PaperIDs() []string; HasPaper(key string) bool
func (s *State) AttachPaper(path, id string); LinkPaper(id, key string); RenamePaper(old, new string)
func (s *State) FetchPapers(keys []string)
func (s *State) OpenSourceNotes(key string)
func (s *State) NotesEditor(id string) *document.Open  // the panel's notes when they are id
func (s *State) FlushNotes() bool; DiscardNotes(); NotesError() string
func (s *State) Highlights(id string) []pdfkit.Highlight  // for search, cached by file stamp
func (s *State) PathChanged(old, new string)
```

The paper shows in a `ui.WebView` whose page loads the PDF file (WKWebView's own viewer:
selection, search, zoom). Highlights are PDF annotations written through `internal/pdfkit`:
Highlight Selection reads the text the user copied (the viewer's selection is not readable
from outside) and highlights its first occurrence in the document.

Editing flow: the editor is MyGo's `ui/editor` with a `Highlighter` that calls
`typst.Highlight` on each edit; `OnCompletion` and `OnHover` ask the newest
`Compilation`; diagnostics are `Placed` onto the buffer's version. A change schedules a
compile on the session goroutine (one at a time, newest wins) and a save 500 ms later.
The preview shows `Render`ed pages as bitmaps at the window's scale, re-rendered only
when a page's hash changes.

A document's id is its vault-relative path. Anything that renames or moves a file goes
through `Workspace.PathChanged`.

## Limits

See the previous architecture's limits list in git history; those that still hold are
noted in each package's doc comment.
