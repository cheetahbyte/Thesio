//! The Sources page: the papers in the vault.

use crate::app::Workspace;
use crate::sidebar::RowMenu;
use crate::citations::picker::PickFor;
use crate::citations::table::{CELL_X, Col, ROW_H, TABLE_X, column, column_spec};
use crate::library::{Library, Row, row_of_paper};
use crate::overlay::Overlay;
use crate::theme::{self, Theme};
use crate::ui::{Icon, TipExt, TitleBarExt, list_header_text, list_line, page_header, primary_icon_button, ui_text};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::paths::{base_name, natural_cmp};

impl Workspace {
    pub(crate) fn render_sources(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.citations.library.as_ref().map_or(vec![], Library::rows);
        let mut papers: Vec<(String, Option<Row>, String)> = self.paper_ids().into_iter().map(|id| {
            let row = row_of_paper(&rows, &id).cloned();
            let title = row.as_ref().map(|row| row.title.clone()).filter(|title| !title.is_empty())
                .unwrap_or_else(|| base_name(&id).to_string());
            (id, row, title)
        }).collect();
        papers.sort_by(|a, b| natural_cmp(&a.2, &b.2));
        let fetching = self.papers.fetching;
        let header = page_header(self, "Sources", (!papers.is_empty()).then_some(papers.len()), cx)
            .when(fetching > 0, |d| d.child(div().mr(px(12.)).text_size(px(13.)).text_color(Theme::text_caption()).child(if fetching == 1 { "Downloading 1 paper…".to_string() } else { format!("Downloading {fetching} papers…") })))
            .child(primary_icon_button("sources-add", Icon::Plus, "Add").no_drag().tip("Add a PDF", None).on_click(cx.listener(|w, _, window, cx| {
                w.choose_pdf(window, cx, |w, path, window, cx| w.open_source_picker(PickFor::Attach(path), window, cx));
            })));
        let page = div().flex_1().min_w_0().h_full().flex().flex_col().child(header);
        if papers.is_empty() {
            return page.child(crate::ui::empty_state(Icon::Paper, "No papers yet", "Open-access papers download here, or add a PDF yourself.")).into_any_element();
        }
        let table_header = div().h(px(26.)).flex_none().mx(px(TABLE_X)).px(px(CELL_X)).flex().items_center().border_b_1().border_color(list_line())
            .text_size(px(12.)).line_height(px(24.)).text_color(list_header_text())
            .children([Col::Title, Col::Authors, Col::Year, Col::Key].map(|col| { let (label, width) = column_spec(col); column(width).child(label) }));
        let count = papers.len();
        let table = uniform_list("source-rows", count, cx.processor(move |_: &mut Workspace, range: std::ops::Range<usize>, _, cx| {
            range.map(|i| {
                let (id, row, title) = &papers[i];
                let text = |pick: fn(&Row) -> &String| row.as_ref().map_or(String::new(), |row| pick(row).clone());
                let cell = |col: Col, value: String| column(column_spec(col).1).truncate().child(ui_text(value));
                let (open, menu, link) = (id.clone(), id.clone(), id.clone());
                let unlinked = row.is_none();
                div().id(("source-row", i)).w_full().h(px(ROW_H)).px(px(CELL_X)).flex().items_center().cursor_pointer().rounded(px(crate::ui::ROW_RADIUS)).text_size(px(13.)).text_color(Theme::text_caption())
                    .child(cell(Col::Title, title.clone()).text_color(Theme::text()))
                    .child(cell(Col::Authors, text(|row| &row.authors)))
                    .child(cell(Col::Year, text(|row| &row.year)))
                    .when(!unlinked, |d| d.child(cell(Col::Key, text(|row| &row.key)).font_family(theme::MONO_FONT).text_size(px(12.))))
                    .when(unlinked, |d| d.child(column(column_spec(Col::Key).1).pr_0().flex().child(
                        div().id(("source-link", i)).px(px(8.)).h(px(22.)).flex().items_center().rounded(px(5.)).text_size(px(12.)).text_color(Theme::text())
                            .border_1().border_color(Theme::line_border()).active(|s| s.bg(Theme::fill_pressed()))
                            .child("Link to Source…")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |w, _, window, cx| {
                                cx.stop_propagation();
                                w.open_source_picker(PickFor::Link(link.clone()), window, cx);
                            })),
                    )))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, window, cx| {
                        w.open_overlay(Overlay::RowMenu(RowMenu { id: menu.clone(), pos: ev.position, batch: false }), window, cx);
                    }))
                    .on_click(cx.listener(move |w, _, window, cx| w.open_paper(&open, window, cx)))
            }).collect()
        })).flex_1().min_h_0().px(px(TABLE_X)).pt(px(4.));
        page.child(table_header).child(table).into_any_element()
    }
}
