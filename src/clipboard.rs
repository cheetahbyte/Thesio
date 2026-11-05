//! The system pasteboard beyond what GPUI reads.

/// Devtools stand-in for the pasteboard, (HTML, plain text): scripted tests
/// must not touch the shared system clipboard.
pub static FAKE_CLIPBOARD: std::sync::Mutex<Option<(Option<String>, String)>> = std::sync::Mutex::new(None);


#[cfg(not(target_os = "macos"))]
pub fn clipboard_html() -> Option<String> {
    None
}
