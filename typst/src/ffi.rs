//! The C ABI, declared in `include/thesio_typst.h`. Strings in are
//! NUL-terminated UTF-8; strings out are owned by the caller and freed with
//! `thesio_string_free`, byte buffers with `thesio_bytes_free`. JSON results
//! are `{"ok": ...}` or `{"error": "..."}`. A panic never crosses the ABI: it
//! becomes an error, or the documented fallback for a scalar result.
//!
//! Safety, for every function here: pointers are null or valid for what the
//! header says, and a handle is not used after it is freed.
#![allow(clippy::missing_safety_doc)]

use crate::bibliography::{self, Mode};
use crate::compiler::{self, Compilation, Compiler, Diagnostic, PageTarget};
use crate::ide::CodeStart;
use crate::{images, library, paths, project, references, syntax, typst_source};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::str::FromStr;
use typst::syntax::package::PackageSpec;

pub struct ThesioCompiler(Compiler);

pub struct ThesioCompilation {
    inner: Compilation,
    /// `thesio_compilation_info`, computed once at compile.
    info: String,
}

const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<ThesioCompilation>();
};

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    let text = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned());
    format!("Typst failed: {}", text.unwrap_or_else(|| "unknown panic".into()))
}

fn guard<T>(fallback: T, work: impl FnOnce() -> T) -> T { catch_unwind(AssertUnwindSafe(work)).unwrap_or(fallback) }

fn owned(text: String) -> *mut c_char {
    CString::new(text).unwrap_or_else(|e| {
        let mut bytes = e.into_vec();
        bytes.retain(|b| *b != 0);
        CString::new(bytes).unwrap_or_default()
    }).into_raw()
}

/// Runs `work` and hands its value out as `{"ok": ...}`, or why it failed.
fn reply(work: impl FnOnce() -> Result<Value>) -> *mut c_char {
    let value = match catch_unwind(AssertUnwindSafe(work)) {
        Ok(Ok(value)) => json!({ "ok": value }),
        Ok(Err(error)) => json!({ "error": format!("{error:#}") }),
        Err(panic) => json!({ "error": panic_message(&*panic) }),
    };
    owned(value.to_string())
}

/// Plain text out, or null when `work` failed.
fn text_out(work: impl FnOnce() -> Result<String>) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(Ok(text)) => owned(text),
        _ => std::ptr::null_mut(),
    }
}

unsafe fn text<'a>(ptr: *const c_char) -> Result<&'a str> {
    anyhow::ensure!(!ptr.is_null(), "null string");
    unsafe { CStr::from_ptr(ptr) }.to_str().context("text is not UTF-8")
}

unsafe fn set_error(out: *mut *mut c_char, message: String) {
    if !out.is_null() { unsafe { *out = owned(message) }; }
}

unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() { unsafe { *out = value }; }
}

fn bytes_out(bytes: Vec<u8>, len: *mut usize) -> *mut u8 {
    unsafe { put(len, bytes.len()) };
    Box::into_raw(bytes.into_boxed_slice()) as *mut u8
}

fn diagnostic_json(d: &Diagnostic) -> Value {
    let (start, end) = d.range.as_ref().map_or((-1, -1), |r| (r.start as i64, r.end as i64));
    json!({
        "error": d.error, "message": d.message, "hints": d.hints, "path": d.path.as_deref().unwrap_or_default(),
        "start": start, "end": end, "line": d.line.map_or(-1, |line| line as i64),
    })
}

fn info_json(k: &Compilation) -> Value {
    let pages: Vec<Value> = k.document.as_ref().map_or(vec![], |document| (0..document.pages().len()).map(|i| {
        let size = document.pages()[i].frame.size();
        json!({ "width": size.x.to_pt() as f32, "height": size.y.to_pt() as f32, "hash": format!("{:032x}", compiler::page_hash(document, i)) })
    }).collect());
    json!({
        "main": k.main,
        "ok": k.document.is_some(),
        "diagnostics": k.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
        "words": k.words.iter().map(|(path, words)| json!({ "path": path, "words": words })).collect::<Vec<_>>(),
        "labels": k.labels.iter().map(|l| json!({ "key": l.key, "title": l.title, "kind": l.kind, "page": l.page.unwrap_or(0) })).collect::<Vec<_>>(),
        "missing": k.missing.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "pages": pages,
    })
}

// The compiler.

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_string_free(text: *mut c_char) {
    if !text.is_null() { drop(unsafe { CString::from_raw(text) }); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_bytes_free(data: *mut u8, len: usize) {
    if !data.is_null() { drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, len)) }); }
}

/// Null when `root` is not UTF-8 or setup panicked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compiler_new(root: *const c_char) -> *mut ThesioCompiler {
    guard(std::ptr::null_mut(), || match unsafe { text(root) } {
        Ok(root) => Box::into_raw(Box::new(ThesioCompiler(Compiler::new(root)))),
        Err(_) => std::ptr::null_mut(),
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compiler_free(compiler: *mut ThesioCompiler) {
    if !compiler.is_null() { guard((), || drop(unsafe { Box::from_raw(compiler) })); }
}

/// `{"ok": null}`, or an error for an invalid path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compiler_set_buffer(compiler: *mut ThesioCompiler, path: *const c_char, source: *const c_char) -> *mut c_char {
    reply(|| {
        let compiler = unsafe { compiler.as_mut() }.context("null compiler")?;
        compiler.0.set_buffer(unsafe { text(path) }?, unsafe { text(source) }?)?;
        Ok(Value::Null)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compiler_clear_buffer(compiler: *mut ThesioCompiler, path: *const c_char) {
    guard((), || {
        if let (Some(compiler), Ok(path)) = (unsafe { compiler.as_mut() }, unsafe { text(path) }) { compiler.0.clear_buffer(path); }
    })
}

/// Compiles `main` on a thread of its own. Null with `*error` set when the
/// path is invalid or compiling panicked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compiler_compile(compiler: *mut ThesioCompiler, main: *const c_char, error: *mut *mut c_char) -> *mut ThesioCompilation {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<ThesioCompilation> {
        let compiler = unsafe { compiler.as_mut() }.context("null compiler")?;
        let inner = compiler.0.compile(unsafe { text(main) }?)?;
        let info = json!({ "ok": info_json(&inner) }).to_string();
        Ok(ThesioCompilation { inner, info })
    }));
    match result {
        Ok(Ok(compilation)) => Box::into_raw(Box::new(compilation)),
        Ok(Err(e)) => { unsafe { set_error(error, format!("{e:#}")) }; std::ptr::null_mut() }
        Err(panic) => { unsafe { set_error(error, panic_message(&*panic)) }; std::ptr::null_mut() }
    }
}

// A compilation.

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_free(compilation: *mut ThesioCompilation) {
    if !compilation.is_null() { guard((), || drop(unsafe { Box::from_raw(compilation) })); }
}

unsafe fn inner<'a>(ptr: *const ThesioCompilation) -> Result<&'a Compilation> {
    Ok(&unsafe { ptr.as_ref() }.context("null compilation")?.inner)
}

/// `{"ok": {main, ok, diagnostics, words, labels, missing, pages}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_info(compilation: *const ThesioCompilation) -> *mut c_char {
    match unsafe { compilation.as_ref() } {
        Some(compilation) => owned(compilation.info.clone()),
        None => reply(|| anyhow::bail!("null compilation")),
    }
}

/// Downloads one of `missing`, blocking. A failure shows in the next compile.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_fetch(compilation: *const ThesioCompilation, spec: *const c_char) {
    guard((), || {
        let Ok(k) = (unsafe { inner(compilation) }) else { return };
        if let Ok(spec) = unsafe { text(spec) }.and_then(|spec| PackageSpec::from_str(spec).map_err(|e| anyhow::anyhow!("{e}"))) { k.fetch(&spec); }
    })
}

/// Premultiplied RGBA, row by row, `*len` bytes. Null with `*error` set when
/// there is no such page.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_render(compilation: *const ThesioCompilation, page: usize, scale: f32, width: *mut u32, height: *mut u32, len: *mut usize, error: *mut *mut c_char) -> *mut u8 {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<compiler::PageImage> {
        let k = unsafe { inner(compilation) }?;
        let document = k.document.as_ref().context("the document did not compile")?;
        anyhow::ensure!(scale.is_finite() && scale > 0.0, "invalid scale {scale}");
        compiler::render(document, page, scale)?.with_context(|| format!("no page {page}"))
    }));
    match result {
        Ok(Ok(image)) => {
            unsafe { put(width, image.width); put(height, image.height) };
            bytes_out(image.rgba, len)
        }
        Ok(Err(e)) => { unsafe { set_error(error, format!("{e:#}")) }; std::ptr::null_mut() }
        Err(panic) => { unsafe { set_error(error, panic_message(&*panic)) }; std::ptr::null_mut() }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_pdf(compilation: *const ThesioCompilation, len: *mut usize, error: *mut *mut c_char) -> *mut u8 {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<Vec<u8>> {
        let k = unsafe { inner(compilation) }?;
        compiler::export_pdf(k.document.as_ref().context("the document did not compile")?)
    }));
    match result {
        Ok(Ok(bytes)) => bytes_out(bytes, len),
        Ok(Err(e)) => { unsafe { set_error(error, format!("{e:#}")) }; std::ptr::null_mut() }
        Err(panic) => { unsafe { set_error(error, panic_message(&*panic)) }; std::ptr::null_mut() }
    }
}

/// `{"ok": [{title, kind, detail, from, tail, insert, fields: [[start, end]]}]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_complete(compilation: *const ThesioCompilation, path: *const c_char, source: *const c_char, caret: usize, explicit: bool) -> *mut c_char {
    reply(|| {
        let k = unsafe { inner(compilation) }?;
        let found = k.complete(unsafe { text(path) }?, unsafe { text(source) }?, caret, explicit);
        Ok(found.into_iter().map(|s| json!({
            "title": s.title, "kind": s.kind, "detail": s.detail.unwrap_or_default(), "from": s.from, "tail": s.tail,
            "insert": s.insert.text, "fields": s.insert.fields.iter().map(|f| [f.start, f.end]).collect::<Vec<_>>(),
        })).collect())
    })
}

/// `{"ok": {text, code}}`, or `{"ok": null}` when there is nothing to say.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_hover(compilation: *const ThesioCompilation, path: *const c_char, source: *const c_char, offset: usize) -> *mut c_char {
    reply(|| {
        let k = unsafe { inner(compilation) }?;
        Ok(k.hover(unsafe { text(path) }?, unsafe { text(source) }?, offset).map_or(Value::Null, |h| json!({ "text": h.text, "code": h.code })))
    })
}

/// Zero-based page and height in points; false when it is on no page.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_position_of(compilation: *const ThesioCompilation, path: *const c_char, offset: usize, page: *mut usize, y: *mut f32) -> bool {
    guard(false, || {
        let (Ok(k), Ok(path)) = (unsafe { inner(compilation) }, unsafe { text(path) }) else { return false };
        let Some((at, height)) = k.position_of(path, offset) else { return false };
        unsafe { put(page, at); put(y, height) };
        true
    })
}

/// `{"ok": {path, offset, note}}` with `note` set for a note link, or `{"ok": null}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_target_at(compilation: *const ThesioCompilation, page: usize, x: f32, y: f32) -> *mut c_char {
    reply(|| {
        let k = unsafe { inner(compilation) }?;
        Ok(match k.target_at(page, x, y) {
            Some(PageTarget::Source(path, offset)) => json!({ "path": path, "offset": offset, "note": "" }),
            Some(PageTarget::Note(note)) => json!({ "path": "", "offset": 0, "note": note }),
            None => Value::Null,
        })
    })
}

/// `{"ok": [bool]}`, one per page.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_pages_from(compilation: *const ThesioCompilation, path: *const c_char) -> *mut c_char {
    reply(|| Ok(json!(unsafe { inner(compilation) }?.pages_from(unsafe { text(path) }?))))
}

/// `{"ok": [diagnostic]}`: this compile's diagnostics in `path`, moved onto `source`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_placed(compilation: *const ThesioCompilation, path: *const c_char, source: *const c_char) -> *mut c_char {
    reply(|| {
        let k = unsafe { inner(compilation) }?;
        Ok(k.placed(&k.diagnostics, unsafe { text(path) }?, unsafe { text(source) }?).iter().map(diagnostic_json).collect())
    })
}

/// `offset` itself when anything fails.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_compilation_moved(compilation: *const ThesioCompilation, path: *const c_char, offset: usize, source: *const c_char) -> usize {
    guard(offset, || {
        let (Ok(k), Ok(path), Ok(source)) = (unsafe { inner(compilation) }, unsafe { text(path) }, unsafe { text(source) }) else { return offset };
        k.moved(path, offset, source)
    })
}

// Stateless helpers over Typst's parser.

/// `{"ok": [{s, e, t}]}`: byte range and tag name of each highlighted leaf.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_highlight(source: *const c_char) -> *mut c_char {
    reply(|| Ok(syntax::highlights(unsafe { text(source) }?).into_iter().map(|(r, tag)| json!({ "s": r.start, "e": r.end, "t": syntax::tag_name(tag) })).collect()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_prose_words(source: *const c_char) -> usize {
    guard(0, || unsafe { text(source) }.map_or(0, typst_source::prose_words))
}

/// `{"ok": {headings: [{offset, level, title}], includes: [{offset, literal}]}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_parse_document(source: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed = project::parse_document(unsafe { text(source) }?);
        Ok(json!({
            "headings": parsed.headings.iter().map(|h| json!({ "offset": h.offset, "level": h.level, "title": h.title })).collect::<Vec<_>>(),
            "includes": parsed.includes.iter().map(|(offset, literal)| json!({ "offset": offset, "literal": literal })).collect::<Vec<_>>(),
        }))
    })
}

/// `{"ok": [{literal, target, start, end}]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_note_links(source: *const c_char, file: *const c_char) -> *mut c_char {
    reply(|| Ok(references::note_links(unsafe { text(source) }?, unsafe { text(file) }?).into_iter()
        .map(|l| json!({ "literal": l.literal, "target": l.target, "start": l.range.start, "end": l.range.end })).collect()))
}

unsafe fn range_out(range: Option<std::ops::Range<usize>>, start: *mut usize, end: *mut usize) -> bool {
    let Some(range) = range else { return false };
    unsafe { put(start, range.start); put(end, range.end) };
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_note_link_at(source: *const c_char, caret: usize, start: *mut usize, end: *mut usize) -> bool {
    guard(false, || unsafe { range_out(text(source).ok().and_then(|source| references::note_link_at(source, caret)), start, end) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_citation_at(source: *const c_char, caret: usize, start: *mut usize, end: *mut usize) -> bool {
    guard(false, || unsafe { range_out(text(source).ok().and_then(|source| references::citation_at(source, caret)), start, end) })
}

/// `{"ok": [key]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_cited_keys(source: *const c_char) -> *mut c_char {
    reply(|| Ok(json!(references::cited_keys(unsafe { text(source) }?))))
}

/// `{"ok": [offset]}`: the byte after each `@` or `<` citing `key`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_citations_of(source: *const c_char, key: *const c_char) -> *mut c_char {
    reply(|| Ok(json!(references::citations_of(unsafe { text(source) }?, unsafe { text(key) }?))))
}

/// `{"ok": text}`, or `{"ok": null}` when nothing cites `old`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_rename_citation(source: *const c_char, old: *const c_char, new: *const c_char, labelled: bool) -> *mut c_char {
    reply(|| Ok(json!(references::rename_citation(unsafe { text(source) }?, unsafe { text(old) }?, unsafe { text(new) }?, labelled))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_defines_label(source: *const c_char, key: *const c_char) -> bool {
    guard(false, || match unsafe { (text(source), text(key)) } {
        (Ok(source), Ok(key)) => typst_source::defines_label(source, key),
        _ => false,
    })
}

/// `{"ok": text}`, or `{"ok": null}` when no path changed. `files` is a JSON
/// array of every vault path before the move of `from` to `to`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_rewrite_paths(source: *const c_char, old_file: *const c_char, new_file: *const c_char, main: *const c_char, files: *const c_char, from: *const c_char, to: *const c_char) -> *mut c_char {
    reply(|| {
        let files: rustc_hash::FxHashSet<String> = serde_json::from_str(unsafe { text(files) }?).context("files is not a JSON array of strings")?;
        let (from, to) = unsafe { (text(from)?, text(to)?) };
        let rewritten = references::rewrite_paths(unsafe { text(source) }?, unsafe { text(old_file) }?, unsafe { text(new_file) }?, unsafe { text(main) }?,
            |path| files.contains(path), |path| paths::moved(path, from, to));
        Ok(json!(rewritten))
    })
}

/// 0 none, 1 embedded, 2 linked, 3 custom.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_bibliography_mode(source: *const c_char, file: *const c_char) -> i32 {
    guard(0, || match unsafe { (text(source), text(file)) } {
        (Ok(source), Ok(file)) => match bibliography::mode(source, file) { Mode::None => 0, Mode::Embedded => 1, Mode::Linked => 2, Mode::Custom => 3 },
        _ => 0,
    })
}

/// `{"ok": style}`, or `{"ok": null}` when the call names none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_bibliography_style(source: *const c_char) -> *mut c_char {
    reply(|| Ok(json!(bibliography::style(unsafe { text(source) }?))))
}

/// `{"ok": text}`, or `{"ok": null}` without a `#bibliography`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_set_bibliography_style(source: *const c_char, style: *const c_char) -> *mut c_char {
    reply(|| Ok(json!(bibliography::set_style(unsafe { text(source) }?, unsafe { text(style) }?))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_bibliography_block_range(source: *const c_char, start: *mut usize, end: *mut usize) -> bool {
    guard(false, || unsafe { range_out(text(source).ok().and_then(bibliography::block_range), start, end) })
}

/// Null on failure, as for each plain-text result below.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_without_bibliography_block(source: *const c_char) -> *mut c_char {
    text_out(|| Ok(bibliography::without_block(unsafe { text(source) }?)))
}

/// `{"ok": text}`, or `{"ok": null}` when the block already holds `yaml` or there is none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_sync_bibliography_block(source: *const c_char, yaml: *const c_char) -> *mut c_char {
    reply(|| Ok(json!(bibliography::sync_block(unsafe { text(source) }?, unsafe { text(yaml) }?))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_embed_bibliography(source: *const c_char, yaml: *const c_char) -> *mut c_char {
    text_out(|| Ok(bibliography::embed(unsafe { text(source) }?, unsafe { text(yaml) }?)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_link_bibliography(source: *const c_char, file: *const c_char) -> *mut c_char {
    text_out(|| Ok(bibliography::link(unsafe { text(source) }?, unsafe { text(file) }?)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_remove_bibliography(source: *const c_char, file: *const c_char) -> *mut c_char {
    text_out(|| Ok(bibliography::remove(unsafe { text(source) }?, unsafe { text(file) }?)))
}

/// `at` itself when anything fails.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_figure_place(source: *const c_char, at: usize) -> usize {
    guard(at, || unsafe { text(source) }.map_or(at, |source| images::figure_place(source, at)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_code_opens(source: *const c_char, start: usize) -> bool {
    guard(false, || unsafe { text(source) }.is_ok_and(|source| CodeStart::default().opens(source, start)))
}

/// The usual extension of what Typst reads the bytes as, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_image_format(data: *const u8, len: usize) -> *mut c_char {
    guard(std::ptr::null_mut(), || {
        if data.is_null() { return std::ptr::null_mut(); }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        images::image_format(bytes).map_or(std::ptr::null_mut(), |extension| owned(extension.to_string()))
    })
}

// Hayagriva.

/// `{"ok": null}` when Typst can read `yaml` as a library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_library_check(yaml: *const c_char) -> *mut c_char {
    reply(|| library::check(unsafe { text(yaml) }?).map(|_| Value::Null))
}

/// `{"ok": [{key, title, authors, date, parent_title}]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_library_entries(yaml: *const c_char) -> *mut c_char {
    reply(|| Ok(serde_json::to_value(library::entries(unsafe { text(yaml) }?)?)?))
}

/// `{"ok": {yaml, verbatim: {key: {keywords, language}}}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn thesio_biblatex_import(bibtex: *const c_char) -> *mut c_char {
    reply(|| {
        let (yaml, verbatim) = library::import_biblatex(unsafe { text(bibtex) }?)?;
        Ok(json!({ "yaml": yaml, "verbatim": verbatim }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(ptr: *mut c_char) -> Value {
        let value = serde_json::from_str(unsafe { CStr::from_ptr(ptr) }.to_str().unwrap()).unwrap();
        unsafe { thesio_string_free(ptr) };
        value
    }

    #[test]
    fn a_panic_becomes_an_error() {
        let value = read(reply(|| panic!("boom")));
        assert_eq!(value["error"], "Typst failed: boom");
    }

    #[test]
    fn invalid_text_is_an_error_and_null_is_refused() {
        let bad = c"\xff";
        assert!(read(unsafe { thesio_highlight(bad.as_ptr()) })["error"].is_string());
        assert!(read(unsafe { thesio_compilation_info(std::ptr::null()) })["error"].is_string());
        assert!(unsafe { thesio_compiler_new(std::ptr::null()) }.is_null());
    }

    #[test]
    fn a_compilation_answers_through_the_abi() {
        let root = std::env::temp_dir().join(format!("thesio-ffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.typ"), "= Hello\nWorld.\n").unwrap();
        let root = CString::new(root.to_str().unwrap()).unwrap();
        unsafe {
            let compiler = thesio_compiler_new(root.as_ptr());
            let mut error = std::ptr::null_mut();
            let k = thesio_compiler_compile(compiler, c"main.typ".as_ptr(), &mut error);
            assert!(!k.is_null() && error.is_null());
            let info = read(thesio_compilation_info(k));
            assert_eq!(info["ok"]["ok"], true);
            assert_eq!(info["ok"]["pages"].as_array().unwrap().len(), 1);
            let (mut w, mut h, mut len) = (0, 0, 0);
            let rgba = thesio_compilation_render(k, 0, 1.0, &mut w, &mut h, &mut len, &mut error);
            assert!(!rgba.is_null() && len == (w * h * 4) as usize);
            thesio_bytes_free(rgba, len);
            assert!(thesio_compilation_render(k, 5, 1.0, &mut w, &mut h, &mut len, &mut error).is_null());
            thesio_string_free(error);
            let mut error = std::ptr::null_mut();
            assert!(thesio_compiler_compile(compiler, c"../out.typ".as_ptr(), &mut error).is_null() && !error.is_null());
            thesio_string_free(error);
            thesio_compilation_free(k);
            thesio_compiler_free(compiler);
        }
    }
}
