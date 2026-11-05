# Architecture

Thesio is a single-process GPUI application in Rust. It edits plain `.typ` files in a vault and shows a live preview compiled by Typst.

The repository is a Cargo workspace with two crates:

- **`crates/core` (`thesio_core`)** is the engine. It has no GPUI dependency and can be built and tested alone.
- **The root crate (`thesio`)** is the GPUI application: window, views, and menus.

Dependencies point only from the application to core. Inside the application, `ui.rs`, `theme.rs`, `completion.rs`, `source.rs`, and `preview.rs` do not know `Workspace`, so a view can be read without it. Workspace state lives in structs owned by the feature that uses it: `OpenDocument`, `Sidebar`, `Outline`, `Search`, `Citations`, `Papers`.

```
main.rs: window
  app.rs: Workspace, the window's root view
    actions.rs: every action's name, menu entry, default keys, and handler
    system.rs: file dialogs, other apps, the clipboard; scripted runs stop here
    store.rs: the vault on disk [core]
    document.rs: OpenDocument, its saves, and every write to a document
    overlay.rs: the one menu or dialog over the window
    chrome.rs: the page header and header buttons the views share
    sidebar, tree, home, search, trash, confirm, docmenu
    citations/: the Library table, inspector, tags, picker, lookup, bibliographies
    papers/: Sources, the viewer, notes, quotes
    settings, preferences, appearance
    source.rs: SourceEditor, the open file's text
      text.rs: shaping and painting a line, shared with TextField
      completion.rs: what a provider of the popup's items answers
      buffer.rs: text, selection, undo [core]
      syntax.rs: highlighting from Typst's parser [core]
    preview.rs: Preview, the view of the pages and diagnostics
    session.rs: DocumentSession, one document's compiles and its newest result
      compiler.rs: the Typst World, PDF export, page images [core]
```

## Core modules

| Module | Responsibility |
|---|---|
| `store.rs` | Lists, reads, and writes vault files; renames, moves, Trash, and app state. Operations return their warnings. See [FORMAT.md](FORMAT.md). |
| `compiler.rs` | Compiles a vault file with the vault as the Typst project root. Unsaved text shadows the disk through buffers. `compile` runs on a large stack and returns a `Compilation`: the document, diagnostics, word counts, and the files it read, so it answers `typst-ide`'s questions about moving between pages and source, on any thread, without the compiler. |
| `buffer.rs` | Source text, selection, a line index, and undo as a list of edits. Consecutive typing within 700 ms merges into one step. |
| `syntax.rs` | Highlight spans from `typst-syntax`. |
| `library.rs` | The citation library. |
| `biblatex.rs` | BibLaTeX import and export. Hayagriva parses, and export takes every name from the field table. |
| `bibliography.rs` | A document's bibliography, read from its text: the generated block, the `#bibliography` call, its style and its mode. |
| `source_type.rs` | The entry schema: source types, their fields, where each field lives in an entry, and its BibLaTeX name. Nothing else names an entry key. |
| `identifier.rs` | Recognises DOIs, arXiv IDs, PMIDs, and ISBNs in text. |
| `lookup.rs` | Fetches entries by identifier and downloads open-access papers. The only code that reaches the network. |
| `typst_source.rs` | The parser helpers everything else shares: the tree walker, path rules, and escaping. |
| `references.rs` | Note links, citations, and rewriting path arguments after a move. |
| `project.rs` | A document and the files it includes, found the way the compiler finds them: the outline, which documents include which, and what opens at launch. |
| `history.rs` | `Versions`: a file's earlier versions and how often they are kept. |
| `diff.rs` | What changed between two texts, by line. |
| `quote.rs` | A PDF highlight as Typst: cleaned text, page numbers, and the quotation's markup. |
| `paths.rs` | Vault path helpers, file naming, natural order, atomic writes. |
| `time.rs` | Local dates without a time zone database, in UTC when the offset is unknown. |

## Editing flow

1. `SourceEditor` turns keys, mouse events, and input-method text into `Buffer` edits.
2. It recomputes highlights and emits `SourceEvent::Changed`.
3. `Workspace` passes the text to the `DocumentSession` and schedules a save 500 ms later.
4. `DocumentSession` waits 120 ms for typing to stop, then compiles on a background thread. A newer edit supersedes a pending compile. At launch nothing waits: `session::prepare` compiles the likely document from `main`, before the UI exists, and the first frame holds up to 100 ms for the result so it paints finished. A wrong guess is dropped and compiled normally.
5. Pages are hashed, and only changed pages are rendered again. When the open file is included by another, the whole document still compiles so numbering and references stay right. Spans show which pages draw from the open file or files it includes; the rest render small and blurred, and the preview opens at the file's longest run of pages. A failed compile keeps the last good pages on screen and lists diagnostics below them. Clicking a diagnostic in the open file moves the caret there.
6. Each result is published on the session: pages, diagnostics, word counts, and the `Compilation` they came from. `Preview` draws them and answers clicks from that value, so the main thread never waits for the compiler. `SessionEvent::Compiled` announces a result, and `SessionEvent::Diagnostics` announces that its diagnostics differ from the last. The editor holds the same session and reads its newest result from `SourceEditor::compilation`. An editor with no `Preview`, such as paper notes, can get a session from `DocumentSession::without_pages`, which compiles and renders no pages.

A failed save keeps the changes pending, shows the error in the top bar, and asks before anything replaces the document.

Text that does not come from typing goes through `Workspace::write_document`, which writes the file and updates its entry, the editors showing it, and the preview. `Workspace::text_of` reads a document, from the open editor if it is the open one. Errors other than a failed save appear as toasts.

## Editor rendering

`SourceEditor` is a GPUI `List` with one row per logical line, so only visible lines are shaped. Each `LineElement` wraps its line softly and paints its own selection and caret through `text.rs`, which `TextField` shares. Painted rows are kept from frame to frame to resolve mouse positions and vertical caret movement. `Buffer` reports each edit as a `Change`, and only the rows it names are replaced, which keeps the scroll position and measured heights.

Every caret move other than an edit goes through `SourceEditor::move_caret`, which keeps the caret out of the folded bibliography and emits `SourceEvent::Caret`. Bringing the caret into view is a `Reveal` request with the margin it wants. It is resolved after a frame has painted the caret's row; until then it moves the list towards that row, first by measured heights and then to the row itself.

Rows also paint `Decoration`s: an underline over a precise byte range, in a colour, which moves with edits around it. `SourceEditor::set_decorations` sets them for diagnostics, and `SourceEditor::text_at` says which byte the pointer is on, if any, for hover. Neither has a caller yet.

## Completion

`completion.rs` has the `Provider` trait, and each provider lives with its feature: `CitationKeys` in `citations/`, `NoteLinks` in `document.rs`. A provider finds what it can complete at the caret (`find`: the range an accepted item replaces and the query so far) and answers with a task (`items`), so an answer may come from a background thread. A newer query replaces the task, which drops an older answer. An `Item` holds what to insert, what to select in it afterwards, and optional `info` shown under the list. Both ask `Workspace` from that task, which runs after the editor's update ends. Only typing or ⌃Space opens the popup, and `SourceEvent::Completed` tells the host what was accepted. The popup keeps six rows on screen and scrolls the rest.

## Overlays

`Workspace::overlay` holds the one menu or dialog over the window, so opening one replaces the last. No flow stacks them: a menu's action closes the menu first, and the Save failed confirmation carries the action it holds back, so cancelling it returns to no overlay rather than a dialog beneath. `overlay.rs` has the enum, Esc, the render dispatch, the modal and menu shells that close it, and whether the paper viewer has to step aside; focus goes back where it was when the overlay closes. Changing the main view closes the overlay, through `Workspace::set_view`, which also closes the paper viewer away from Paper. Rendering changes nothing: what follows the Library's rows, such as the inspector, runs from `Workspace::table_changed`. Column and sidebar drags are separate full-window layers, and the stacking order is `ui::layer`.

## Actions

`actions.rs` is one table of the workspace's actions, so a new action is one row: its name, its menu group and label, its default keys, and its handler. `shortcuts.rs` applies `.thesio/shortcuts.json` over those keys, and `shortcuts::hint` reads a tooltip's key back from the keymap, so a custom key shows and an unbound action shows none. The Edit menu names the editor's own actions, which keep their keys in `source.rs` and `input.rs`.

## Vault ids

A document's id is its vault-relative path. Renaming or moving a file therefore changes its id. `Workspace::path_changed` updates `OpenDocument`, so its editor and undo history survive. Folders are `Entry` values with `folder: true`, which lets the sidebar tree treat them like any other row.

## Search

The search dialog lists documents, then sources, then PDF highlights, from any PDF in the vault. Highlights live inside each PDF, so `search.rs` reads them with PDFKit when the dialog opens, off the main thread, and keeps them by file time and size. Results fill in once that finishes. Text in a PDF that is not highlighted is not searched.

## Version history

`history.rs` keeps versions as plain files in `.thesio/history/<id>/`: one when a file opens, then at most one every five minutes as it saves, and none if nothing changed. A paper's notes count as a file that opens with the paper, and save the same way, 500 ms after typing stops. Every version stays for a day, then one a day for 90 days, and a file's newest version always stays. `Store::relocate` moves a file's versions with it. Restoring keeps the current text as a version first and goes through the editor, so it can be undone. `Library::commit` keeps `library.yaml` the same way before it writes, so a run of quick edits leaves the state before the first, and a version that no longer parses cannot be restored. A file that cannot be read cannot be kept, so the write stops.

## Sidebar

The Outline reads `=` headings from the top-level document and the files it includes, in reading order (`Project::outline`). It updates when the document saves, not on every keystroke, and headings made with `#heading(...)` are not listed.

`tree.rs` builds the tree from entries: folders first, then names in natural order, ignoring case and comparing runs of digits by value (`paths::natural_cmp`). Every visible file is listed. Text files open in the editor without a preview, and other files open in the system's default app. Rows open on click rather than on mouse down, so a drag can start from any row. Dropping on a folder moves into it, dropping on a file moves beside it, and dropping on empty space moves to the vault root. Open folders are saved in `.thesio/settings.json`.

A rename or move also updates the paths in every vault document: `#include`, `#import`, the path arguments of `image`, `bibliography`, `read`, and the data loaders, and `#link`s to `.typ` files. A moved file's own relative paths are adjusted too. A path starting with `/` means the file the compiler finds, nearest the document's main file first (`project::candidates`), so it stays relative to that folder while the main file and the target remain inside it, and names the vault root otherwise. If the open document changes, its editor takes the new text as one undoable edit.

## Note links

A note link is plain Typst, `#link("../notes/idea.typ")[Idea]`, so a document stays valid without the app. Any `link` whose target ends in `.typ` and is not a web address counts (`references::note_links`). A page only keeps the link's text, so a preview click finds the target through the loaded file that wrote it. Linked From reads the links in every document and updates when the open document opens or saves.

## Citations

`library.yaml` at the vault root is the citation library, in Hayagriva's format. The YAML mapping is the source of truth, so the editor writes only the fields you change, and an edit that Hayagriva rejects is not written. A file that does not parse is left alone and the Library page reports it. Deleting entries asks first and says how many documents cite them.

`source_type.rs` lists the source types and each type's fields. A source type is a Hayagriva type plus its parent's type, so it is read from any entry without a new field, and a Hayagriva type outside the table shows as Other and keeps its `type` until changed. An article with no parent is a Preprint only with an arXiv ID or link, an `archive`, or `genre: Preprint`, which choosing that type writes; otherwise it is a Journal Article. The table is our own: Zotero's schema has no licence file, so it is not copied. The inspector beside the table edits the selected entry and saves when a field loses focus.

BibLaTeX goes through Hayagriva, except keywords and language, which it drops and `import_biblatex` reads with the `biblatex` crate. Import then refines `@misc` entries: an arXiv ID makes a Preprint, a version a Software entry, and an access date a Web Page. A test exports the fields of every imported type and reads them back.

The citation style is the `style` argument of the document's `#bibliography`, so it lives in the file and survives block updates. Picking one edits that argument and nothing else, including in a `#bibliography` the user wrote. Changing a citation key rewrites `@key` and `#cite(<key>)` in every document, and leaves plain labels, raw text, and comments alone.

Tags are a `tags` list on an entry, a field Hayagriva ignores. They stay out of the generated bibliography blocks, so tagging never rewrites a document, and BibLaTeX export writes them as `keywords`. The table's selection and column widths live in `Citations`; widths persist in preferences, while actions apply only to selected rows the search and tag filter leave visible.

Add by ID recognises DOIs, arXiv IDs, PMIDs, and ISBNs, including pasted links. DOIs and arXiv IDs go to doi.org, PMIDs to NCBI, and ISBNs to Open Library. Requests happen only when the user asks, and send only the identifier. An entry with a DOI, PMID, ISBN, or arXiv ID already in the library is skipped. Other numbers, such as an ISSN or a version, are shared by many works and do not count. Keys are generated as surname and year.

Adding a source also looks for its paper: arXiv first, then the locations Unpaywall lists, when an email is set in Settings. Only PDFs are kept, as `sources/<key>.pdf`, and the Sources page lists that folder. A paper follows its entry when the key changes, and stays when the entry is deleted. A paper can also be added by hand. A PDF in `sources/` whose name matches no key shows as unlinked until it is linked to a source.

On macOS papers open in `native_pdf.rs`, a PDFKit `PDFView` above the GPUI surface. It hides what GPUI draws beneath it, so the viewer steps aside while a menu or dialog is open. PDFKit was chosen over the `hayro` renderer because it brings text selection and writes standard PDF annotations, which future annotation work can build on. Other platforms open papers in the system app.

Highlights and their comments are PDF annotations, written into the paper in `sources/`, so any reader shows them. A source's notes are an ordinary Typst document at `notes/<key>.typ`, kept beside the paper in `papers/` and renamed with the key.

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
- BibLaTeX export covers common fields only, and Hayagriva's model has no direct equivalent for some BibLaTeX types.
- Zotero import and sync are not implemented, and ADS Bibcodes are not supported because ADS requires a personal API key.
