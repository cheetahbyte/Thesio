//! Small shared UI pieces: icons, tooltips, popover shells.

use crate::app::Workspace;
use crate::theme::{self, Theme, craft};
use crate::time::civil;
use gpui::{prelude::FluentBuilder, *};
use std::cell::RefCell;
use std::rc::Rc;

pub use crate::icon::Icon;

pub fn icon(name: Icon, size: f32, color: Hsla) -> Svg {
    svg().path(SharedString::from(name.path(size))).size(px(size)).flex_none().text_color(color)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToastKind {
    Success,
    Error,
}

/// Toast: a white rounded card `top` below the window top, centred, with a filled
/// status circle (green check / red x / accent i). It lasts 3s and fades over
/// its last .3s; None once it is over.
pub fn toast_el(kind: ToastKind, msg: &SharedString, t0: std::time::Instant, top: f32, window: &mut Window) -> Option<AnyElement> {
    let e = t0.elapsed().as_secs_f32();
    if e >= 3. {
        return None;
    }
    let op = if e > 2.7 {
        window.request_animation_frame();
        (3. - e) / 0.3
    } else {
        1.
    };
    let (fill, mark, mark_size) = match kind {
        ToastKind::Success => (theme::c(craft::old::G500), Icon::Check, 10.),
        ToastKind::Error => (Theme::danger(), Icon::Close, 10.),
    };
    let vw = window.viewport_size().width;
    let card = div()
        .flex()
        .items_center()
        .px(px(14.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(crate::theme::raised(Theme::bg_body()))
        .border_1()
        .border_color(Theme::line_border())
        .shadow(theme::shadow_panel())
        .font_family(theme::UI_FONT)
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(Theme::text())
        .child(
            div()
                .relative()
                .size(px(16.))
                .mr(px(8.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(Icon::Circle, 16., fill).absolute().top_0().left_0())
                .child(icon(mark, mark_size, gpui::white())),
        )
        .child(ui_text(msg.clone()));
    Some(
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Window)
                .position(point(px(0.), px(top)))
                .child(div().w(vw).flex().justify_center().opacity(op).child(card)),
        )
        .with_priority(layer::TOAST)
        .into_any_element(),
    )
}

/// Popover shell shared by the slash menu, block menu and toolbar dropdowns:
/// the light panel (macOS 26 menu material, near-white), hairline,
/// radius 12, soft drop.
pub fn popover() -> Div {
    div()
        .bg(crate::theme::raised(Theme::popover_bg()))
        .border_1()
        .border_color(Theme::line_border())
        .rounded(px(MENU_RADIUS))
        .shadow(theme::shadow_menu())
        .font_family(theme::UI_FONT)
        .text_color(Theme::text())
}

/// Column headers of the page lists: tertiary text (measured #88898B on
/// the name / last-viewed / created columns).
pub fn list_header_text() -> Hsla {
    theme::c(0x88898b)
}

/// Hairline under a list header and between rows (black 4.5%).
pub fn list_line() -> Hsla {
    theme::ca(craft::N1000, 0.045)
}


/// Stacking of what GPUI draws above the page, lowest first.
pub mod layer {
    pub const COMPLETION: usize = 1;
    pub const MENU: usize = 2;
    pub const DIALOG: usize = 3;
    /// A full-window layer that holds the pointer during a drag.
    pub const DRAG: usize = 4;
    pub const TOAST: usize = 20;
}

/// The window dimmed under a modal dialog: a light black veil.
pub fn scrim() -> Hsla {
    theme::rgbaf(0, 0, 0, if theme::dark() { 0.45 } else { 0.25 })
}

/// The modal dialog: white, radius 12, deep soft drop.
pub fn dialog(id: &'static str) -> Stateful<Div> {
    div().id(id).occlude().rounded(px(12.)).bg(crate::theme::raised(Theme::bg_body())).border_1().border_color(Theme::line_border()).shadow(theme::shadow_dialog()).font_family(theme::UI_FONT).text_color(Theme::text())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

#[derive(Clone, Copy, PartialEq)]
pub enum ModalAt { Center, Top }

/// `panel` over the dimmed window. With `dismiss`, a click beside it closes it.
pub fn modal(id: &'static str, at: ModalAt, dismiss: bool, panel: impl IntoElement, cx: &Context<Workspace>) -> AnyElement {
    deferred(div().id(id).absolute().inset_0().bg(scrim()).flex().justify_center().occlude()
        .when(at == ModalAt::Center, |d| d.items_center())
        .when(at == ModalAt::Top, |d| d.items_start().pt(px(96.)))
        .on_mouse_down(MouseButton::Left, cx.listener(move |w, _, window, cx| {
            cx.stop_propagation();
            if dismiss { w.close_overlay(window, cx); }
        }))
        .child(panel)).with_priority(layer::DIALOG).into_any_element()
}

/// `menu` floating at `position`, kept inside the window.
pub fn anchored_menu(corner: Corner, position: Point<Pixels>, menu: impl IntoElement) -> AnyElement {
    deferred(anchored().anchor(corner).position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(layer::MENU).into_any_element()
}

/// A menu panel `width` wide that closes on a click outside it.
pub fn menu_panel(id: &'static str, width: f32, cx: &Context<Workspace>) -> Stateful<Div> {
    popover().id(id).occlude().w(px(width)).py(px(MENU_PAD_Y)).on_mouse_down_out(cx.listener(|w, _, window, cx| w.close_overlay(window, cx)))
}

/// A menu row's click: the menu closes, then `action` runs.
pub fn menu_action(cx: &Context<Workspace>, action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |w, _, window, cx| {
        w.close_overlay(window, cx);
        action(w, window, cx);
    })
}

/// The page header, as tall as the document's so the header pills sit in
/// the same place: a 17pt semibold view title, and the open-sidebar
/// button when the sidebar is hidden.
pub(crate) fn page_header(ws: &Workspace, title: &str, count: Option<usize>, cx: &mut Context<Workspace>) -> Stateful<Div> {
    div()
        .id("page-header")
        .h(px(crate::app::HEADER_H))
        .flex_none()
        .flex()
        .items_center()
        // Clear of the traffic lights, like the document header.
        .pl(if ws.sidebar_open { px(26.) } else { px(theme::TRAFFIC_LIGHTS_W) })
        .pr(px(header_right_inset()))
        .title_bar()
        .when(!ws.sidebar_open && cfg!(target_os = "windows"), |d| d.child(div().mr(px(12.)).child(ws.header_navigation(cx))))
        .when(!ws.sidebar_open && !cfg!(target_os = "windows"), |d| {
            d.child(ws.sidebar_toggle("open-sidebar", Theme::icon(), cx).ml(px(4.)).mr(px(12.)))
        })
        .child(div().text_size(px(15.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
        .children(count.map(|n| div().ml(px(8.)).text_size(px(13.)).text_color(Theme::text_caption()).child(n.to_string())))
        .child(div().flex_1())
}

/// Centred empty state: a large glyph, a title, and a caption.
pub fn empty_state(glyph: Icon, title: &'static str, text: &'static str) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .pb(px(64.))
        .child(icon(glyph, 44., Theme::text_placeholder()))
        .child(div().mt(px(14.)).text_size(px(15.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title))
        .child(div().mt(px(2.)).text_size(px(13.)).line_height(px(18.)).text_color(Theme::text_caption()).child(text))
}

/// The primary button: a flat accent fill, a white 13pt label.
pub fn primary_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    colored_button(id, label, false)
}

pub fn danger_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    colored_button(id, label, true)
}

fn colored_button(id: &'static str, label: &'static str, danger: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(BUTTON_H))
        .px(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .bg(if danger { Theme::danger() } else { Theme::primary() })
        .active(move |s| s.bg(if danger { theme::c(craft::R700) } else { Theme::primary_strong() }))
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(gpui::white())
        .child(label)
}

/// A primary button led by a glyph.
pub fn primary_icon_button(id: &'static str, glyph: Icon, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(BUTTON_H))
        .pl(px(9.))
        .pr(px(12.))
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .bg(Theme::primary())
        .active(|s| s.bg(Theme::primary_strong()))
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(gpui::white())
        .child(icon(glyph, 14., gpui::white()))
        .child(label)
}

/// A segmented control: a grey groove and a white selected pill.
pub fn segmented(id: &'static str, labels: &[&'static str], selected: usize, on: impl Fn(&mut Workspace, usize, &mut Context<Workspace>) + Clone + 'static, cx: &mut Context<Workspace>) -> Div {
    div().flex_none().h(px(BUTTON_H)).flex().gap(px(2.))
        .children(labels.iter().enumerate().map(|(i, label)| {
            let on = on.clone();
            div().id((id, i)).h_full().px(px(12.)).flex().items_center().justify_center().rounded(px(BUTTON_RADIUS)).cursor_pointer()
                .text_size(px(13.)).line_height(px(18.))
                // In dark mode the picked segment is lighter than the groove (macOS: #636366).
                .when(i == selected, |d| d.bg(Theme::fill_hover_strong()).text_color(Theme::text()))
                .when(i != selected, |d| d.text_color(Theme::text_caption()))
                .child(*label)
                .on_click(cx.listener(move |w, _, _, cx| on(w, i, cx)))
        }))
}

/// A glyph-only toolbar button.
pub fn toolbar_icon(id: &'static str, glyph: Icon) -> Stateful<Div> {
    div().id(id).flex_none().size(px(TOOLBAR_ITEM)).flex().items_center().justify_center().rounded(px(BUTTON_RADIUS)).cursor_pointer()
        .active(|s| s.bg(Theme::fill_pressed())).child(icon(glyph, 15., Theme::text_code()))
}

/// A count with thousands set apart: 18,420.
pub fn format_count(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) { out.push(','); }
        out.push(digit);
    }
    out
}

/// Toolbar buttons that belong together.
pub fn toolbar_group() -> Div {
    div().flex_none().flex().items_center().gap(px(2.))
}

/// A text field: a hairline outline that takes the accent while focused.
pub fn field_well(field: &Entity<crate::input::TextField>, window: &Window, cx: &App) -> Div {
    well(field, window, cx).child(field.clone())
}

/// A search field: a magnifier, then `field`.
pub fn search_well(field: &Entity<crate::input::TextField>, window: &Window, cx: &App) -> Div {
    well(field, window, cx).gap(px(6.)).child(icon(Icon::Search, 13., Theme::text_caption())).child(div().flex_1().min_w_0().child(field.clone()))
}

fn well(field: &Entity<crate::input::TextField>, window: &Window, cx: &App) -> Div {
    let focused = field.read(cx).is_focused(window);
    div().h(px(BUTTON_H)).px(px(9.)).flex().items_center().rounded(px(BUTTON_RADIUS)).text_size(px(13.))
        .border_1().border_color(if focused { Theme::primary() } else { Theme::line_border() })
}

#[cfg(test)]
mod button_tests {
    #[test]
    fn danger_button_constructs_without_duplicate_interaction_styles() {
        let _ = super::danger_button("test-danger", "Delete");
    }
}

/// The secondary button: a hairline outline, flat.
pub fn secondary_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(BUTTON_H))
        .px(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .border_1()
        .border_color(Theme::line_border())
        .active(|s| s.bg(Theme::fill_pressed()))
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(Theme::text())
.child(label)
}

/// A borderless dialog action, beside the buttons but quieter than them.
pub fn plain_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div().id(id).flex_none().h(px(BUTTON_H)).px(px(4.)).flex().items_center().rounded(px(BUTTON_RADIUS)).cursor_pointer()
        .text_size(px(13.)).line_height(px(20.)).text_color(Theme::primary()).child(label)
}

/// The panel close button: a grey disc with a white cross (measured
/// #B6B7B8 on #F7F7F7, black at about 26%).
pub fn close_button(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .bg(theme::ca(craft::N1000, 0.26))
        .child(icon(Icon::Close, 16., gpui::white()))
}

pub const BUTTON_H: f32 = 26.;
pub const BUTTON_RADIUS: f32 = 6.;
const TOOLBAR_ITEM: f32 = 28.;

/// Menus and popovers round at 8; their rows at 5, 4pt in from the edge.
pub const MENU_RADIUS: f32 = 8.;
pub const ROW_RADIUS: f32 = 5.;

/// A menu row: 26 tall, 13pt label, 16pt glyph, a quiet hover.
pub const MENU_ROW_H: f32 = 26.;
pub const MENU_PAD_Y: f32 = 5.;

pub fn menu_row(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(MENU_ROW_H))
        .mx(px(4.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(Theme::text())
        .hover(|s| s.bg(Theme::menu_highlight()))
}

/// A [`crate::ui::menu_row`] label.
pub fn menu_label(label: impl Into<SharedString>) -> Div {
    div().whitespace_nowrap().child(label.into())
}

/// Glyph of a [`menu_row`].
pub fn menu_glyph(name: Icon, size: f32) -> Svg {
    icon(name, size, Theme::text_code())
}

/// Tooltip bubble: a small flat tip, 11pt on the menu surface, no arrow.
pub struct Tooltip {
    /// One or more lines.
    label: SharedString,
    /// Window bounds of the trigger, when it was registered through [`TipExt::tip`].
    anchor: Option<Bounds<Pixels>>,
    /// Where GPUI anchors the tooltip root (mouse + 1 at show time).
    origin: Point<Pixels>,
    side: TipSide,
}

/// Preferred side: chrome shows tooltips below, document controls above,
/// menu items to the right. Flipped when there is no room.
#[derive(Clone, Copy, PartialEq)]
pub enum TipSide {
    Below,
    Above,
    Right,
}

const TIP_GAP: f32 = 6.;
const TIP_PAD: f32 = 6.;
const TIP_LINE: f32 = 14.;
const TIP_TEXT: f32 = 11.;

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let lines: Vec<SharedString> = self.label.split('\n').map(|l| SharedString::from(l.to_string())).collect();
        let bubble = div()
            .px(px(6.))
            .py(px(3.))
            .rounded(px(5.))
            .bg(theme::raised(Theme::popover_bg()))
            .border_1()
            .border_color(Theme::line_border())
            .text_color(Theme::text())
            .text_size(px(TIP_TEXT))
            .line_height(px(TIP_LINE))
            .font_family(theme::UI_FONT)
            .whitespace_nowrap()
            .flex()
            .flex_col()
            .children(lines.iter().map(|l| ui_text(l.clone())));
        let Some(anchor) = self.anchor else {
            return div().pl(px(8.)).pt(px(10.)).child(bubble).into_any_element();
        };
        // Beside the trigger on the preferred side, unless it doesn't fit, and kept inside the window.
        let mut style = window.text_style();
        style.font_family = theme::UI_FONT.into();
        let text_w = lines
            .iter()
            .map(|l| {
                let runs = ui_text(l.clone()).runs(&style);
                window.text_system().shape_line(l.clone(), px(TIP_TEXT), &runs, None).width
            })
            .fold(px(0.), |a, b| a.max(b));
        let w = text_w + px(12.);
        let h = px(TIP_PAD + TIP_LINE * lines.len() as f32);
        let vp = window.viewport_size();
        let gap = px(TIP_GAP);
        let (x, y) = if self.side == TipSide::Right {
            let x = if anchor.right() + gap + w <= vp.width || anchor.left() - gap - w < px(0.) { anchor.right() + gap } else { anchor.left() - gap - w };
            (x, (anchor.center().y - h / 2.).min(vp.height - h).max(px(0.)))
        } else {
            let fits_below = anchor.bottom() + gap + h <= vp.height;
            let fits_above = anchor.top() - gap - h >= px(0.);
            let below = if self.side == TipSide::Above { !fits_above } else { fits_below };
            ((anchor.center().x - w / 2.).min(vp.width - w).max(px(0.)), if below { anchor.bottom() + gap } else { anchor.top() - gap - h })
        };
        let o = self.origin;
        div().size(px(1.)).child(div().absolute().left(x - o.x).top(y - o.y).child(bubble)).into_any_element()
    }
}

/// Tooltip anchored on the element rather than the mouse.
pub trait TipExt: Sized {
    /// Window chrome: below, with the shortcut in full-width parentheses.
    fn tip(self, text: &str, keys: Option<&str>) -> Self {
        let label = match keys {
            Some(k) => format!("{text}（{k}）"),
            None => text.to_string(),
        };
        self.tip_label(label, TipSide::Below)
    }
    /// `tip` with the key the keymap holds for `action`.
    fn tip_action(self, text: &str, action: &dyn Action, cx: &App) -> Self {
        self.tip(text, crate::shortcuts::hint(action, cx).as_deref())
    }
    fn tip_label(self, label: String, side: TipSide) -> Self;
}

/// Window chrome above the page's top line works like a native title bar: a
/// press drags the window, a double press zooms it (per the system setting).
/// Controls on it opt out with `no_drag`, so double-clicking them doesn't zoom.
pub trait TitleBarExt: InteractiveElement + Sized {
    fn title_bar(self) -> Self {
        // Keep the workspace's focus handler from preventing the native caption action.
        if cfg!(target_os = "windows") { return self.occlude().window_control_area(WindowControlArea::Drag); }
        self.on_mouse_down(MouseButton::Left, |ev, window, _| match ev.click_count {
            1 => window.start_window_move(),
            2 => window.titlebar_double_click(),
            _ => {}
        })
    }
    fn no_drag(self) -> Self {
        let element = if cfg!(target_os = "windows") { self.occlude() } else { self };
        element.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
}

impl<E: InteractiveElement> TitleBarExt for E {}

pub(crate) const WINDOW_CONTROLS_W: f32 = if cfg!(target_os = "windows") { 163. } else { 0. };
pub(crate) fn header_right_inset() -> f32 {
    if cfg!(target_os = "windows") { WINDOW_CONTROLS_W } else { (crate::app::HEADER_H - crate::app::HEADER_ACTIONS_H) / 2. }
}

pub(crate) fn window_controls(window: &Window) -> Option<Div> {
    if !cfg!(target_os = "windows") { return None; }
    let button = |id: &'static str, area: WindowControlArea, label: &'static str| {
        div().id(id).size(px(crate::app::HEADER_ACTIONS_H)).mx(px(7.)).rounded(px(8.)).flex_none().flex().items_center().justify_center().cursor_pointer()
            .occlude().window_control_area(area).active(|s| s.bg(Theme::fill_pressed())).tip(label, None)
    };
    let maximized = window.is_maximized();
    // Native hit-test areas preserve Snap Layouts and the normal WM_CLOSE save guard.
    Some(div().absolute().top_0().right_0().h(px(crate::app::HEADER_H)).flex().items_center().title_bar()
        .child(div().mx(px(12.)).w(px(1.)).h(px(18.)).bg(Theme::line_divider()))
        .child(button("window-minimize", WindowControlArea::Min, "Minimize")
            .child(icon(Icon::Minus, 18., Theme::icon())))
        .child(button("window-maximize", WindowControlArea::Max, if maximized { "Restore" } else { "Maximize" }).child(icon(if maximized { Icon::Copy } else { Icon::Maximize }, 18., Theme::icon())))
        .child(button("window-close", WindowControlArea::Close, "Close")
            .child(icon(Icon::Close, 18., Theme::icon()))))
}


impl TipExt for Stateful<Div> {
    fn tip_label(self, label: String, side: TipSide) -> Self {
        let bounds = Rc::new(std::cell::Cell::new(None::<Bounds<Pixels>>));
        let record = bounds.clone();
        let label = SharedString::from(label);
        self.child(canvas(move |b, _, _| record.set(Some(b)), |_, _, _, _| {}).absolute().inset_0()).tooltip(move |window, cx| {
            let origin = window.mouse_position() + point(px(1.), px(1.));
            let (label, anchor) = (label.clone(), bounds.get());
            cx.new(|_| Tooltip { label, anchor, origin, side }).into()
        })
    }
}





/// Format search result times relative to the current day.
pub fn format_visit(secs: u64) -> String {
    let now = crate::time::now();
    let ago = now.saturating_sub(secs);
    if ago < 60 {
        return "just now".into();
    }
    if ago < 3600 {
        return format!("{} min ago", ago / 60);
    }
    let (y, mo, d, h, mi) = civil(secs);
    let day = |back: u64| {
        let (y, mo, d, _, _) = civil(now.saturating_sub(back * 86_400));
        (y, mo, d)
    };
    let names = ["Today", "Yesterday", "Two days ago"];
    if let Some(name) = (0..3).find(|&b| day(b) == (y, mo, d)).map(|b| names[b as usize]) {
        format!("{name} {h:02}:{mi:02}")
    } else if y == civil(now).0 {
        format!("{mo:02}-{d:02} {h:02}:{mi:02}")
    } else {
        format!("{y}-{mo:02}-{d:02}")
    }
}

/// Full-width CJK punctuation (、。〈〉《》「」『』【】〔〕（）：，… in U+3001–301B / U+FF01–FF65).
pub fn is_cjk_punct(ch: char) -> bool {
    matches!(ch, '\u{3001}'..='\u{3003}' | '\u{3008}'..='\u{3011}' | '\u{3014}'..='\u{301B}'
        | '\u{FF01}'..='\u{FF0F}' | '\u{FF1A}'..='\u{FF20}' | '\u{FF3B}'..='\u{FF40}' | '\u{FF5B}'..='\u{FF65}')
}

/// Font for full-width punctuation. When CoreText falls back from the
/// system UI font it squeezes these (an opening paren after a CJK
/// character loses ~5px). Browsers draw them full width with PingFang SC.
pub const CJK_PUNCT_FONT: &str = "PingFang SC";

/// Single-line UI text that keeps full-width punctuation at full width,
/// with an optional colored range (search hits). Inherits the parent text
/// style; honours `text_ellipsis()` against the final width (StyledText
/// caches its first, unconstrained measurement when nowrap).
pub struct UiText {
    text: SharedString,
    hit: Option<(std::ops::Range<usize>, Hsla)>,
}

pub fn ui_text(text: impl Into<SharedString>) -> UiText {
    UiText { text: text.into(), hit: None }
}

pub struct UiTextLayout {
    runs: Vec<TextRun>,
    font_size: Pixels,
    line_height: Pixels,
    ellipsis: Option<SharedString>,
    full: Rc<RefCell<Option<ShapedLine>>>,
}

impl UiText {
    pub fn hit(mut self, range: Option<std::ops::Range<usize>>, color: Hsla) -> Self {
        self.hit = range.map(|r| (r, color));
        self
    }

    fn runs(&self, style: &TextStyle) -> Vec<TextRun> {
        match &self.hit {
            Some((r, c)) => ui_runs(&self.text, style, std::slice::from_ref(r), *c),
            None => ui_runs(&self.text, style, &[], style.color),
        }
    }
}

/// Text runs that keep full-width punctuation at full width (see
/// [`CJK_PUNCT_FONT`]), with `hits` drawn in `hit_color`.
pub fn ui_runs(text: &str, style: &TextStyle, hits: &[std::ops::Range<usize>], hit_color: Hsla) -> Vec<TextRun> {
    let mut cuts = vec![0, text.len()];
    for (i, ch) in text.char_indices() {
        if is_cjk_punct(ch) {
            cuts.extend([i, i + ch.len_utf8()]);
        }
    }
    for r in hits {
        cuts.extend([r.start.min(text.len()), r.end.min(text.len())]);
    }
    cuts.sort();
    cuts.dedup();
    cuts.windows(2)
        .map(|w| {
            let mut run = style.to_run(w[1] - w[0]);
            if text[w[0]..w[1]].chars().all(is_cjk_punct) {
                run.font.family = CJK_PUNCT_FONT.into();
            }
            if hits.iter().any(|r| r.start <= w[0] && w[1] <= r.end) {
                run.color = hit_color;
            }
            run
        })
        .collect()
}

/// Longest prefix of `line` that fits `width` together with `suffix`.
fn truncated(line: &ShapedLine, runs: &[TextRun], suffix: &str, font_size: Pixels, width: Pixels, window: &mut Window) -> ShapedLine {
    let text = line.text.clone();
    let mut suffix_runs: Vec<TextRun> = runs.last().cloned().into_iter().collect();
    if let Some(r) = suffix_runs.first_mut() {
        r.len = suffix.len();
        r.font.family = theme::UI_FONT.into();
    }
    let suffix_w = window.text_system().shape_line(suffix.to_string().into(), font_size, &suffix_runs, None).width;
    let mut cut = 0;
    for (i, ch) in text.char_indices() {
        let end = i + ch.len_utf8();
        if line.x_for_index(end) + suffix_w > width {
            break;
        }
        cut = end;
    }
    let mut out = vec![];
    let mut at = 0;
    for r in runs {
        if at >= cut {
            break;
        }
        let mut r = r.clone();
        r.len = r.len.min(cut - at);
        at += r.len;
        out.push(r);
    }
    out.extend(suffix_runs);
    let s: SharedString = format!("{}{}", &text[..cut], suffix).into();
    window.text_system().shape_line(s, font_size, &out, None)
}

impl IntoElement for UiText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for UiText {
    type RequestLayoutState = UiTextLayout;
    type PrepaintState = Option<ShapedLine>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, UiTextLayout) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = style.line_height.to_pixels(font_size.into(), window.rem_size());
        let runs = if self.text.is_empty() { vec![] } else { self.runs(&style) };
        let ellipsis = style.text_overflow.as_ref().map(|TextOverflow::Truncate(s)| s.clone());
        let full = Rc::new(RefCell::new(None));
        let (text, r, f, clip) = (self.text.clone(), runs.clone(), full.clone(), ellipsis.is_some());
        let id = window.request_measured_layout(Style::default(), move |known, avail, window, _| {
            let mut slot = f.borrow_mut();
            let line = slot.get_or_insert_with(|| window.text_system().shape_line(text.clone(), font_size, &r, None));
            let mut w = line.width.ceil();
            if clip {
                let limit = known.width.or(match avail.width {
                    AvailableSpace::Definite(x) => Some(x),
                    _ => None,
                });
                if let Some(l) = limit {
                    w = w.min(l);
                }
            }
            size(w, line_height)
        });
        (id, UiTextLayout { runs, font_size, line_height, ellipsis, full })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut UiTextLayout,
        window: &mut Window,
        _: &mut App,
    ) -> Option<ShapedLine> {
        let full = layout.full.borrow().clone()?;
        match &layout.ellipsis {
            Some(suffix) if full.width > bounds.size.width + px(0.5) => {
                Some(truncated(&full, &layout.runs, suffix, layout.font_size, bounds.size.width, window))
            }
            _ => Some(full),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut UiTextLayout,
        line: &mut Option<ShapedLine>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(line) = line {
            let _ = line.paint(bounds.origin, layout.line_height, window, cx);
        }
    }
}

/// [`UiText`] that wraps: a paragraph keeping full-width punctuation at full
/// width, in the inherited text style.
pub fn ui_para(text: impl Into<SharedString>) -> UiPara {
    UiPara { text: text.into(), inner: None }
}

pub struct UiPara {
    text: SharedString,
    inner: Option<StyledText>,
}

impl IntoElement for UiPara {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for UiPara {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = window.text_style();
        let runs = ui_runs(&self.text, &style, &[], style.color);
        self.inner.insert(StyledText::new(self.text.clone()).with_runs(runs)).request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(t) = &mut self.inner {
            t.prepaint(id, inspector_id, bounds, layout, window, cx)
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut (),
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(t) = &mut self.inner {
            t.paint(id, inspector_id, bounds, layout, state, window, cx)
        }
    }
}

pub fn header_icon(id: &'static str, name: Icon) -> Stateful<Div> { toolbar_icon(id, name).no_drag() }

/// A 28pt button in the window's title bar, beside the traffic lights.
fn bar_button(id: &'static str, glyph: Icon, size: f32, color: Hsla) -> Stateful<Div> {
    div().id(id).no_drag().flex_none().size(px(28.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer().child(icon(glyph, size, color))
}

impl Workspace {
    pub(crate) fn sidebar_toggle(&self, id: &'static str, color: Hsla, cx: &mut Context<Self>) -> Stateful<Div> {
        bar_button(id, Icon::SidebarToggle, 17., color)
            .tip_action(if self.sidebar_open { "Hide sidebar" } else { "Show sidebar" }, &crate::actions::ToggleSidebar, cx)
            .on_click(cx.listener(|w, _, _, cx| w.toggle_sidebar(cx)))
    }

    pub(crate) fn home_button(&self, id: &'static str, size: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        bar_button(id, Icon::Home, size, Theme::icon()).on_click(cx.listener(|w, _, window, cx| w.show_view(crate::app::View::Home, window, cx)))
    }
}

