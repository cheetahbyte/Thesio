//! What reaches outside the app: file dialogs, other apps, the clipboard. A
//! scripted run (`devtools.rs`) stops here: its answers stand in for the system.

use gpui::*;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static PICK: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);
static PDF_SELECTION: Mutex<Option<String>> = Mutex::new(None);
static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);

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
pub fn pick_files(extension: Option<&'static str>, multiple: bool, cx: &App) -> impl Future<Output = Option<Vec<PathBuf>>> + use<> {
    let extensions = extension.map(|extension| vec![extension.into()]);
    let dialog = (!scripted()).then(|| cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple, prompt: None, extensions }));
    async move {
        match dialog {
            Some(dialog) => dialog.await.ok().and_then(Result::ok).flatten(),
            None => scripted_pick(),
        }
    }
}

/// The path chosen in a save dialog, if any.
pub fn pick_new_path(directory: &Path, name: &str, cx: &App) -> impl Future<Output = Option<PathBuf>> + use<> {
    let dialog = (!scripted()).then(|| cx.prompt_for_new_path(directory, Some(name)));
    async move {
        match dialog {
            Some(dialog) => dialog.await.ok().and_then(Result::ok).flatten(),
            None => scripted_pick().and_then(|paths| paths.into_iter().next()),
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
    if scripted() { *CLIPBOARD.lock().unwrap() = Some(text); } else { cx.write_to_clipboard(ClipboardItem::new_string(text)); }
}

pub fn paste(cx: &App) -> Option<String> {
    if scripted() { CLIPBOARD.lock().unwrap().clone() } else { cx.read_from_clipboard().and_then(|item| item.text()) }
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
pub(crate) fn script_clipboard(text: String) { *CLIPBOARD.lock().unwrap() = Some(text); }
