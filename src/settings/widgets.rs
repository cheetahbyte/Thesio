//! Settings controls: switch on the accent, grouped rows.

use crate::theme::Theme;
use crate::ui;
use gpui::{prelude::FluentBuilder, *};

/// A group: the rows in one rounded card, split by hairlines.
pub(crate) fn group(rows: Vec<AnyElement>) -> Div {
    let n = rows.len();
    div().w_full().flex().flex_col().rounded(px(8.)).border_1().border_color(Theme::line_border()).overflow_hidden()
        .children(rows.into_iter().enumerate().map(move |(i, r)| div().w_full().flex().flex_col().when(i + 1 < n, |d| d.border_b_1().border_color(ui::list_line())).child(r)))
}

/// One setting: title and an optional description on the left, the control on the right.
pub(crate) fn row(title: impl Into<SharedString>, desc: Option<SharedString>, control: impl IntoElement) -> Div {
    div().w_full().min_h(px(40.)).px(px(12.)).py(px(8.)).flex().items_center().justify_between().gap(px(24.))
        .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.))
            .child(div().text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(title.into()))
            .children(desc.map(|d| div().text_size(px(11.)).line_height(px(15.)).text_color(Theme::text_caption()).child(d))))
        .child(div().flex_none().flex().items_center().gap(px(8.)).child(control))
}

/// An on/off switch: flat, the accent when on, the knob at that end.
pub(crate) fn switch(id: &'static str, on: bool) -> Stateful<Div> {
    div().id(id).flex_none().w(px(30.)).h(px(18.)).p(px(2.)).flex().items_center().rounded_full().cursor_pointer()
        .when(on, |d| d.justify_end().bg(Theme::primary()))
        .when(!on, |d| d.bg(Theme::fill_active()))
        .child(div().size(px(14.)).rounded_full().bg(gpui::white()))
}
