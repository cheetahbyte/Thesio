//! The buttons and page header the Workspace's views share.

use crate::actions::{FocusSearch, GoHome, ToggleSidebar};
use crate::app::{View, Workspace};
use crate::theme::{self, Theme};
use crate::ui::{HEADER_H, Icon, TipExt, TipSide, TitleBarExt, header_right_inset, icon, toolbar_icon};
use gpui::{prelude::FluentBuilder, *};

/// Where a page's title starts when the sidebar is open.
const PAGE_INSET: f32 = 26.;
/// What sits before the title with the sidebar closed: Show sidebar and its
/// margins of 4 and 12, and on Windows Home beside it.
const NAV_W: f32 = if cfg!(target_os = "windows") { 76. } else { 44. };

/// Where a page's title starts.
pub(crate) fn page_title_x(sidebar_open: bool) -> f32 {
    if sidebar_open { PAGE_INSET } else { theme::TRAFFIC_LIGHTS_W + NAV_W }
}

/// The page header, as tall as the document's so the header pills sit in
/// the same place: a 17pt semibold view title, and the open-sidebar
/// button when the sidebar is hidden.
pub(crate) fn page_header(ws: &Workspace, title: &str, count: Option<usize>, cx: &mut Context<Workspace>) -> Stateful<Div> {
    div()
        .id("page-header")
        .h(px(HEADER_H))
        .flex_none()
        .flex()
        .items_center()
        // Clear of the traffic lights, like the document header.
        .pl(if ws.sidebar.open { px(PAGE_INSET) } else { px(theme::TRAFFIC_LIGHTS_W) })
        .pr(px(header_right_inset()))
        .title_bar()
        .when(!ws.sidebar.open && cfg!(target_os = "windows"), |d| d.child(div().mr(px(12.)).child(ws.header_navigation(cx))))
        .when(!ws.sidebar.open && !cfg!(target_os = "windows"), |d| {
            d.child(ws.sidebar_toggle("open-sidebar", Theme::icon(), cx).ml(px(4.)).mr(px(12.)))
        })
        .child(div().text_size(px(15.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
        .children(count.map(|n| div().ml(px(8.)).text_size(px(13.)).text_color(Theme::text_caption()).child(n.to_string())))
        .child(div().flex_1())
}

/// A 28pt button in the window's title bar, beside the traffic lights.
fn bar_button(id: &'static str, glyph: Icon, size: f32, color: Hsla) -> Stateful<Div> {
    div().id(id).no_drag().flex_none().size(px(28.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer().child(icon(glyph, size, color))
}

impl Workspace {
    pub(crate) fn sidebar_toggle(&self, id: &'static str, color: Hsla, cx: &mut Context<Self>) -> Stateful<Div> {
        bar_button(id, Icon::SidebarToggle, 17., color)
            .tip_action(if self.sidebar.open { "Hide sidebar" } else { "Show sidebar" }, &ToggleSidebar, cx)
            .on_click(cx.listener(|w, _, _, cx| w.toggle_sidebar(cx)))
    }

    pub(crate) fn home_button(&self, id: &'static str, size: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        bar_button(id, Icon::Home, size, Theme::icon()).on_click(cx.listener(|w, _, window, cx| w.show_view(View::Home, window, cx)))
    }

    /// Sidebar toggle and Home, in the document sidebar's toggle box (28pt,
    /// 4pt past the traffic lights, glyph 17), so the buttons don't move when
    /// switching between the document list and Settings.
    pub(crate) fn header_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        div().flex().items_center().ml(px(4.)).gap(px(4.))
            .child(self.sidebar_toggle("settings-collapse", Theme::icon(), cx))
            .child(self.home_button("settings-home", 17., cx).tip_action("Home", &GoHome, cx))
            .into_any_element()
    }

    /// Opens search; the tooltip names the key.
    pub(crate) fn search_button(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let tip = crate::shortcuts::hint(&FocusSearch, cx).map_or("Search".to_string(), |keys| format!("Search ({keys})"));
        toolbar_icon(id, Icon::Search).tip_label(tip, TipSide::Below).on_click(cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(FocusSearch), cx)))
    }
}
