//! The Settings view: a category list in the sidebar, one page per category,
//! each page a column of grouped rows. Only what a user really needs to change
//! lives here.

mod widgets;

use crate::{app::{View, Workspace}, theme::{self, Theme}, ui};
use crate::input::{TextField, TextFieldEvent};
use crate::preferences::prefs;
use crate::ui::{Icon, TitleBarExt, TipExt, secondary_button, segmented};
use crate::sidebar as sb;
use gpui::{prelude::FluentBuilder, *};
use std::sync::{Arc, LazyLock};
use widgets::{group, row, switch};

pub struct Settings {
    pub(crate) page: Page,
    pub(crate) scroll: ScrollHandle,
    email: Entity<TextField>,
    _subs: Vec<Subscription>,
}

impl Settings {
    pub fn new(cx: &mut Context<Workspace>) -> Self {
        let email = cx.new(|cx| {
            let mut f = TextField::new("name@university.edu", cx);
            f.placeholder_color = Theme::text_disabled;
            f.set_text(&prefs().unpaywall_email, cx);
            f
        });
        let subs = vec![cx.subscribe(&email, |w, field, ev: &TextFieldEvent, cx| if matches!(ev, TextFieldEvent::Changed) {
            let email = field.read(cx).text().trim().to_string();
            w.set_pref(|p| p.unpaywall_email = email, cx);
        })];
        Self { page: Page::default(), scroll: ScrollHandle::new(), email, _subs: subs }
    }
}

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

const COLUMN_W: f32 = 640.;

/// The sidebar list. `None` is Trash, its own view, shown with this sidebar.
const NAV: [Option<Page>; 4] = [Some(Page::Appearance), Some(Page::Library), None, Some(Page::About)];

impl Workspace {
    pub(crate) fn open_settings(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.page = page;
        self.settings.scroll.set_offset(point(px(0.), px(0.)));
        if self.view != View::Settings { self.show_view(View::Settings, window, cx); }
        cx.notify();
    }

    pub(crate) fn render_settings_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let current = (self.view == View::Settings).then_some(self.settings.page);
        let in_trash = self.view == View::Trash;
        let nav_row = |page: Option<Page>, cx: &mut Context<Self>| {
            let on = if page.is_some() { page == current } else { in_trash };
            let (title, icon) = page.map_or(("Trash", Icon::Delete), |p| (p.title(), p.icon()));
            sb::nav_row(SharedString::from(format!("settings-nav-{page:?}")), icon, title, on)
                .on_click(cx.listener(move |w, _, window, cx| match page { Some(p) => w.open_settings(p, window, cx), None => w.show_view(View::Trash, window, cx) }))
        };
        div().w(px(self.sidebar_w(window))).h_full().flex_none().flex().flex_col().bg(Theme::sidebar_veil()).text_color(Theme::text()).font_family(theme::UI_FONT)
            .child(div().h(px(crate::ui::HEADER_H)).flex_none().flex().items_center().pl(px(theme::TRAFFIC_LIGHTS_W)).pr(px(12.)).title_bar().child(self.header_navigation(cx)))
            .child(div().pt(px(8.)).px(px(sb::SB_X)).flex().flex_col()
                .child(div().h(px(sb::SECTION_H)).pl(px(4.)).pt(px((sb::SECTION_TOP + sb::SECTION_H) / 2. - sb::ROW_H / 2.))
                    .child(div().px(px(4.)).text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::sidebar_section()).child("Settings")))
                .child(div().flex().flex_col().gap(px(sb::ROW_GAP)).children(NAV.iter().map(|p| nav_row(*p, cx)).collect::<Vec<_>>())))
            .child(div().flex_1())
            .child(sb::footer_line())
            .child(sb::footer_slot(sb::nav_row("ft-back", Icon::Back, "Back", false).tip("Back", Some("Esc"))
                .on_click(cx.listener(|w, _, window, cx| w.close_settings(window, cx)))))
            .into_any_element()
    }

    /// Back to where Settings was opened from.
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = if self.document.is_some() { View::Doc } else { View::Home };
        self.show_view(back, window, cx);
    }

    pub(crate) fn render_settings(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let page = self.settings.page;
        let body = match page {
            Page::Appearance => self.appearance_page(cx),
            Page::Library => self.library_page(window, cx),
            Page::About => self.about_page(),
        };
        div().flex_1().min_w_0().h_full().flex().flex_col().bg(Theme::bg_body()).text_color(Theme::text()).font_family(theme::UI_FONT)
            // The bar only drags the window; the page title heads the centred column.
            .child(div().h(px(crate::ui::HEADER_H)).flex_none().flex().items_center().title_bar()
                .when(!self.sidebar.open, |d| d.pl(px(theme::TRAFFIC_LIGHTS_W)).child(self.header_navigation(cx))))
            .child(div().id("settings-content").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.settings.scroll)
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
            .child(group(vec![
                row("Appearance", None,
                    segmented("appearance", &["System", "Light", "Dark"], at,
                        cx.listener(|w, i: &usize, _, cx| w.set_pref(|p| p.appearance = ["", "light", "dark"][*i].into(), cx)))).into_any_element(),
            ]))
            .into_any_element()
    }

    fn library_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let root = crate::store::Store::root_dir();
        let location = if cfg!(target_os = "windows") { root.display().to_string() } else {
            dirs::home_dir().and_then(|h| root.strip_prefix(h).ok().map(|r| format!("~/{}", r.display()))).unwrap_or_else(|| root.display().to_string())
        };
        let hide_library_files = prefs().hide_library_files;
        div().w_full().flex().flex_col().gap(px(8.))
            .child(group(vec![
                row("Vault folder", Some(location.into()),
                    secondary_button("reveal-library", crate::system::REVEAL_LABEL).on_click(|_, _, cx| crate::system::reveal(&crate::store::Store::root_dir(), cx))).into_any_element(),
                row("Hide library files", Some("Keeps library.yaml and the sources folder out of the sidebar. They stay in the vault.".into()),
                    switch("hide-library-files", hide_library_files).on_click(cx.listener(move |w, _, _, cx| w.set_pref(|p| p.hide_library_files = !hide_library_files, cx)))).into_any_element(),
                row("Unpaywall email", Some("Finds open-access PDFs for sources with a DOI. Your email and each DOI are sent to Unpaywall.".into()),
                    ui::field_well(&self.settings.email, window, cx).w(px(220.))).into_any_element(),
                row("Trash", Some(format!("Deleted documents are kept for {} days.", crate::store::TRASH_DAYS).into()),
                    secondary_button("open-trash", "Open").on_click(cx.listener(|w, _, window, cx| w.show_view(View::Trash, window, cx)))).into_any_element(),
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
