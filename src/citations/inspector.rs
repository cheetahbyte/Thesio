//! The panel beside the table that edits the selected entry.

use crate::app::{View, Workspace};
use crate::confirm::Confirm;
use crate::input::{TextField, TextFieldEvent};
use crate::library::InvalidLibrary;
use crate::overlay::Overlay;
use crate::theme::{self, Theme, craft};
use crate::ui::{Icon, ToastKind, anchored_menu, icon, list_line, menu_action, menu_glyph, menu_label, menu_panel, menu_row, ui_text};
use gpui::{prelude::FluentBuilder, *};
use thesio_core::references::citations_of;
use thesio_core::source_type::{Field, JOURNAL_ARTICLE, SOURCE_TYPES, SourceType};

pub(crate) const INSPECTOR_W: f32 = 320.;

/// The panel beside the table that edits the selected entry.
pub struct Inspector {
    key: String,
    key_field: Entity<TextField>,
    fields: Vec<(Field, Entity<TextField>)>,
    error: Option<String>,
    /// The documents citing this entry: file, times cited, and where it first is.
    cited_in: Vec<(String, usize, usize)>,
    _subs: Vec<Subscription>,
}

/// The panel, and whether the user closed it.
pub(crate) struct Details {
    pub(crate) panel: Option<Inspector>,
    pub(crate) hidden: bool,
}

impl Workspace {
    /// Shows `key` in the inspector and puts the caret in its first field, or
    /// without a key, adds a blank entry to fill in.
    pub(crate) fn open_entry(&mut self, key: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let key = match key {
            Some(key) => key,
            None => {
                let Some(library) = &mut self.citations.library else { return };
                let key = match library.add_blank(&JOURNAL_ARTICLE) {
                    Ok(key) => key,
                    Err(e) => return self.show_toast_kind(ToastKind::Error, &format!("Could not add an entry: {e}"), cx),
                };
                self.citations.tag_filter = None;
                self.citations.table.search.update(cx, |search, cx| search.set_text("", cx));
                self.refresh_after_library_change(cx);
                key
            }
        };
        if self.view != View::Library { self.show_view(View::Library, window, cx); }
        if self.citations.details.hidden { self.set_details_hidden(false, window, cx); }
        self.select_only(&key);
        self.table_changed(window, cx);
        if let Some((_, field)) = self.citations.details.panel.as_ref().and_then(|inspector| inspector.fields.first()) { window.focus(&field.focus_handle(cx)); }
        cx.notify();
    }

    pub(crate) fn set_details_hidden(&mut self, hidden: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.citations.details.hidden = hidden;
        self.sync_inspector(window, cx);
        if hidden { window.focus(&self.citations.table.focus); }
        self.set_pref(|p| p.library_details_hidden = hidden, cx);
        cx.notify();
    }

    /// Matches the inspector to the selection: one visible entry, or none.
    pub(crate) fn sync_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_in(&self.library_rows(cx));
        let wanted = match selected.as_slice() {
            [key] if !self.citations.details.hidden => Some(key.clone()),
            _ => None,
        };
        if self.citations.details.panel.as_ref().map(|inspector| &inspector.key) == wanted.as_ref() { return; }
        // A field's blur arrives after this frame, once the field is gone.
        self.save_inspector(cx);
        self.citations.details.panel = wanted.and_then(|key| self.build_inspector(key, window, cx));
    }

    pub(crate) fn inspector_field(&mut self, example: &str, text: &str, wrap: bool, subs: &mut Vec<Subscription>, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextField> {
        let field = cx.new(|cx| {
            // Worded as an example: a bare value in an empty field reads as data.
            let hint = if example.is_empty() { String::new() } else { format!("e.g. {example}") };
            let mut f = TextField::new(hint, cx);
            f.placeholder_color = Theme::text_disabled;
            f.tab_events = true;
            f.wrap = wrap;
            f.set_text(text, cx);
            f
        });
        subs.push(cx.subscribe_in(&field, window, |w, field, ev: &TextFieldEvent, window, cx| {
            let step = match ev {
                TextFieldEvent::Submit => return w.save_inspector(cx),
                TextFieldEvent::Cancel => return w.revert_inspector(window, cx),
                TextFieldEvent::Tab => 1,
                TextFieldEvent::ShiftTab => -1,
                _ => return,
            };
            w.save_inspector(cx);
            let Some(inspector) = &w.citations.details.panel else { return };
            let order: Vec<&Entity<TextField>> = std::iter::once(&inspector.key_field).chain(inspector.fields.iter().map(|(_, field)| field)).collect();
            let Some(at) = order.iter().position(|f| *f == field) else { return };
            let next = (at as i32 + step).rem_euclid(order.len() as i32) as usize;
            window.focus(&order[next].focus_handle(cx));
        }));
        let handle = field.focus_handle(cx);
        subs.push(cx.on_blur(&handle, window, |w, _, cx| w.save_inspector(cx)));
        field
    }

    pub(crate) fn build_inspector(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) -> Option<Inspector> {
        let library = self.citations.library.as_ref()?;
        let kind = library.source_type(&key)?;
        let values: Vec<(Field, String)> = kind.fields.iter().map(|field| (*field, library.field_text(&key, field))).collect();
        let mut subs = vec![];
        let key_field = self.inspector_field("knuth1984", &key, false, &mut subs, window, cx);
        let fields = values.into_iter().map(|(field, text)| (field, self.inspector_field(field.hint, &text, field.long, &mut subs, window, cx))).collect();
        let cited_in = self.entries.iter().filter(|e| e.typst).filter_map(|e| {
            let places = citations_of(&e.body, &key);
            places.first().map(|first| (e.id.clone(), places.len(), *first))
        }).collect();
        Some(Inspector { key, key_field, fields, error: None, cited_in, _subs: subs })
    }

    /// Esc: back to what the file holds.
    pub(crate) fn revert_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.citations.details.panel.take().map(|inspector| inspector.key) else { return };
        self.citations.details.panel = self.build_inspector(key, window, cx);
        window.focus(&self.citations.table.focus);
        cx.notify();
    }

    /// Writes what the inspector holds. The fields stay as typed when the
    /// library refuses them, with the reason beside them.
    pub(crate) fn save_inspector(&mut self, cx: &mut Context<Self>) {
        let Some(inspector) = &self.citations.details.panel else { return };
        let key = inspector.key.clone();
        let new_key = inspector.key_field.read(cx).text().trim().to_string();
        let fields: Vec<(Field, String)> = inspector.fields.iter().map(|(field, entity)| (*field, entity.read(cx).text().to_string())).collect();
        let Some(library) = &mut self.citations.library else { return };
        let before = library.paper_ids(&key);
        let result = library.save_fields(&key, &new_key, &fields);
        let found_by_new_id = library.paper_ids(&new_key) != before;
        // Beside the field, the file's path only gets in the way.
        let error = result.as_ref().err().map(|e| e.downcast_ref::<InvalidLibrary>().map_or_else(|| e.to_string(), |invalid| invalid.reason.clone()));
        if let Some(inspector) = &mut self.citations.details.panel { inspector.error = error; }
        if matches!(result, Ok(true)) {
            // Before the refresh: bibliographies list what documents cite.
            if new_key != key { self.rename_citations(&key, &new_key, cx); }
            self.refresh_after_library_change(cx);
            if new_key != key {
                self.select_only(&new_key);
                if let Some(inspector) = &mut self.citations.details.panel { inspector.key = new_key.clone(); }
                self.rename_paper(&key, &new_key, cx);
            }
            if found_by_new_id { self.fetch_papers(vec![new_key], cx); }
        }
        cx.notify();
    }

    /// Changes the type, asking first if that would lose data.
    pub(crate) fn choose_source_type(&mut self, kind: &'static SourceType, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.citations.details.panel.as_ref().map(|inspector| inspector.key.clone()) else { return };
        if self.citations.library.as_ref().is_some_and(|library| library.type_change_drops_parent(&key, kind)) {
            return self.open_overlay(Overlay::Confirm(Confirm::ChangeSourceType { key, kind }), window, cx);
        }
        self.set_source_type(&key, kind, window, cx);
    }

    pub(crate) fn set_source_type(&mut self, key: &str, kind: &'static SourceType, window: &mut Window, cx: &mut Context<Self>) {
        self.save_inspector(cx);
        let Some(library) = &mut self.citations.library else { return };
        if let Err(e) = library.set_source_type(key, kind) { self.show_toast_kind(ToastKind::Error, &format!("Could not change the type: {e}"), cx); }
        if self.citations.details.panel.as_ref().is_some_and(|inspector| inspector.key == key) { self.citations.details.panel = self.build_inspector(key.to_string(), window, cx); }
        self.refresh_after_library_change(cx);
        cx.notify();
    }

    pub(crate) fn render_inspector(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let inspector = self.citations.details.panel.as_ref()?;
        let kind = self.citations.library.as_ref()?.source_type(&inspector.key)?;
        let row = |label: SharedString, value: AnyElement| {
            div().w_full().flex().items_start().gap(px(10.)).py(px(4.))
                .child(div().w(px(92.)).flex_none().text_right().text_size(px(12.)).line_height(px(20.)).text_color(Theme::text_caption()).child(label))
                .child(div().flex_1().min_w_0().text_size(px(13.)).line_height(px(20.)).child(value))
                .into_any_element()
        };
        let type_button = div().id("inspector-type").flex().items_center().gap(px(4.)).cursor_pointer().rounded(px(4.))
            .active(|s| s.bg(Theme::fill_pressed()))
            .child(kind.label)
            .child(icon(Icon::ChevronDown, 10., Theme::text_caption()))
            .on_click(cx.listener(|w, ev: &ClickEvent, window, cx| w.open_overlay(Overlay::TypeMenu(ev.position()), window, cx)));
        Some(div().id("inspector").w(px(INSPECTOR_W)).flex_none().h_full().bg(Theme::fill_group()).border_l_1().border_color(list_line()).px(px(12.)).pb(px(16.)).overflow_y_scroll().flex().flex_col()
            .child(row("Type".into(), type_button.into_any_element()))
            .children(inspector.fields.iter().map(|(field, entity)| row(field.label.into(), entity.clone().into_any_element())))
            .child(row("Citation Key".into(), inspector.key_field.clone().into_any_element()))
            .child(div().mt(px(14.)).mb(px(2.)).pl(px(4.)).text_size(px(12.)).text_color(Theme::text_caption()).child("Cited In"))
            .when(inspector.cited_in.is_empty(), |d| d.child(div().pl(px(4.)).text_size(px(13.)).text_color(Theme::text_placeholder()).child("No document yet")))
            .children(inspector.cited_in.iter().enumerate().map(|(i, (id, count, first))| {
                let (id, offset) = (id.clone(), *first);
                div().id(("cited-in", i)).h(px(28.)).px(px(4.)).flex().items_center().gap(px(8.)).rounded(px(crate::ui::ROW_RADIUS)).cursor_pointer().text_size(px(13.))
                    .active(|s| s.bg(Theme::fill_pressed()))
                    .child(div().flex_1().min_w_0().truncate().child(ui_text(id.clone())))
                    .child(div().flex_none().text_size(px(12.)).text_color(Theme::text_caption()).child(count.to_string()))
                    .on_click(cx.listener(move |w, _, window, cx| w.go_to_source(&id, offset, window, cx)))
            }))
            .when_some(inspector.error.clone(), |d, error| d.child(div().mt(px(8.)).text_size(px(12.)).line_height(px(16.)).text_color(theme::c(craft::R400)).child(error)))
            .into_any_element())
    }

    pub(crate) fn render_type_menu(&self, position: Point<Pixels>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let inspector = self.citations.details.panel.as_ref()?;
        let current = self.citations.library.as_ref()?.source_type(&inspector.key)?;
        let rows = SOURCE_TYPES.iter().copied().enumerate().map(|(i, kind)| {
            menu_row(("inspector-type-row", i))
                .child(div().w(px(16.)).flex_none().when(std::ptr::eq(kind, current), |d| d.child(menu_glyph(Icon::Check, 14.))))
                .child(menu_label(kind.label))
                .on_click(menu_action(cx, move |w, window, cx| w.choose_source_type(kind, window, cx)))
        }).collect::<Vec<_>>();
        let menu = menu_panel("inspector-type-menu", 220., cx).max_h(px(420.)).overflow_y_scroll().children(rows);
        Some(anchored_menu(Corner::TopLeft, position, menu))
    }
}
