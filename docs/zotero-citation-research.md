# Zotero and citation libraries in Rust

## Conclusion

Evaluate Hayagriva for citation rendering and Bib(La)TeX import. Treat Zotero access as a separate integration: start with its local API for desktop reads, or its web API for remote access. No examined crate establishes a complete offline, two-way sync engine. Validate metadata conversion, citation output, and dependency fit with representative documents before choosing dependencies.

## Zotero access and synchronization

- [`libzotero`](https://github.com/byte-ranch/libzotero) offers local and web API clients for reading user and group items and collections. Its authors describe web support as incomplete and not fully tested; [version 0.1.0](https://docs.rs/crate/libzotero/0.1.0) signals an early API. Evaluate it for local reads, not as a proven sync solution.
- [`papers-zotero`](https://docs.rs/papers-zotero/0.3.1/papers_zotero/) provides asynchronous web API read and write endpoints, including deleted keys and versioned responses. Its [package documentation](https://docs.rs/crate/papers-zotero/0.3.1) calls it an internal crate whose API can change without notice. Endpoint coverage alone does not establish conflict handling or complete synchronization.
- [`zotero` 0.0.2](https://docs.rs/crate/zotero/0.0.2) exposes item and collection operations, but its published release dates from 2019 and uses `reqwest` 0.9. Its [repository](https://github.com/eonm/zotero) shows a different, newer API from the published crate. Assess the exact revision before using it.

Zotero's [sync protocol](https://www.zotero.org/support/dev/web_api/v3/syncing) requires library and object versions, dirty state, deletions, and handling stale-write conflicts. A client library for API requests is not itself evidence of this state machine. Attachment synchronization needs separate evaluation.

The [local API](https://www.zotero.org/support/dev/web_api/v3/local_api) works offline but must be enabled in Zotero. Local writes require Zotero 10 or later and user authorization. Zotero 10 local object versions are distinct from web versions; don't share version checkpoints between them. Zotero recommends API access over [direct SQLite access](https://www.zotero.org/support/dev/client_coding/direct_sqlite_database_access), which must be read-only and uses a schema that can change.

## Bibliographic data and formatting

- [`biblatex`](https://docs.rs/biblatex/latest/biblatex/) parses BibTeX and BibLaTeX into bibliographic entries. Parsing is distinct from Zotero synchronization and Citation Style Language (CSL) rendering.
- [Hayagriva](https://github.com/typst/hayagriva) provides a bibliography model, YAML storage, Bib(La)TeX parsing with default features, and CSL citation and bibliography formatting. It accepts external CSL styles and locales; its optional archive bundles styles. Its [release history](https://docs.rs/crate/hayagriva/latest) spans multiple years, but it remains pre-1.0. Style support is the project's claim, not an independent correctness assessment. Check [archive licensing](https://docs.rs/crate/hayagriva/latest) before bundling styles.
- Zotero's [`citeproc-rs`](https://github.com/zotero/citeproc-rs) is a CSL processor, not a Zotero synchronization client. Its README calls it work in progress, says not all CSL tests pass, and describes the Rust processor API as not yet published on crates.io. Assess the current repository state before relying on it.

Zotero's [data model](https://www.zotero.org/support/dev/client_coding/direct_sqlite_database_access) includes item-specific fields, notes, attachments, tags, and related items. BibTeX export and CSL rendering are different representations, not guaranteed lossless substitutes for that model. Preserve source identifiers and test round trips and field mappings before relying on conversion. This is especially relevant because [PRODUCT.md](../PRODUCT.md) names BibTeX export as a goal.

## Verification limits

Findings are based on the linked maintainers' documentation and Zotero's specifications. No crates were integrated, benchmarked, or tested against a live Zotero library. Completeness, metadata fidelity, CSL conformance, and current maintenance beyond the cited evidence remain unverified.
