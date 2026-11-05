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
| `store.rs` | Lists, reads, and writes vault files; renames, moves, Trash, and app state. See [FORMAT.md](FORMAT.md). |
| `compiler.rs` | Compiles a vault file with the vault as the Typst project root. Unsaved text shadows the disk through buffers. |
| `buffer.rs` | Source text, selection, a line index, and undo as a list of edits. Consecutive typing within 700 ms merges into one step. |
| `syntax.rs` | Highlight spans from `typst-syntax`. |
| `time.rs` | Local dates without a time zone database. |

## Editing flow

1. `SourceEditor` turns keys, mouse events, and input-method text into `Buffer` edits.
2. It recomputes highlights and emits `SourceEvent::Changed`.
3. `Workspace` passes the text to `Preview` and schedules a save 500 ms later.
4. `Preview` waits 120 ms for typing to stop, then compiles on a background thread. A newer edit supersedes a pending compile.
5. Pages are hashed, and only changed pages are rendered again. A failed compile keeps the last good pages on screen and lists diagnostics below them. Clicking a diagnostic in the open file moves the caret there.

A failed save keeps the changes pending, shows the error in the top bar, and asks before anything replaces the document.

## Editor rendering

`SourceEditor` is a GPUI `List` with one row per logical line, so only visible lines are shaped. Each `LineElement` wraps its line softly and paints its own selection and caret. Painted rows are kept for the frame to resolve mouse positions and vertical caret movement.

## Vault ids

A document's id is its vault-relative path. Renaming or moving a file therefore changes its id. `Workspace::path_changed` updates the open document, so its editor and undo history survive. Folders are `Entry` values with `folder: true`, which lets the sidebar tree treat them like any other row.

## Sidebar

`tree.rs` builds the tree from entries: folders first, then names in natural order, ignoring case and comparing runs of digits by value (`store::natural_cmp`). Every visible file is listed. Text files open in the editor without a preview, and other files open in the system's default app. Rows open on click rather than on mouse down, so a drag can start from any row. Dropping on a folder moves into it, dropping on a file moves beside it, and dropping on empty space moves to the vault root. Open folders are saved in `.thesio/settings.json`.

## Limits

- Fonts are the set bundled with Typst.
- `@preview` packages are rejected instead of downloaded.
- Renaming or moving a file does not update `#include` paths that point at it.
- Changes made to the open file by other programs are not picked up, and the next save overwrites them.
- Two windows on the same file are not kept in sync.
- The editor has no find and replace yet.
