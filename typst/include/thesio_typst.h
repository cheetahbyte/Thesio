/* The C ABI of thesio-typst (typst/src/ffi.rs), linked as libthesio_typst.a.
 *
 * Strings in are NUL-terminated UTF-8. Every char* out is owned by the caller
 * and freed with thesio_string_free; byte buffers with thesio_bytes_free.
 * JSON results are {"ok": ...} or {"error": "..."}. Offsets are UTF-8 bytes.
 * A ThesioCompilation is immutable and safe to use from any thread; a
 * ThesioCompiler is used from one thread at a time. A Rust panic never
 * crosses this boundary: it becomes an error or the documented fallback. */
#ifndef THESIO_TYPST_H
#define THESIO_TYPST_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ThesioCompiler ThesioCompiler;
typedef struct ThesioCompilation ThesioCompilation;

void thesio_string_free(char *text);
void thesio_bytes_free(uint8_t *data, size_t len);

/* The compiler. root is the vault folder; null when it is not UTF-8. */
ThesioCompiler *thesio_compiler_new(const char *root);
void thesio_compiler_free(ThesioCompiler *compiler);
/* {"ok": null}, or an error for an invalid vault path. */
char *thesio_compiler_set_buffer(ThesioCompiler *compiler, const char *path, const char *text);
void thesio_compiler_clear_buffer(ThesioCompiler *compiler, const char *path);
/* Null with *error set (free it) for an invalid path. Never touches the network. */
ThesioCompilation *thesio_compiler_compile(ThesioCompiler *compiler, const char *main, char **error);

/* A compilation. */
void thesio_compilation_free(ThesioCompilation *compilation);
/* {"ok": {main, ok, diagnostics: [{error, message, hints, path, start, end, line}],
 *  words: [{path, words}], labels: [{key, title, kind, page}], missing: [spec],
 *  pages: [{width, height, hash}]}}. Missing path is "", missing start/end/line -1,
 *  unknown page 0; width and height in points; hash is hex. */
char *thesio_compilation_info(const ThesioCompilation *compilation);
/* Downloads a package spec from missing, blocking; failures show in the next compile. */
void thesio_compilation_fetch(const ThesioCompilation *compilation, const char *spec);
/* Premultiplied RGBA, *len bytes; scale is pixels per point. Null with *error set. */
uint8_t *thesio_compilation_render(const ThesioCompilation *compilation, size_t page, float scale,
                                   uint32_t *width, uint32_t *height, size_t *len, char **error);
uint8_t *thesio_compilation_pdf(const ThesioCompilation *compilation, size_t *len, char **error);
/* {"ok": [{title, kind, detail, from, tail, insert, fields: [[start, end]]}]} */
char *thesio_compilation_complete(const ThesioCompilation *compilation, const char *path,
                                  const char *text, size_t caret, bool explicit_);
/* {"ok": {text, code}} or {"ok": null} */
char *thesio_compilation_hover(const ThesioCompilation *compilation, const char *path,
                               const char *text, size_t offset);
/* Zero-based page and y in points; false when on no page. */
bool thesio_compilation_position_of(const ThesioCompilation *compilation, const char *path,
                                    size_t offset, size_t *page, float *y);
/* {"ok": {path, offset, note}} (note non-empty for a note link) or {"ok": null} */
char *thesio_compilation_target_at(const ThesioCompilation *compilation, size_t page, float x, float y);
/* {"ok": [bool]} */
char *thesio_compilation_pages_from(const ThesioCompilation *compilation, const char *path);
/* {"ok": [diagnostic]} as in info, moved onto text */
char *thesio_compilation_placed(const ThesioCompilation *compilation, const char *path, const char *text);
/* offset itself on failure */
size_t thesio_compilation_moved(const ThesioCompilation *compilation, const char *path,
                                size_t offset, const char *text);

/* Stateless helpers over Typst's parser. */
/* {"ok": [{s, e, t}]}: start, end, tag name */
char *thesio_highlight(const char *text);
size_t thesio_prose_words(const char *text);
/* {"ok": {headings: [{offset, level, title}], includes: [{offset, literal}]}} */
char *thesio_parse_document(const char *text);
/* {"ok": [{literal, target, start, end}]} */
char *thesio_note_links(const char *text, const char *file);
bool thesio_note_link_at(const char *text, size_t caret, size_t *start, size_t *end);
bool thesio_citation_at(const char *text, size_t caret, size_t *start, size_t *end);
/* {"ok": [key]} */
char *thesio_cited_keys(const char *text);
/* {"ok": [offset]} */
char *thesio_citations_of(const char *text, const char *key);
/* {"ok": text} or {"ok": null} when unchanged */
char *thesio_rename_citation(const char *text, const char *old_key, const char *new_key, bool labelled);
bool thesio_defines_label(const char *text, const char *key);
/* files: JSON array of every vault path before the move of from to to.
 * {"ok": text} or {"ok": null} when unchanged */
char *thesio_rewrite_paths(const char *text, const char *old_file, const char *new_file, const char *main,
                           const char *files, const char *from, const char *to);
/* 0 none, 1 embedded, 2 linked, 3 custom */
int32_t thesio_bibliography_mode(const char *text, const char *file);
/* {"ok": style} or {"ok": null} */
char *thesio_bibliography_style(const char *text);
/* {"ok": text} or {"ok": null} without a #bibliography */
char *thesio_set_bibliography_style(const char *text, const char *style);
bool thesio_bibliography_block_range(const char *text, size_t *start, size_t *end);
/* Plain text, null on failure. */
char *thesio_without_bibliography_block(const char *text);
/* {"ok": text} or {"ok": null} when unchanged or without a block */
char *thesio_sync_bibliography_block(const char *text, const char *yaml);
/* Plain text, null on failure. */
char *thesio_embed_bibliography(const char *text, const char *yaml);
char *thesio_link_bibliography(const char *text, const char *file);
char *thesio_remove_bibliography(const char *text, const char *file);
/* at itself on failure */
size_t thesio_figure_place(const char *text, size_t at);
bool thesio_code_opens(const char *text, size_t start);
/* png, jpg, gif, webp, svg or pdf; null when Typst cannot show the bytes */
char *thesio_image_format(const uint8_t *data, size_t len);

/* Hayagriva. */
/* {"ok": null} when Typst reads yaml as a library */
char *thesio_library_check(const char *yaml);
/* {"ok": [{key, title, authors, date, parent_title}]} */
char *thesio_library_entries(const char *yaml);
/* {"ok": {yaml, verbatim: {key: {keywords, language}}}} */
char *thesio_biblatex_import(const char *bibtex);

#ifdef __cplusplus
}
#endif

#endif
