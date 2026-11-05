//! Light, dark, and the system accent.

use crate::preferences::prefs;
use crate::theme;
use gpui::*;

/// Tell the OS which look the app wants (macOS: the window material, menus
/// and dialogs follow it); empty follows the system.
pub fn apply_appearance(pref: &str) {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc::{class, msg_send, sel, sel_impl};
        use objc::runtime::Object;
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let name = match pref { "light" => Some(c"NSAppearanceNameAqua"), "dark" => Some(c"NSAppearanceNameDarkAqua"), _ => None };
        let look: *mut Object = match name {
            Some(n) => { let s: *mut Object = msg_send![class!(NSString), stringWithUTF8String: n.as_ptr()]; msg_send![class!(NSAppearance), appearanceNamed: s] }
            None => std::ptr::null_mut(),
        };
        let _: () = msg_send![app, setAppearance: look];
    }
    #[cfg(not(target_os = "macos"))]
    let _ = pref;
}

/// Draw light or dark as the preference and the system say; true when the look changed.
pub fn sync_dark(cx: &App) -> bool {
    let on = match prefs().appearance.as_str() {
        "light" => false,
        "dark" => true,
        _ => matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark),
    };
    theme::set_dark(on)
}

/// Redraws every window after the theme changed.
pub(crate) fn refresh_all(cx: &mut App) {
    crate::app::set_menus(cx);
    cx.refresh_windows();
}
