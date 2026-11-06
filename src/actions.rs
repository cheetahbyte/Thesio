//! Every workspace action in one table: its name, menu entry, default keys, and
//! what it does. Tooltips read their key from the keymap (`shortcuts::hint`), so
//! a new action is one row here. Editing keys live with `source` and `input`.

use crate::app::{View, Workspace};
use crate::settings::Page;
use gpui::*;

/// A run of menu items; separators fall between groups.
#[derive(Clone, Copy, PartialEq)]
enum Group { About, Preferences, Hide, Exit, Create, Close, Size, FullScreen }

/// Rows with `window` act on the window they were pressed in, not on whichever
/// window macOS considers active. Rows with `app` work with no window at all.
macro_rules! commands {
    ($($name:ident { $(menu: $group:ident $label:literal,)? keys: $keys:expr, $(window: |$w:tt, $window:tt, $cx:tt| $run:expr,)? $(app: |$app_cx:ident| $app_run:expr,)? })*) => {
        actions!(workspace, [$($name),*]);

        fn items(group: Group) -> Vec<MenuItem> {
            let mut items = vec![];
            $($(if group == Group::$group { items.push(MenuItem::action($label, $name)); })?)*
            items
        }

        fn bindings() -> Vec<KeyBinding> {
            let mut bindings = vec![];
            $(let keys: &[&str] = $keys;
            bindings.extend(keys.iter().map(|key| KeyBinding::new(key, $name, None)));)*
            bindings
        }

        fn on_app(cx: &mut App) {
            $($(cx.on_action(|_: &$name, $app_cx| $app_run);)?)*
        }

        /// The handlers on the window's root element.
        pub fn on_window<E: InteractiveElement>(root: E, cx: &mut Context<Workspace>) -> E {
            $($(let root = root.on_action(cx.listener(|$w, _: &$name, $window, $cx| $run));)?)*
            root
        }
    };
}

const fn on_mac(keys: &'static [&'static str]) -> &'static [&'static str] {
    if cfg!(target_os = "macos") { keys } else { &[] }
}

commands! {
    OpenAbout { menu: About "About Thesio", keys: &[], window: |w, window, cx| w.open_settings(Page::About, window, cx), }
    OpenSettings { menu: Preferences "Settings…", keys: &["cmd-,"], window: |w, window, cx| w.show_view(View::Settings, window, cx), }
    HideApp { menu: Hide "Hide Thesio", keys: on_mac(&["cmd-h"]), app: |cx| cx.hide(), }
    HideOthers { menu: Hide "Hide Others", keys: &[], app: |cx| cx.hide_other_apps(), }
    ShowAll { menu: Hide "Show All", keys: &[], app: |cx| cx.unhide_other_apps(), }
    // Key dispatch holds the window, and a window on the update stack cannot be asked to save.
    Quit { menu: Exit "Quit Thesio", keys: &["cmd-q"], app: |cx| cx.defer(crate::app::quit), }
    NewDoc { menu: Create "New Document", keys: &["cmd-n"], window: |w, window, cx| w.new_doc(window, cx), }
    NewFolder { menu: Create "New Folder", keys: &["cmd-shift-n"], window: |w, window, cx| w.new_folder(None, window, cx), }
    InsertCitation { menu: Create "Insert Citation…", keys: &["cmd-shift-c"], window: |w, window, cx| w.open_cite_picker(window, cx), }
    ImportDoc { menu: Create "Import as Document…", keys: &["cmd-shift-i"], window: |w, window, cx| w.open_import(window, cx), }
    CloseWindow {
        menu: Close "Close Window", keys: &["cmd-w"],
        window: |w, window, cx| if w.ready_to_close(window, cx) { window.remove_window(); },
        app: |cx| crate::app::close_active_window(cx),
    }
    Minimize { menu: Size "Minimize", keys: on_mac(&["cmd-m"]), window: |_, window, _| window.minimize_window(), }
    Zoom { menu: Size "Zoom", keys: &[], window: |_, window, _| window.zoom_window(), }
    // On Windows the system handles the other window keys.
    ToggleFullScreen {
        menu: FullScreen "Enter / Exit Full Screen", keys: if cfg!(target_os = "macos") { &["ctrl-cmd-f"] } else { &["f11"] },
        window: |_, window, _| window.toggle_fullscreen(),
    }
    GoHome { keys: &["cmd-alt-home"], window: |w, window, cx| w.show_view(View::Home, window, cx), }
    ToggleSidebar { keys: &["cmd-]", "cmd-\\"], window: |w, _, cx| w.toggle_sidebar(cx), }
    FocusSearch {
        keys: &["cmd-j"],
        window: |w, window, cx| {
            if w.search_open() { w.close_overlay(window, cx); } else { w.open_search(window, cx); }
            cx.stop_propagation();
        },
    }
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys(bindings());
    on_app(cx);
}

fn with_separators(groups: &[Group]) -> Vec<MenuItem> {
    let mut menu = vec![];
    for (i, group) in groups.iter().enumerate() {
        if i > 0 { menu.push(MenuItem::separator()); }
        menu.extend(items(*group));
    }
    menu
}

pub fn set_menus(cx: &mut App) {
    use crate::source as k;
    let mut app = with_separators(&[Group::About, Group::Preferences]);
    app.extend([MenuItem::separator(), MenuItem::os_submenu("Services", SystemMenuType::Services), MenuItem::separator()]);
    app.extend(with_separators(&[Group::Hide, Group::Exit]));
    cx.set_menus(vec![
        Menu { name: "Thesio".into(), items: app },
        Menu { name: "File".into(), items: with_separators(&[Group::Create, Group::Close]) },
        // The standard Edit menu: its items act on the focused editor, and
        // macOS adds Dictation and Emoji & Symbols to it.
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Undo", k::Undo, OsAction::Undo),
                MenuItem::os_action("Redo", k::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", k::Cut, OsAction::Cut),
                MenuItem::os_action("Copy", k::Copy, OsAction::Copy),
                MenuItem::os_action("Paste", k::Paste, OsAction::Paste),
                MenuItem::os_action("Select All", k::SelectAll, OsAction::SelectAll),
            ],
        },
        Menu { name: "Window".into(), items: with_separators(&[Group::Size, Group::FullScreen]) },
    ]);
}
