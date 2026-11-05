# Changelog

## Unreleased

- Redesign the interface: native SF Symbols on macOS, calmer buttons and fields, and grouped forms.
- Simplify the Library toolbar: Add opens Add by Identifier, with manual entry inside it, and Import and Export move to a "…" menu.
- Show empty and no-result states in the Library, and prose instead of Typst source on Home cards.
- Complete citations in the editor: typing `@` offers matching Library entries, and accepting one adds a bibliography if the document has none.
- Add a Sources page. Adding a source downloads an open-access PDF to `sources/`, from arXiv or, with an email in Settings, Unpaywall.
- Show papers inside the app on macOS, with text selection and zoom.
- Add PDFs by hand from Sources or a Library row, and link unlinked papers to a source.
- Add a right-click menu to Library rows: Edit, Cite, Copy Key, and Delete.
- Remove file icons from the sidebar; folders keep theirs.
- Remove the empty General settings page.
- Hide `library.yaml` and `sources/` from the sidebar by default, with a Documents setting to show them.
- Fix the dialog backdrop brightening the window in dark mode.

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
