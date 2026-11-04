# Storage format

## Library directory

The default location is `~/Documents/Thesio`, a separate folder in Documents that users can see and back up. Set the `THESIO_HOME` environment variable to use another location.

```text
Thesio/
├── format.json          {"format": 1}: directory layout version (store::FORMAT)
├── index.json           Document ID to Meta
├── index.json.bak       Previous index.json
├── settings.json        Window-level UI settings, such as sidebar width
├── .lock                Write lock (flock), held briefly when rewriting the index; not a single-instance marker
├── docs/<id>.xml        One DocxXML file per document
├── assets/              Pasted or inserted images and attachments; assets/thumbs contains file preview thumbnails
└── trash/
    ├── index.json       ID to Trashed { meta, deleted, root }
    └── <id>.xml
```

### Meta fields

| Field | Meaning | When omitted |
|---|---|---|
| `created`, `modified`, `opened` | Unix seconds | Required; `opened` defaults to 0 |
| `pinned` | Pinned | false |
| `starred` | Time starred, or 0 if not starred | 0 |
| `parent` | Parent document ID in the document tree | Top level |
| `order` | Sibling sort key (milliseconds at the most recent move) | Sort by `created` |
| `width` | Page width: `standard` (820), `wide` (1020), or `full` | `standard` |
| `catalogue_closed` | Table of contents is collapsed | false |
| `reading` | Reading mode | false |
| `info` | Document information row: `{hidden, created, modified}` | Show only modification time |
| `refs_hidden` | Hide references list | false |

Add `#[serde(default)]` to every new field. Fields with default values are not written. Older versions ignore fields they do not recognize, and newer versions use defaults when reading older libraries.

### Reliability conventions

- **Atomic writes:** Files are first written to `<name>.tmp`, then `rename` replaces the original. After a crash, the disk contains either the old or the new content.
- **Index recovery:** Before saving `index.json`, the old file is copied to `.bak`. If parsing fails on read:
  - The damaged file is retained under the name `<name>.corrupt-<seconds>` and is not overwritten.
  - The application uses `.bak` and informs the user once.
  - If no usable `.bak` exists, the application uses an empty index. Documents remain in `docs/`, but metadata such as pinned state and hierarchy is lost.
- **Version marker:** When opening a library with a `format.json` version newer than the application, the application prompts the user to upgrade before editing. Increment `FORMAT` when a directory layout change would cause older versions to misread the library.
- **Manually added files:** XML files placed directly in `docs/`, such as DocxXML exported from Feishu, appear in the list.
- **Save failures:** Save failures are not silent.

## DocxXML

DocxXML is the Feishu and Lark document XML format. `crates/core/tests/fixtures/all-blocks.xml` contains the canonical form of each block. Reading and writing it must leave it byte-for-byte unchanged, as checked by `fixture_round_trips`.

### Write rules

- Each top-level block occupies one line. Children start on new lines, indented by two spaces per level.
- **Blocks in table cells are written on one line without indentation.** Newlines in content, such as code block lines, are written as `&#10;` to distinguish them from formatting newlines.
- An empty list item with child paragraphs is written as `<li><p></p>…`. Without the empty `<p>`, child paragraphs merge into the list item when read back.
- Folding is written as `folded="true"`. Icons and covers are written on `<title icon=… cover=… cover-x=… cover-y=…>`.

### Read rules

- Discard only **whitespace produced by source formatting**: a newline and indentation, and spaces and tabs before nested blocks. Other whitespace is content. For example, `<p> </p>` retains its space.
- Preserve unknown elements verbatim and write them back unchanged. Block-level unknown elements use `BlockKind::Raw`.
- Each inline object, including formulas, files, buttons, date reminders, @document references, and unknown inline elements, occupies one U+FFFC in text. Its style stores an `Inline { kind, xml }`:
  - `xml` is the original input, written back unchanged, so unknown attributes are preserved.
  - `kind` is the type parsed on input. Views and exports use only this type.
  - When the editor creates or modifies an object, `docxml::to_inline` constructs canonical XML and reads it back. The two remain consistent and unchanged after saving and reading back.

### Local extensions

These forms are application extensions absent from DocxXML. Other tools do not recognize them and treat them as unknown elements or attributes under the preceding rules:

- `<countdown duration start-time notify color timing/>`: Countdown. Cloud countdowns are widgets and are not part of DocxXML.
- `<time … is-whole-day="true">`: All-day reminder. The model has `isWholeDay`, but XML has no such attribute. A reminder without this attribute at 23:59 local time is also treated as all-day.
- Local image and attachment files use `path="…"` (the cloud uses `token` or `href`). `<figure><source path=… name=…/></figure>` represents a video or file.

### Known limitations

- **Trailing whitespace in cells:** If a block in a cell has nested children, trailing spaces or tabs in its text are lost on read. Older versions indented cell content, so the reader must discard whitespace before nested blocks for compatibility. The fuzzer's `round_trip` applies the same preprocessing (`trim_before_nested`).
- If blocks that cannot have children, such as code blocks, dividers, or images, have children in memory, those children are discarded on write. The editor prevents this structure; see the invariants in `ARCHITECTURE.md`.
