# Architecture

Thesio is a single-process GPUI application written in Rust, with approximately 40,000 lines of code. In memory, a document is a block tree (`model.rs`). On disk, it is a DocxXML file (`docxml.rs`). The editor renders the tree as a virtualized list.

The repository is a Cargo workspace with two crates:

- **`crates/core` (`thesio_core`, approximately 10,000 lines):** The document engine. It contains the model, DocxXML, editing core and undo, disk library, Markdown and HTML import, and Markdown export. It does not depend on GPUI and can be built and tested independently with `cargo test -p thesio_core`. It also contains the fuzzer.
- **Root crate (`thesio`):** The GPUI application, including windows, views, rendering, menus, and overlays. `main.rs` re-exports `thesio_core` modules at the crate root. `editor/mod.rs` re-exports `editor::core` and `editor::table_ops`, so application code still uses `crate::model` and `super::core`.

Dependencies point from the application to core only. Core modules reference only other core modules.

```text
main.rs - window
  └─ app.rs  Workspace (window root view)
       ├─ store.rs        Library: docs/*.xml + index.json (see FORMAT.md) [core]
       ├─ sidebar / home / search / trash / tree / docmenu / docinfo / modes
       └─ editor::Editor (view.rs)    Open document
            └─ editor::Core (core.rs) Document + selection + undo stack     [core]
```

## Layers

| Layer | Files | Responsibility | Rule |
|---|---|---|---|
| Model | `model.rs` | `Document`, `Block`, `BlockKind`, `RichText` (text and style runs), and inline objects `Inline { kind, xml }` | Pure data. `BlockKind::nests()` determines which blocks can have children. |
| Format | `docxml.rs` | DocxXML and `Document` conversion | Unknown elements are preserved verbatim as `Raw`. Inline objects retain their original XML and have their type parsed once on input. Output is stable; see `FORMAT.md`. |
| Editing core | `editor/core.rs`, `editor/table_ops.rs` | Document operations: input, deletion, merging, indentation, conversion, dragging, tables, folding, inline objects, and undo | Lives in `thesio_core`, does not use GPUI, and supports direct unit testing and fuzzing. |
| Editor view | `editor/view.rs` and other `editor/*` files | Keyboard and mouse events to Core calls; overlays, menus, and block interactions | Document changes go through Core, with the exceptions described in this document. |
| Rendering | `editor/render.rs`, `editor/text.rs` | Blocks to GPUI elements; `text.rs` is a custom-drawn rich text element | Reads the document only. |
| Application | `app.rs` and other top-level modules | Sidebar, home, search, trash, document tree, import and export, and printing | Reads and writes disk through `Store`. |

## Editing flow

Each edit follows this order:

1. The view receives an event and calls `core.checkpoint(kind)`. It records an undo snapshot, increments `generation`, and sets `dirty`. Consecutive input in the same block, less than 700 ms apart, is grouped into one step.
2. Call a Core operation to modify `core.doc` or `core.sel`.
3. Call `Editor::changed()`:
   - First, `core.reveal_caret()` unfolds any heading or list item hiding either end of the selection or block selection.
   - If there are changes, it emits `EditorEvent::Changed` and refreshes search results, the slash menu, and the @ panel.
4. `Workspace` receives `Changed` and calls `schedule_save`. After 500 ms, `flush()` writes to disk. On failure, changes remain unsaved, the top bar shows an error, and the user is prompted before switching documents, creating a document, or quitting.

`crates/core/src/editor/fuzz_tests.rs` checks Core invariants after each random edit:

- Selection endpoints are visible caret stops in the same scope, either the same cell or the document body.
- Block IDs are unique, and runs are in bounds and do not overlap. Inline object runs contain only U+FFFC.
- Column proportions sum to 1, and merged table cells do not overlap.
- Only blocks for which `nests()` is true and containers (quotes, callouts, grids, and columns) have children.
- Code blocks contain no styles or inline objects.
- Saving and reading back produces the same result.
- Undoing all steps and then redoing them returns to the same state.

### Direct modification exceptions

Structural changes, including adding, deleting, or moving blocks and merging or splitting text, all live in Core. A few view modules directly modify **single-block properties** through `core.block_mut(id)`, with `checkpoint` and `changed` around the modification:

- Image size, crop, and description (`image.rs`)
- Countdown settings (`countdown.rs`)
- Code block name (`caption.rs`)
- Icon and cover (`cover.rs`)
- Title rename (`view.rs`)
- Some check and fold toggles (`view.rs`, `render.rs`)

These operations do not change the tree structure and therefore do not break the listed invariants. Add every new structural operation to Core and add a random operation to the fuzzer's `step`.

## Undo

- Each step is a snapshot (`Snapshot`) of the entire `Document`, stored in a `VecDeque`. History is limited to 500 steps and approximately 256 MB, but retains at least 20 steps regardless of document length.
- Document icons and fold states are outside history. `keep_outside_history` preserves their current values during undo and redo.
- `generation` increments on every change, including folding. The rendering cache, table of contents, and word count use it to determine when to recompute.

Snapshots are simple and reliable, but each step copies the entire document. In a measured history of a million-character document, synchronous input frame processing had a p50 of 5.06 ms. This excludes display latency.

## Rendering and virtualization

- The page is a GPUI `List`. Its items are the header (`Item::Header`), each visible top-level block, and `Item::Below` (the references list).
- Children of text, heading, and list blocks each occupy a row (`flattens`). Children of container blocks render inside the container's row.
- `sync_items` computes a key, `hash_block`, for each row and compares it with the previous frame. It calls `splice` only for the changed interval outside the common prefix and suffix, so only changed rows have their height remeasured.
- For text, `hash_block` uses only the length. The list remeasures visible rows every frame, so an equal-length edit, such as ⌘B on a whole paragraph, can at most temporarily stale the cached height of an off-screen row. Hashing the text itself adds approximately 3 ms per keystroke in a million-character document. Measurements are recorded in comments in `render.rs`.
- The scrollbar uses `ListState::estimated_scroll_metrics`, added to vendor GPUI. It estimates unmeasured rows using the average row height; see `vendor/PATCHES.md`.

## Module map

Document engine (`crates/core/src/`):

| Module | Contents |
|---|---|
| `model.rs` | Document model, including inline object types and `display_title` |
| `docxml.rs` | DocxXML parsing and serialization; `to_inline` constructs inline objects |
| `store.rs` | Library reads and writes, index recovery, trash, settings, and `image_path` |
| `paste.rs` | HTML and Markdown to blocks, shared by paste and import |
| `markdown.rs` | Markdown export |
| `lang.rs` | Code language table |
| `palette.rs` | Text, background, and callout palettes (RGBA, independent of GPUI; `theme.rs` converts them to GPUI colors) |
| `time.rs` | Date conversion, local time zone, and current time |
| `editor/core.rs` | Editing core |
| `editor/table_ops.rs` | Table operations and cell selection (`CellSel`) |
| `editor/fuzz_tests.rs` | Random edit fuzzer |

Application top level (`src/`):

| Module | Contents |
|---|---|
| `main.rs` | Entry point, `THESIO_SCRIPT` script mode, and `thesio_core` module re-exports |
| `app.rs` | `Workspace`: two-column window, open, create, save, delete, save status, and reminder scheduling |
| `html.rs`, `printing.rs`, `transfer.rs` | HTML and PDF export, printing, and import and export UI |
| `clipboard.rs` | System clipboard HTML reading and simulated clipboard in script mode |
| `sidebar.rs`, `tree.rs` | Sidebar and document tree, including subdocuments and move-to |
| `home.rs`, `trash.rs` | Home and trash |
| `search.rs` | Search dialog and advanced search |
| `docmenu.rs`, `docinfo.rs`, `modes.rs` | “…” menu, document information, edit and read mode switching, and shortcuts panel |
| `theme.rs` | Design tokens: `craft` color scale, semantic `Theme::*` colors, display colors for named colors, `shadow_*` shadows, type scale, and note panel dimensions |
| `ui.rs`, `input.rs` | Shared widgets (menu shell, menu row, white pill button, tips, and toast) and single-line input |
| `highlight.rs` | Code highlighting |
| `assets.rs` | Icons, fonts, and cover gallery embedded in the binary |
| `devtools.rs` | Script driver; see `tools/harness/README.md` |

Editor (`src/editor/`):

| Module | Contents |
|---|---|
| `view.rs` | `Editor` entity: state, keyboard and mouse handling, and `changed()` |
| `keymap.rs`, `menus.rs` | Shortcut bindings and slash and conversion menu data |
| `render.rs`, `text.rs` | Page rendering and rich text element |
| `overlays.rs` | Handles, slash and + menus, block menu, and floating toolbar |
| `drag.rs`, `gap.rs` | Block dragging and clicks in gaps between blocks |
| `table.rs`, `grid.rs` | Table UI (operations are in core's `table_ops.rs`) and columns |
| `image.rs`, `file.rs`, `cover.rs` | Images, video or files, icons, and covers |
| `math.rs` | Formulas (`katex-rs` generates HTML, followed by custom layout) |
| `link.rs`, `links.rs` | Link panel and cards, @document, and references list |
| `button.rs`, `countdown.rs`, `reminder.rs`, `subpages.rs` | Buttons, countdowns, date reminders, and subpage list |
| `emoji.rs`, `emoji_picker.rs` | Emoji data and panel |
| `catalogue.rs`, `find.rs`, `reading.rs`, `scrollbar.rs`, `caption.rs` | Table of contents, find and replace, reading mode and information row, scrollbar, code block names, and image descriptions |

## Interface language

The interface is English-only. `Preferences::english` remains in persisted preferences for backward compatibility; it does not select the interface language. Existing localization helpers and menu labels still map interface text to English.

User titles, body text, attachment names, and saved button text are not translation keys. Interface defaults such as an empty title are presented in English. The application translates known Core storage diagnostic templates while preserving paths, document IDs, and system errors. The document engine does not depend on the interface language. Language regression checks cover interface keys, menu tables, date reminders, and preservation of user content and diagnostic parameters.
