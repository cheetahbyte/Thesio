# Vault format

A vault is a plain folder, by default `~/Documents/Thesio`. Set `THESIO_HOME` to use another location. The vault root is the Typst project root, so `#include`, `#image`, and `#bibliography` can reach any file in it.

```
Thesio/
├── notes/reading.typ      Documents, in any folders you like
├── thesis.typ
├── library.yaml           The citation library, in Hayagriva format
├── references.bib         Any other files; text files open in the editor without a preview
├── .thesio/               App state
│   ├── format.json        {"format": 1}: store::FORMAT
│   ├── index.json         Vault path to Meta
│   ├── trash.json         Trash key to Trashed
│   ├── settings.json      Window state and open folders
│   ├── preferences.json   Appearance
│   └── shortcuts.json     Custom shortcuts
└── .trash/<key>/<name>    Deleted files and folders
```

Items whose names start with a dot are hidden from the sidebar. Deleting `.thesio/` loses pins, stars, and the Trash index, but no documents.

## Meta

Meta holds only what a file cannot. Titles come from file names, parents from folders, and timestamps from the file system.

| Field | Meaning | Default |
|---|---|---|
| `pinned` | Pinned in the sidebar | false |
| `opened` | Last opened, in Unix seconds | 0 |
| `starred` | Time starred, 0 when not starred | 0 |
| `page_offset` | Added to a PDF's page numbers to match the printed ones | none |

New fields use `#[serde(default)]`, and default values are not written.

## Reliability

- **Atomic writes.** Files are written to `<name>.tmp` and then renamed over the original.
- **Index recovery.** Before a JSON file is replaced, its previous version is copied to `<name>.bak`. A file that does not parse is kept as `<name>.corrupt-<seconds>`, and the backup is used.
- **Version marker.** A `format.json` newer than the app shows a warning.
- **Trash.** An item is recorded before it moves, so an interrupted delete leaves an extra entry instead of a lost file. Items expire after 30 days. Restoring to a name that is now taken uses a new name.
