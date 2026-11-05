//! Settings: preferences (`.thesio/preferences.json` in the vault) and the
//! settings view — a category list in the sidebar, one page per category,
//! each page a column of grouped rows. Only what a
//! user really needs to change lives here.

use crate::{app::{View, Workspace}, theme::{self, Theme}, ui};
use crate::ui::{Icon, TitleBarExt, TipExt};
use crate::sidebar as sb;
use thesio_core::paths::{STATE_DIR, write_atomic};
use gpui::{prelude::FluentBuilder, *};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock, RwLock};

// ---- preferences

static PREFS: RwLock<Option<Preferences>> = RwLock::new(None);
/// Why `preferences.json` could not be read. Nothing writes to it meanwhile.
static UNREADABLE: RwLock<Option<String>> = RwLock::new(None);

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Retained for compatibility with existing preferences and older builds.
    pub english: bool,
    /// Retained for compatibility with existing preferences and older builds.
    pub quick_hotkey: bool,
    /// Light or dark look: "light", "dark", or empty to follow the system.
    pub appearance: String,
    /// Keep `library.yaml` and the papers out of the sidebar. They stay in the vault.
    pub hide_library_files: bool,
    /// Sent to Unpaywall, which requires one, to find open-access papers.
    pub unpaywall_email: String,
    /// Library column widths by column name, where they were resized.
    pub library_columns: std::collections::BTreeMap<String, f32>,
    /// Keep the Library's details panel closed.
    pub library_details_hidden: bool,
}

impl Default for Preferences {
    fn default() -> Self { Preferences { english: true, quick_hotkey: true, appearance: String::new(), hide_library_files: true, unpaywall_email: String::new(), library_columns: Default::default(), library_details_hidden: false } }
}


fn path() -> std::path::PathBuf { crate::store::Store::root_dir().join(STATE_DIR).join("preferences.json") }

pub fn load() {
    let read = match std::fs::read(path()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(e) => Err(e.to_string()),
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
    };
    let (preferences, problem) = match read {
        Ok(preferences) => (preferences, None),
        Err(problem) => (Preferences::default(), Some(problem)),
    };
    *UNREADABLE.write().unwrap() = problem;
    apply(preferences);
}

fn apply(p: Preferences) {
    *PREFS.write().unwrap() = Some(p);
}

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

/// The current preferences (loaded on first use).
pub fn prefs() -> Preferences {
    if PREFS.read().unwrap().is_none() { load(); }
    PREFS.read().unwrap().clone().unwrap_or_default()
}

fn save(p: &Preferences) -> anyhow::Result<()> {
    if let Some(problem) = &*UNREADABLE.read().unwrap() { anyhow::bail!("preferences.json could not be read and was left alone: {problem}"); }
    std::fs::create_dir_all(crate::store::Store::root_dir().join(STATE_DIR))?;
    write_atomic(&path(), &serde_json::to_vec_pretty(p)?)?;
    apply(p.clone());
    Ok(())
}

/// Redraw every window's document after the theme changed.
pub(crate) fn refresh_all(this: &mut Workspace, cx: &mut Context<Workspace>) {
    fn refresh(ws: &mut Workspace, cx: &mut Context<Workspace>) {
        ws.search.update(cx, |f, _| f.placeholder_color = Theme::text_disabled());
        cx.notify();
    }
    refresh(this, cx);
    // The other windows. Skip this workspace, and never read a window that is
    // mid-update (the one a click came from): GPUI panics on that, so ask
    // `is_active` first, which is None for a window on the update stack.
    let me = cx.entity_id();
    for w in cx.windows() {
        if let Some(h) = w.downcast::<Workspace>()
            && h.is_active(cx).is_some()
            && h.entity(cx).is_ok_and(|e| e.entity_id() != me)
        {
            let _ = h.update(cx, |ws, _, cx| refresh(ws, cx));
        }
    }
    crate::app::set_menus(cx);
    cx.refresh_windows();
}



// ---- the view

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page { #[default] Appearance, Library, About }

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::Library => "Documents",
            Page::About => "About",
        }
    }
    fn icon(self) -> Icon {
        match self {
            Page::Appearance => Icon::Palette,
            Page::Library => Icon::Storage,
            Page::About => Icon::Info,
        }
    }
}

/// The sidebar list, one evenly spaced run (five rows need no unlabelled
/// groups); `None` is Trash (its own view, shown with this sidebar).
const NAV: [Option<Page>; 4] = [Some(Page::Appearance), Some(Page::Library), None, Some(Page::About)];

// ---- controls (switch on the accent, 30pt buttons, grouped rows)

const COLUMN_W: f32 = 640.;

/// A titled group: the caption above, then the rows in one rounded card,
/// split by hairlines.
pub(crate) fn group(title: Option<&'static str>, rows: Vec<AnyElement>) -> Div {
    let n = rows.len();
    div().w_full().flex().flex_col().gap(px(8.))
        .children(title.map(caption))
        .child(div().w_full().flex().flex_col().rounded(px(8.)).border_1().border_color(Theme::line_border()).overflow_hidden()
            .children(rows.into_iter().enumerate().map(move |(i, r)| div().w_full().flex().flex_col().when(i + 1 < n, |d| d.border_b_1().border_color(ui::list_line())).child(r))))
}

pub(crate) fn caption(t: &'static str) -> Div {
    div().px(px(4.)).text_size(px(13.)).line_height(px(18.)).font_weight(FontWeight::MEDIUM).text_color(Theme::text_caption()).child(t)
}

/// One setting: title and an optional description on the left, the control on the right.
fn row(title: impl Into<SharedString>, desc: Option<SharedString>, control: impl IntoElement) -> Div {
    div().w_full().min_h(px(40.)).px(px(12.)).py(px(8.)).flex().items_center().justify_between().gap(px(24.))
        .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.))
            .child(div().text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(title.into()))
            .children(desc.map(|d| div().text_size(px(11.)).line_height(px(15.)).text_color(Theme::text_caption()).child(d))))
        .child(div().flex_none().flex().items_center().gap(px(8.)).child(control))
}



/// A segmented control: a grey groove and a white selected pill.
pub(crate) fn segmented(id: &'static str, labels: &[&'static str], selected: usize, on: impl Fn(&mut Workspace, usize, &mut Context<Workspace>) + Clone + 'static, cx: &mut Context<Workspace>) -> Div {
    div().flex_none().h(px(ui::BUTTON_H)).flex().gap(px(2.))
        .children(labels.iter().enumerate().map(|(i, label)| {
            let on = on.clone();
            div().id((id, i)).h_full().px(px(12.)).flex().items_center().justify_center().rounded(px(ui::BUTTON_RADIUS)).cursor_pointer()
                .text_size(px(13.)).line_height(px(18.))
                // In dark mode the picked segment is lighter than the groove (macOS: #636366).
                .when(i == selected, |d| d.bg(Theme::fill_hover_strong()).text_color(Theme::text()))
                .when(i != selected, |d| d.text_color(Theme::text_caption()))
                .child(*label)
                .on_click(cx.listener(move |w, _, _, cx| on(w, i, cx)))
        }))
}

/// An on/off switch: flat, the accent when on, the knob at that end.
fn switch(id: &'static str, on: bool) -> Stateful<Div> {
    div().id(id).flex_none().w(px(30.)).h(px(18.)).p(px(2.)).flex().items_center().rounded_full().cursor_pointer()
        .when(on, |d| d.justify_end().bg(Theme::primary()))
        .when(!on, |d| d.bg(Theme::fill_active()))
        .child(div().size(px(14.)).rounded_full().bg(gpui::white()))
}

pub(crate) fn button(id: impl Into<ElementId>, label: &'static str) -> Stateful<Div> {
    div().id(id).flex_none().h(px(ui::BUTTON_H)).px(px(12.)).flex().items_center().justify_center().rounded(px(ui::BUTTON_RADIUS)).cursor_pointer()
        .border_1().border_color(Theme::line_border()).active(|s| s.bg(Theme::fill_pressed()))
        .text_size(px(13.)).line_height(px(18.)).text_color(Theme::text()).child(label)
}

impl Workspace {
    /// Change preferences and refresh what depends on them.
    pub(crate) fn set_pref(&mut self, f: impl FnOnce(&mut Preferences), cx: &mut Context<Self>) {
        let old = prefs();
        let mut p = old.clone();
        f(&mut p);
        if let Err(e) = save(&p) {
            self.show_toast_kind(ui::ToastKind::Error, &format!("{}: {e}", "Could not save settings"), cx);
            return;
        }
        if p.appearance != old.appearance { apply_appearance(&p.appearance); }
        if sync_dark(cx) || p.hide_library_files != old.hide_library_files {
            refresh_all(self, cx);
        }
        cx.notify();
    }





    pub(crate) fn open_settings(&mut self, page: Page, cx: &mut Context<Self>) {
        self.settings_page = page;
        self.settings_scroll.set_offset(point(px(0.), px(0.)));
        if self.view != View::Settings { self.show_view(View::Settings, cx); }
        cx.notify();
    }

    pub(crate) fn render_settings_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let current = (self.view == View::Settings).then_some(self.settings_page);
        let in_trash = self.view == View::Trash;
        let nav_row = |page: Option<Page>, cx: &mut Context<Self>| {
            let on = if page.is_some() { page == current } else { in_trash };
            let (title, icon) = page.map_or(("Trash", Icon::Delete), |p| (p.title(), p.icon()));
            div().id(SharedString::from(format!("settings-nav-{page:?}"))).h(px(sb::ROW_H)).flex().items_center().pl(px(6.)).pr(px(4.)).rounded(px(sb::ROW_R))
                .text_size(px(13.)).line_height(px(20.)).cursor_pointer()
                .when(on, |d| d.bg(Theme::sidebar_selected()).text_color(Theme::text()))
                .when(!on, |d| d.text_color(Theme::sidebar_text()))
                .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().mr(px(9.)).child(ui::icon(icon, sb::ROW_ICON, Theme::sidebar_glyph())))
                .child(title)
                .on_click(cx.listener(move |w, _, _, cx| match page { Some(p) => w.open_settings(p, cx), None => w.show_view(View::Trash, cx) }))
        };
        div().w(px(self.sidebar_w(window))).h_full().flex_none().flex().flex_col().bg(Theme::sidebar_veil()).text_color(Theme::text()).font_family(theme::UI_FONT)
            .child(div().h(px(crate::app::HEADER_H)).flex_none().flex().items_center().pl(px(theme::TRAFFIC_LIGHTS_W)).pr(px(12.)).title_bar().child(self.header_navigation(cx)))
            // Match the document sidebar: Home sits 8 under the bar, followed by
            // a section header and rows on a 31pt pitch. Settings aligns with Home;
            // the 34pt box keeps the rows below.
            .child(div().pt(px(8.)).px(px(sb::SB_X)).flex().flex_col()
                .child(div().h(px(sb::SECTION_H)).pl(px(4.)).pt(px((sb::SECTION_TOP + sb::SECTION_H) / 2. - sb::ROW_H / 2.))
                    .child(div().px(px(4.)).text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::sidebar_section()).child("Settings")))
                .child(div().flex().flex_col().gap(px(sb::ROW_GAP)).children(NAV.iter().map(|p| nav_row(*p, cx)).collect::<Vec<_>>())))
            // Replace Settings with Back, returning to the previous page.
            .child(div().flex_1())
            .child(sb::footer_line())
            .child(sb::footer_slot(sb::footer_row("ft-back", Icon::Back, "Back").tip("Back", Some("Esc"))
                .on_click(cx.listener(|w, _, _, cx| w.close_settings(cx)))))
            .into_any_element()
    }

    /// Sidebar toggle and Home, in the document sidebar's toggle box (28pt,
    /// 4pt past the traffic lights, glyph 17), so the buttons don't move when
    /// switching between the document list and Settings.
    pub(crate) fn header_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.sidebar_open;
        let bar_button = |id: &'static str, glyph: Icon| {
            div().id(id).no_drag().flex_none().size(px(28.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                .child(ui::icon(glyph, 17., Theme::icon()))
        };
        div().flex().items_center().ml(px(4.)).gap(px(4.))
            .child(bar_button("settings-collapse", Icon::SidebarToggle)
                .tip(if open { "Hide sidebar" } else { "Show sidebar" }, Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                .on_click(cx.listener(|w, _, _, cx| { w.sidebar_open = !w.sidebar_open; cx.notify(); })))
            .child(bar_button("settings-home", Icon::Home).tip("Home", Some(crate::shortcuts::hint("workspace::GoHome", "⌘ + ⌥ + Home")))
                .on_click(cx.listener(|w, _, _, cx| w.show_view(View::Home, cx))))
            .into_any_element()
    }

    /// Back to where Settings was opened from.
    pub(crate) fn close_settings(&mut self, cx: &mut Context<Self>) {
        let back = if self.editor.is_some() && self.current.is_some() { View::Doc } else { View::Home };
        self.show_view(back, cx);
    }

    pub(crate) fn render_settings(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let page = self.settings_page;
        let body = match page {
            Page::Appearance => self.appearance_page(cx),
            Page::Library => self.library_page(window, cx),
            Page::About => self.about_page(),
        };
        div().flex_1().min_w_0().h_full().flex().flex_col().bg(Theme::bg_body()).text_color(Theme::text()).font_family(theme::UI_FONT)
            // The bar only drags the window; the page title heads the centred column.
            .child(div().h(px(crate::app::HEADER_H)).flex_none().flex().items_center().title_bar()
                .when(!self.sidebar_open, |d| d.pl(px(theme::TRAFFIC_LIGHTS_W)).child(self.header_navigation(cx))))
            .child(div().id("settings-content").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.settings_scroll)
                .child(div().w_full().flex().flex_col().items_center().px(px(32.)).pb(px(64.))
                    .child(div().w_full().max_w(px(COLUMN_W)).flex().flex_col().gap(px(28.))
                        .child(div().flex().items_center().gap(px(16.))
                            .child(div().flex_1().min_w_0().px(px(4.)).text_size(px(22.)).line_height(px(28.)).font_weight(FontWeight::SEMIBOLD).child(page.title())))
                        .child(body))))
            .into_any_element()
    }

    fn appearance_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let at = match prefs().appearance.as_str() { "light" => 1, "dark" => 2, _ => 0 };
        div().w_full().flex().flex_col().gap(px(28.))
            .child(group(None, vec![
                row("Appearance", None,
                    segmented("appearance", &["System", "Light", "Dark"], at,
                        |w, i, cx| w.set_pref(|p| p.appearance = ["", "light", "dark"][i].into(), cx), cx)).into_any_element(),
            ]))
            .into_any_element()
    }

    fn library_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let root = crate::store::Store::root_dir();
        let location = if cfg!(target_os = "windows") { root.display().to_string() } else {
            dirs::home_dir().and_then(|h| root.strip_prefix(h).ok().map(|r| format!("~/{}", r.display()))).unwrap_or_else(|| root.display().to_string())
        };
        let reveal = if cfg!(target_os = "macos") { "Show in Finder" } else if cfg!(target_os = "windows") {
            "Show in File Explorer"
        } else { "Show in File Manager" };
        let hide_library_files = prefs().hide_library_files;
        div().w_full().flex().flex_col().gap(px(8.))
            .child(group(None, vec![
                row("Vault folder", Some(location.into()),
                    button("reveal-library", reveal).on_click(|_, _, cx| cx.reveal_path(&crate::store::Store::root_dir()))).into_any_element(),
                row("Hide library files", Some("Keeps library.yaml and the sources folder out of the sidebar. They stay in the vault.".into()),
                    switch("hide-library-files", hide_library_files).on_click(cx.listener(move |w, _, _, cx| w.set_pref(|p| p.hide_library_files = !hide_library_files, cx)))).into_any_element(),
                row("Unpaywall email", Some("Finds open-access PDFs for sources with a DOI. Your email and each DOI are sent to Unpaywall.".into()),
                    ui::field_well(&self.citations.email, window, cx).w(px(220.))).into_any_element(),
                row("Trash", Some(format!("Deleted documents are kept for {} days.", crate::store::TRASH_DAYS).into()),
                    button("open-trash", "Open").on_click(cx.listener(|w, _, _, cx| w.show_view(View::Trash, cx)))).into_any_element(),
            ]))
            .into_any_element()
    }

    fn about_page(&self) -> AnyElement {
        div().w_full().flex().flex_col().gap(px(28.)).child(div().w_full().pt(px(24.)).flex().flex_col().items_center().gap(px(10.))
            .child(img(about_icon()).size(px(88.)))
            .child(div().text_size(px(20.)).line_height(px(26.)).font_weight(FontWeight::SEMIBOLD).child("Thesio"))
            .child(div().text_size(px(13.)).line_height(px(18.)).text_color(Theme::text_caption()).child(format!("{} {}", "Version", env!("CARGO_PKG_VERSION"))))
            .child(div().text_size(px(13.)).line_height(px(20.)).text_color(Theme::text_caption()).child("A local-first workspace for academic research and writing.")))
            .into_any_element()
    }
}

/// The app icon resampled once to 176px, its size at 88pt on a 2x screen.
/// GPUI samples images bilinearly without mipmaps, so the 1024px file shrunk
/// 5.8× on the GPU skips most of its pixels and the edges come out jagged.
/// On a 1x screen the 2:1 step is still an exact box filter.
fn about_icon() -> ImageSource {
    const PATH: &str = "branding/app-icon.png";
    static ICON: LazyLock<Option<Arc<RenderImage>>> = LazyLock::new(|| {
        let bytes = crate::assets::Assets.load(PATH).ok()??;
        let mut buf = image::imageops::resize(&image::load_from_memory(&bytes).ok()?.into_rgba8(), 176, 176, image::imageops::FilterType::Lanczos3);
        for px in buf.pixels_mut() {
            px.0.swap(0, 2);
        }
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
    });
    match &*ICON { Some(icon) => icon.clone().into(), None => PATH.into() }
}

pub fn label(text: impl AsRef<str>) -> String { text.as_ref().to_string() }

/// User titles are content, not translation keys.
pub fn display_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() { crate::store::UNTITLED.to_string() } else { title.to_string() }
}

