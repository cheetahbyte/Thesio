//! Light, dark, and the system accent.

use crate::app::Workspace;
use crate::preferences::{self, Preferences, prefs};
use crate::theme;
use crate::ui::ToastKind;
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

/// Read the system accent; true when it changed. Call on the main thread.
#[cfg(target_os = "macos")]
pub fn sync_accent() -> bool {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    let hex = objc::rc::autoreleasepool(|| unsafe {
        let accent: *mut Object = msg_send![class!(NSColor), controlAccentColor];
        let space: *mut Object = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let color: *mut Object = msg_send![accent, colorUsingColorSpace: space];
        if color.is_null() { return None }
        let (r, g, b): (f64, f64, f64) = (msg_send![color, redComponent], msg_send![color, greenComponent], msg_send![color, blueComponent]);
        let byte = |v: f64| (v.clamp(0., 1.) * 255.).round() as u32;
        Some(byte(r) << 16 | byte(g) << 8 | byte(b))
    });
    hex.is_some_and(theme::set_accent)
}
#[cfg(not(target_os = "macos"))]
pub fn sync_accent() -> bool { false }

impl Workspace {
    /// Change preferences and refresh what depends on them.
    pub(crate) fn set_pref(&mut self, f: impl FnOnce(&mut Preferences), cx: &mut Context<Self>) {
        let old = prefs();
        let mut p = old.clone();
        f(&mut p);
        if let Err(e) = preferences::save(&p) {
            self.show_toast_kind(ToastKind::Error, &format!("Could not save settings: {e}"), cx);
            return;
        }
        if p.appearance != old.appearance { apply_appearance(&p.appearance); }
        if sync_dark(cx) || p.hide_library_files != old.hide_library_files { cx.refresh_windows(); }
        cx.notify();
    }
}
