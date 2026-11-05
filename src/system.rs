//! What reaches outside the app: file dialogs, other apps, the clipboard. A
//! scripted run (`devtools.rs`) stops here: its answers stand in for the system.

use anyhow::Result;
use gpui::*;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use thesio_core::images::{Incoming, named_as_image};

static PICK: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);
static PDF_SELECTION: Mutex<Option<String>> = Mutex::new(None);
static CLIPBOARD: Mutex<Option<Clip>> = Mutex::new(None);

/// What a scripted clipboard holds.
#[derive(Clone)]
pub(crate) enum Clip {
    Text(String),
    Image(Vec<u8>),
    Files(Vec<PathBuf>),
}

pub fn scripted() -> bool {
    static SCRIPTED: OnceLock<bool> = OnceLock::new();
    *SCRIPTED.get_or_init(|| std::env::var_os("THESIO_SCRIPT").is_some())
}

fn scripted_pick() -> Option<Vec<PathBuf>> {
    let paths = PICK.lock().unwrap().take();
    if paths.is_none() { eprintln!("devtools: a file dialog opened with no `pick` waiting"); }
    paths
}

/// The files chosen in an open dialog, if any. `extension` limits them.
pub fn pick_files(extension: Option<&'static str>, multiple: bool, cx: &App) -> impl Future<Output = Result<Option<Vec<PathBuf>>>> + use<> {
    let extensions = extension.map(|extension| vec![extension.into()]);
    let dialog = (!scripted()).then(|| cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple, prompt: None, extensions }));
    async move {
        match dialog {
            Some(dialog) => dialog.await.unwrap_or(Ok(None)),
            None => Ok(scripted_pick()),
        }
    }
}

/// The path chosen in a save dialog, if any.
pub fn pick_new_path(directory: &Path, name: &str, cx: &App) -> impl Future<Output = Result<Option<PathBuf>>> + use<> {
    let dialog = (!scripted()).then(|| cx.prompt_for_new_path(directory, Some(name)));
    async move {
        match dialog {
            Some(dialog) => dialog.await.unwrap_or(Ok(None)),
            None => Ok(scripted_pick().and_then(|paths| paths.into_iter().next())),
        }
    }
}

pub fn open_with_system(path: &Path, cx: &App) {
    if !scripted() { cx.open_with_system(path); }
}

/// What the action that shows a file in the system's file manager is called.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") { "Show in Finder" } else if cfg!(target_os = "windows") { "Show in File Explorer" } else { "Show in File Manager" };

pub fn reveal(path: &Path, cx: &App) {
    if !scripted() { cx.reveal_path(path); }
}

pub fn copy(text: String, cx: &App) {
    if scripted() { *CLIPBOARD.lock().unwrap() = Some(Clip::Text(text)); } else { cx.write_to_clipboard(ClipboardItem::new_string(text)); }
}

pub fn paste(cx: &App) -> Option<String> {
    if scripted() {
        return match CLIPBOARD.lock().unwrap().clone() { Some(Clip::Text(text)) => Some(text), _ => None };
    }
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn image_files(files: Vec<PathBuf>) -> Vec<Incoming> {
    files.into_iter().filter(|path| named_as_image(path)).map(Incoming::File).collect()
}

/// The images a paste brings, if it brings any. Files come first: a copied file
/// also holds its name as text and its icon as an image. Beyond that GPUI puts
/// text ahead of a bitmap, so what was copied with text pastes as text.
pub fn pasted_images(cx: &App) -> Vec<Incoming> {
    if scripted() {
        return match CLIPBOARD.lock().unwrap().clone() {
            Some(Clip::Image(bytes)) => vec![Incoming::Pasted(bytes)],
            Some(Clip::Files(files)) => image_files(files),
            _ => vec![],
        };
    }
    let files = image_files(clipboard_files());
    if !files.is_empty() { return files; }
    cx.read_from_clipboard().map_or(vec![], |item| item.into_entries().filter_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(Incoming::Pasted(image.bytes)),
        _ => None,
    }).collect())
}

/// GPUI's clipboard knows text and images, not files.
#[cfg(target_os = "macos")]
fn clipboard_files() -> Vec<PathBuf> {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    let pasteboard: *mut Object = unsafe { msg_send![class!(NSPasteboard), generalPasteboard] };
    pasteboard_files(pasteboard)
}

#[cfg(not(target_os = "macos"))]
fn clipboard_files() -> Vec<PathBuf> { vec![] }

#[cfg(target_os = "macos")]
fn pasteboard_files(pasteboard: *mut objc::runtime::Object) -> Vec<PathBuf> {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    use std::ffi::{CStr, c_char};
    unsafe extern "C" { static NSPasteboardURLReadingFileURLsOnlyKey: *mut Object; }
    objc::rc::autoreleasepool(|| unsafe {
        let classes: *mut Object = msg_send![class!(NSArray), arrayWithObject: class!(NSURL)];
        let yes: *mut Object = msg_send![class!(NSNumber), numberWithBool: true];
        let options: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObject: yes forKey: NSPasteboardURLReadingFileURLsOnlyKey];
        let urls: *mut Object = msg_send![pasteboard, readObjectsForClasses: classes options: options];
        if urls.is_null() { return vec![]; }
        let count: usize = msg_send![urls, count];
        (0..count).filter_map(|i| {
            let url: *mut Object = msg_send![urls, objectAtIndex: i];
            let path: *mut Object = msg_send![url, path];
            if path.is_null() { return None; }
            let text: *const c_char = msg_send![path, UTF8String];
            Some(PathBuf::from(CStr::from_ptr(text).to_str().ok()?))
        }).collect()
    })
}

/// Text to select in the open paper before the next highlight: mouse steps cannot reach the native viewer.
pub fn take_pdf_selection() -> Option<String> {
    PDF_SELECTION.lock().unwrap().take()
}

/// `pick`: answers the next dialog.
pub(crate) fn script_pick(paths: Vec<PathBuf>) { *PICK.lock().unwrap() = Some(paths); }
/// `pdfselect`.
pub(crate) fn script_pdf_selection(text: String) { *PDF_SELECTION.lock().unwrap() = Some(text); }
/// `clip`: what the next paste reads.
pub(crate) fn script_clipboard(clip: Clip) { *CLIPBOARD.lock().unwrap() = Some(clip); }

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::pasteboard_files;
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    use std::path::PathBuf;

    /// A pasteboard of the test's own: the user's clipboard stays out of it.
    fn pasteboard_holding(urls: &[*mut Object]) -> *mut Object {
        unsafe {
            let pasteboard: *mut Object = msg_send![class!(NSPasteboard), pasteboardWithUniqueName];
            let list: *mut Object = msg_send![class!(NSArray), arrayWithObjects: urls.as_ptr() count: urls.len()];
            let _: isize = msg_send![pasteboard, clearContents];
            let written: bool = msg_send![pasteboard, writeObjects: list];
            assert!(written);
            pasteboard
        }
    }

    fn ns_string(text: &str) -> *mut Object {
        let text = std::ffi::CString::new(text).unwrap();
        unsafe { msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()] }
    }

    #[test]
    fn copied_files_are_read_and_web_addresses_are_not() {
        let (file, web): (*mut Object, *mut Object) = unsafe { (msg_send![class!(NSURL), fileURLWithPath: ns_string("/tmp/a b/plot.png")], msg_send![class!(NSURL), URLWithString: ns_string("https://example.com/plot.png")]) };
        let files = pasteboard_holding(&[file]);
        let webs = pasteboard_holding(&[web]);
        assert_eq!(pasteboard_files(files), [PathBuf::from("/tmp/a b/plot.png")]);
        assert!(pasteboard_files(webs).is_empty());
        for pasteboard in [files, webs] { let _: () = unsafe { msg_send![pasteboard, releaseGlobally] }; }
    }
}
