# Changelog

## Unreleased

- Fix a crash when opening destructive confirmation dialogs.

- Optimize the application crate in debug builds to reduce macOS unwind data.

- Make sidebar text and icons readable on fully transparent macOS glass.
- Fold the generated bibliography in the editor, and leave it out of Home cards and search.
- Add entries to the Library by DOI, arXiv ID, PMID, or ISBN.
- Add `just start` to build and run a debug version.

- Add a Library page for citations, backed by Hayagriva, with BibLaTeX import and export.
- Add a citation picker, and embed cited entries in the top-level document or link the library file.
- Preview an included file through the document that includes it.
- Update `#include`, `#import`, image, and bibliography paths when a file or folder is renamed or moved.
- Sort the sidebar with folders first and names in natural order, and show every file in the vault.
- Add drag and drop, new documents and folders inside folders, and separate file and folder menus.
- Open text files such as `.bib` in the editor, and other files in the system default app.
- Replace the block editor and DocxXML documents with a Typst source editor and live preview.
- Store documents as plain `.typ` files in a vault folder. Libraries from earlier builds are not read.
- Add PDF export through Typst, and a New Folder command.
- Remove document themes, covers, emoji icons, reminders, and Markdown export.
- Remove quick notes and their system-wide keyboard shortcuts.
- Replace the Nucleo interface icons with Lucide icons.
- Make the interface English-only and remove the language selector.
- Translate project documentation and bundled theme descriptions into English.
- Preserve existing document content and saved language preferences.
