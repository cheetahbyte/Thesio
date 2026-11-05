#![recursion_limit = "512"]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod appearance;
mod shared_store;
mod window_size;
mod assets;
mod citations;
mod clipboard;
mod confirm;
mod devtools;
mod docmenu;
mod document;
mod history;
mod home;
mod icon;
mod input;
#[cfg(target_os = "macos")]
mod native_header;
#[cfg(not(target_os = "macos"))]
#[path = "native_header_fallback.rs"]
mod native_header;
#[cfg(target_os = "macos")]
mod native_pdf;
#[cfg(not(target_os = "macos"))]
#[path = "native_pdf_fallback.rs"]
mod native_pdf;
mod overlay;
mod papers;
mod preferences;
mod preview;
mod search;
mod session;
mod sidebar;
mod source;
mod settings;
mod shortcuts;
mod startup;
mod theme;
mod trash;
mod tree;
mod ui;

use thesio_core::{library, store, time};

use gpui::*;

#[cfg(target_os = "macos")]
fn set_app_icon() {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;
    let png = include_bytes!("../assets/branding/app-icon.png");
    unsafe {
        // A bundle has its icon from Info.plist, and decoding this one takes 60 ms.
        let bundle: *mut Object = msg_send![class!(NSBundle), mainBundle];
        let identifier: *mut Object = msg_send![bundle, bundleIdentifier];
        if !identifier.is_null() { return; }
        let data: *mut Object = msg_send![class!(NSData), dataWithBytes:png.as_ptr() length:png.len()];
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData:data];
        if !image.is_null() {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, setApplicationIconImage:image];
            let _: () = msg_send![image, release];
        }
    }
}

/// The main window, centred at the remembered size.
fn open_main_window(scripted: bool, document: Option<String>, cx: &mut App) {
    // Scripted dev runs must not steal keyboard focus from whatever the
    // user is typing into.
    let bounds = Bounds::centered(None, window_size::main_window_size(cx), cx);
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: WindowBackgroundAppearance::Blurred,
            titlebar: Some(TitlebarOptions {
                title: Some("Thesio".into()),
                appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
                traffic_light_position: Some(point(px(20.), px(app::HEADER_H / 2. - 7.))),
            }),
            window_min_size: Some(size(px(480.), px(480.))),
            focus: !scripted,
            ..Default::default()
        },
        |window, cx| {
            if document.is_none() { devtools::start(window, cx); }
            startup::mark("window-open");
            cx.new(|cx| app::Workspace::new(document, window, cx))
        },
    );
    if let Err(e) = result { eprintln!("Could not open document window: {e}"); }
}

fn main() {
    startup::mark("main");
    session::prepare();
    let scripted = std::env::var_os("THESIO_SCRIPT").is_some();
    // No single-instance guard: copies may share a library (builds tested
    // side by side). `Store` rereads an index another copy rewrote before
    // writing its own.
    let app = Application::new().with_assets(assets::Assets);
    // Clicking the Dock icon brings the main window back after ⌘W.
    app.on_reopen(|cx| if cx.windows().is_empty() { open_main_window(false, None, cx); });
    app.run(move |cx: &mut App| {
        startup::mark("app-ready");
        #[cfg(target_os = "macos")]
        set_app_icon();
        input::bind_keys(cx);
        source::bind_keys(cx);
        app::bind_keys(cx);
        preferences::load();
        appearance::apply_appearance(&preferences::prefs().appearance);
        appearance::sync_dark(cx);
        theme::sync_accent();
        shortcuts::init(cx);
        cx.set_global(shared_store::SharedStore::open().expect("open document library"));
        app::set_menus(cx);
        open_main_window(scripted, None, cx);
        cx.on_window_closed(move |cx| {
            if (scripted || !cfg!(target_os = "macos")) && cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        if !scripted {
            cx.activate(true);
        }
    });
}
