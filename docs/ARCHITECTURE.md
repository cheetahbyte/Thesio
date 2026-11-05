# Architecture

Thesio is a single-process GPUI application in Rust. It edits plain `.typ` files in a vault and shows a live preview compiled by Typst.

The repository is a Cargo workspace with two crates:

- **`crates/core` (`thesio_core`)** is the engine. It has no GPUI dependency and can be built and tested alone.
- **The root crate (`thesio`)** is the GPUI application: window, views, and menus.

Dependencies point only from the application to core.

```
main.rs: window
  app.rs: Workspace, the window's root view
    store.rs: the vault on disk [core]
    sidebar, tree, home, search, trash, settings, docmenu
    source.rs: SourceEditor, the open file's text
      buffer.rs: text, selection, undo [core]
      syntax.rs: highlighting from Typst's parser [core]
    preview.rs: Preview, the rendered pages and diagnostics
      compiler.rs: the Typst World, PDF export, page images [core]
```

## Core modules

| Module | Responsibility |
|---|---|
| `store.rs` | Lists, reads, and writes vault files; renames, moves, Trash, and app state. Operations return their warnings. See [FORMAT.md](FORMAT.md). |
| `compiler.rs` | Compiles a vault file with the vault as the Typst project root. Unsaved text shadows the disk through buffers. It keeps the last good document, and `typst-ide` maps between that document and the source for the preview. |
| `buffer.rs` | Source text, selection, a line index, and undo as a list of edits. Consecutive typing within 700 ms merges into one step. |
| `syntax.rs` | Highlight spans from `typst-syntax`. |
| `library.rs` | The citation library. |
| `biblatex.rs` | BibLaTeX import and export, by the names in the field table. |
| `bibliography.rs` | A document's bibliography, read from its text: the generated block, the `#bibliography` call, its style and its mode. |
| `source_type.rs` | The entry schema: source types, their fields, where each field lives in an entry, and its BibLaTeX name. Nothing else names an entry key. |
| `identifier.rs` | Recognises DOIs, arXiv IDs, PMIDs, and ISBNs in text. |
| `lookup.rs` | Fetches entries by identifier and downloads open-access papers. The only code that reaches the network. |
| `typst_source.rs` | The parser helpers everything else shares: the tree walker, path rules, and escaping. |
| `references.rs` | Note links, citations, and rewriting path arguments after a move. |
| `project.rs` | A document and the files it includes, found the way the compiler finds them: the outline, which documents include which, and what opens at launch. |
| `history.rs` | `Versions`: a file's earlier versions and how often they are kept. |
| `diff.rs` | What changed between two texts, by line. |
| `paths.rs` | Vault path helpers, file naming, natural order, atomic writes. |
| `time.rs` | Local dates without a time zone database. |

## Editing flow

1. `SourceEditor` turns keys, mouse events, and input-method text into `Buffer` edits.
2. It recomputes highlights and emits `SourceEvent::Changed`.
3. `Workspace` passes the text to `Preview` and schedules a save 500 ms later.
4. `Preview` waits 120 ms for typing to stop, then compiles on a background thread. A newer edit supersedes a pending compile. At launch nothing waits: `preview::prepare` compiles the likely document from `main`, before the UI exists, and the first frame holds up to 100 ms for the result so it paints finished. A wrong guess is dropped and compiled normally.
5. Pages are hashed, and only changed pages are rendered again. When the open file is included by another, the whole document still compiles so numbering and references stay right. Spans show which pages draw from the open file or files it includes; the rest render small and blurred, and the preview opens at the file's longest run of pages. A failed compile keeps the last good pages on screen and lists diagnostics below them. Clicking a diagnostic in the open file moves the caret there.

A failed save keeps the changes pending, shows the error in the top bar, and asks before anything replaces the document.

## Editor rendering

`SourceEditor` is a GPUI `List` with one row per logical line, so only visible lines are shaped. Each `LineElement` wraps its line softly and paints its own selection and caret. Painted rows are kept for the frame to resolve mouse positions and vertical caret movement.

## Vault ids

A document's id is its vault-relative path. Renaming or moving a file therefore changes its id. `Workspace::path_changed` updates the open document, so its editor and undo history survive. Folders are `Entry` values with `folder: true`, which lets the sidebar tree treat them like any other row.

## Search

The search dialog lists documents, then sources, then PDF highlights. Highlights live inside each PDF, so `search.rs` reads them with PDFKit when the dialog opens, off the main thread, and keeps them by file time and size. Results fill in once that finishes. Text in a PDF that is not highlighted is not searched.

## Version history

`history.rs` keeps versions as plain files in `.thesio/history/<id>/`: one when a file opens, then at most one every five minutes as it saves, and none if nothing changed. Every version stays for a day, then one a day for 90 days, and a file's newest version always stays. `Store::relocate` moves a file's versions with it. Restoring keeps the current text as a version first and goes through the editor, so it can be undone. `Library::commit` keeps `library.yaml` the same way before it writes, so a run of quick edits leaves the state before the first, and a version that no longer parses cannot be restored.

## Sidebar

The Outline reads `=` headings from the top-level document and the files it includes, in reading order (`Project::outline`). It updates when the document saves, not on every keystroke, and headings made with `#heading(...)` are not listed.

`tree.rs` builds the tree from entries: folders first, then names in natural order, ignoring case and comparing runs of digits by value (`paths::natural_cmp`). Every visible file is listed. Text files open in the editor without a preview, and other files open in the system's default app. Rows open on click rather than on mouse down, so a drag can start from any row. Dropping on a folder moves into it, dropping on a file moves beside it, and dropping on empty space moves to the vault root. Open folders are saved in `.thesio/settings.json`.

A rename or move also updates the paths in every vault document: `#include`, `#import`, the path arguments of `image`, `bibliography`, `read`, and the data loaders, and `#link`s to `.typ` files. A moved file's own relative paths are adjusted too. If the open document changes, its editor takes the new text as one undoable edit.

## Note links

A note link is plain Typst, `#link("../notes/idea.typ")[Idea]`, so a document stays valid without the app. Any `link` whose target ends in `.typ` and is not a web address counts (`references::note_links`). A page only keeps the link's text, so a preview click finds the target through the loaded file that wrote it. Linked From reads the links in every document and updates when the open document opens or saves.

## Citations

`library.yaml` at the vault root is the citation library, in Hayagriva's format. The YAML mapping is the source of truth, so the editor writes only the fields you change, and an edit that Hayagriva rejects is not written. A file that does not parse is left alone and the Library page reports it. Deleting entries asks first and says how many documents cite them.

`source_type.rs` lists the source types and each type's fields. A source type is a Hayagriva type plus its parent's type, so it is read from any entry without a new field, and a Hayagriva type outside the table shows as Other and keeps its `type` until changed. An article with no parent is a Preprint only with an arXiv ID or link, an `archive`, or `genre: Preprint`, which choosing that type writes; otherwise it is a Journal Article. The table is our own: Zotero's schema has no licence file, so it is not copied. The inspector beside the table edits the selected entry and saves when a field loses focus.

BibLaTeX goes through Hayagriva, except keywords and language, which it drops and `import_biblatex` reads with the `biblatex` crate. Import then refines `@misc` entries: an arXiv ID makes a Preprint, a version a Software entry, and an access date a Web Page. A test exports the fields of every imported type and reads them back.

The citation style is the `style` argument of the document's `#bibliography`, so it lives in the file and survives block updates. Picking one edits that argument and nothing else, including in a `#bibliography` the user wrote. Changing a citation key rewrites `@key` and `#cite(<key>)` in every document, and leaves plain labels, raw text, and comments alone.

Tags are a `tags` list on an entry, a field Hayagriva ignores. They stay out of the generated bibliography blocks, so tagging never rewrites a document, and BibLaTeX export writes them as `keywords`. The table's selection and column widths live in `Citations`; widths persist in preferences, while actions apply only to selected rows the search and tag filter leave visible.

Add by ID recognises DOIs, arXiv IDs, PMIDs, and ISBNs, including pasted links. DOIs and arXiv IDs go to doi.org, PMIDs to NCBI, and ISBNs to Open Library. Requests happen only when the user asks, and send only the identifier. An entry with a DOI, PMID, ISBN, or arXiv ID already in the library is skipped. Keys are generated as surname and year.

Adding a source also looks for its paper: arXiv first, then the locations Unpaywall lists, when an email is set in Settings. Only PDFs are kept, as `sources/<key>.pdf`, and the Sources page lists that folder. A paper follows its entry when the key changes, and stays when the entry is deleted. A paper can also be added by hand. A PDF in `sources/` whose name matches no key shows as unlinked until it is linked to a source.

On macOS papers open in `native_pdf.rs`, a PDFKit `PDFView` above the GPUI surface. It hides what GPUI draws beneath it, so the viewer steps aside while a menu or dialog is open. PDFKit was chosen over the `hayro` renderer because it brings text selection and writes standard PDF annotations, which future annotation work can build on. Other platforms open papers in the system app.

Highlights and their comments are PDF annotations, written into the paper in `sources/`, so any reader shows them. A source's notes are an ordinary Typst document at `notes/<key>.typ`, kept beside the paper in `paper.rs` and renamed with the key.

A document has one of three bibliography modes, read from its text (`bibliography::mode`):

- **Embedded:** a generated block at the end of the file holds the entries its citations use. It is regenerated after each save, outside the undo history. Thesio owns the block, apart from options after `bytes(...)` such as `style:`. The editor folds it into one row and keeps the caret out of it, and Show bibliography data in the document menu reveals it. Home cards and search ignore it.
- **Linked:** `#bibliography("library.yaml")`, with a path relative to the document.
- **Custom:** any other `#bibliography` call, which Thesio leaves alone.

The first citation in a document embeds a block unless one already covers it. A document can have only one bibliography, so the block goes in the top-level document, the one no other file includes, and covers citations in everything it includes. An included file's preview compiles that top-level document for the same reason.

## Limits

- Fonts are the set bundled with Typst.
- `@preview` packages are rejected instead of downloaded.
- A rename can break code that uses a module by its file name, as in `#import "a.typ"` followed by `a.title`.
- Paths built at run time, such as `"chapters/" + name`, are not updated by a move.
- Changes made to the open file by other programs are not picked up, and the next save overwrites them.
- Two windows on the same file are not kept in sync.
- The editor has no find and replace yet.
- Changing a citation key does not update `@key` citations that use it.
- BibLaTeX export covers common fields only, and Hayagriva's model has no direct equivalent for some BibLaTeX types.
- Zotero import and sync are not implemented, and ADS Bibcodes are not supported because ADS requires a personal API key.
