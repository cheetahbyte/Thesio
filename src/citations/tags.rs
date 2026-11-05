//! Tags: the filter chips' menu, renaming, and the tag editor popover.

use crate::confirm::Confirm;
use crate::app::Workspace;
use crate::input::{TextField, TextFieldEvent};
use thesio_core::library::Row;
use crate::overlay::{Overlay, menu_action, menu_panel};
use crate::theme::Theme;
use crate::ui::{Icon, ToastKind, anchored_menu, field_well, icon, menu_glyph, menu_label, menu_row, popover};
use gpui::{prelude::FluentBuilder, *};
use std::collections::BTreeSet;

/// The popover that tags the selected entries.
pub struct TagEditor {
    keys: Vec<String>,
    field: Entity<TextField>,
    position: Point<Pixels>,
    _subs: Vec<Subscription>,
}

/// The popover that renames a tag, opened from its menu.
pub struct TagRename {
    tag: String,
    field: Entity<TextField>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

impl Workspace {
    pub(crate) fn open_tag_editor(&mut self, keys: Vec<String>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut f = TextField::new("Add a tag", cx);
            f.placeholder_color = Theme::text_disabled;
            f
        });
        let tagged = keys.clone();
        let subs = vec![cx.subscribe_in(&field, window, move |w, field, ev: &TextFieldEvent, window, cx| match ev {
            TextFieldEvent::Submit => {
                let tag = field.read(cx).text().to_string();
                if tag.trim().is_empty() { return; }
                if w.set_tag(&tagged, &tag, true, window, cx) { field.update(cx, |f, cx| f.set_text("", cx)); }
            }
            _ => cx.notify(),
        })];
        let focus = field.focus_handle(cx);
        self.open_overlay(Overlay::TagEditor(TagEditor { keys, field, position, _subs: subs }), window, cx);
        window.focus(&focus);
    }

    pub(crate) fn open_tag_rename(&mut self, tag: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut f = TextField::new("Tag name", cx);
            f.set_text(&tag, cx);
            f
        });
        let subscription = cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| if let TextFieldEvent::Submit = ev { w.finish_tag_rename(window, cx); });
        let focus = field.focus_handle(cx);
        self.open_overlay(Overlay::TagRename(TagRename { tag, field, position, _subscription: subscription }), window, cx);
        window.focus(&focus);
    }

    pub(crate) fn finish_tag_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::TagRename(TagRename { tag: old, field, .. })) = self.take_overlay(window, cx) else { return };
        let new = field.read(cx).text().trim().to_string();
        let Some(library) = &mut self.citations.library else { return };
        if new != old {
            match library.rename_tag(&old, &new) {
                Ok(()) => if self.citations.tag_filter.as_ref() == Some(&old) {
                    self.citations.tag_filter = library.tag_like(&new);
                },
                Err(e) => self.show_toast_kind(ToastKind::Error, &format!("Could not rename the tag: {e}"), cx),
            }
        }
        self.table_changed(window, cx);
    }

    pub(crate) fn delete_tag(&mut self, tag: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(library) = &mut self.citations.library else { return };
        if let Err(e) = library.delete_tag(tag) { self.show_toast_kind(ToastKind::Error, &format!("Could not delete the tag: {e}"), cx); }
        self.table_changed(window, cx);
    }

    /// How many entries carry `tag`.
    pub(crate) fn tag_count(&self, tag: &str) -> usize {
        self.citations.library.as_ref().map_or(0, |library| library.tags().into_iter().find(|(name, _)| name == tag).map_or(0, |(_, count)| count))
    }

    /// Applies `tag` to the entries `keys`. False if it was refused.
    pub(crate) fn set_tag(&mut self, keys: &[String], tag: &str, on: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(library) = &mut self.citations.library else { return false };
        let result = library.set_tag(keys, tag, on);
        if let Err(e) = &result { self.show_toast_kind(ToastKind::Error, &format!("Could not change tags: {e}"), cx); }
        self.table_changed(window, cx);
        result.is_ok()
    }

    pub(crate) fn render_tag_rename(&self, rename: &TagRename, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = popover().id("tag-rename").occlude().w(px(220.)).p(px(8.))
            .on_mouse_down_out(cx.listener(|w, _, window, cx| w.finish_tag_rename(window, cx)))
            .child(field_well(&rename.field, window, cx));
        anchored_menu(Corner::TopLeft, rename.position, panel)
    }

    pub(crate) fn render_tag_menu(&self, tag: &str, position: Point<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let (rename, delete) = (tag.to_string(), tag.to_string());
        let menu = menu_panel("tag-menu", 180., cx)
            .child(menu_row("tag-menu-rename").child(menu_glyph(Icon::Edit, 16.)).child(menu_label("Rename…")).on_click(menu_action(cx, move |w, window, cx| w.open_tag_rename(rename.clone(), position, window, cx))))
            .child(menu_row("tag-menu-delete").text_color(Theme::danger()).child(icon(Icon::Delete, 16., Theme::danger())).child(menu_label("Delete")).on_click(cx.listener(move |w, _, window, cx| w.open_overlay(Overlay::Confirm(Confirm::DeleteTag(delete.clone())), window, cx))));
        anchored_menu(Corner::TopLeft, position, menu)
    }

    pub(crate) fn render_tag_editor(&self, editor: &TagEditor, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let library = self.citations.library.as_ref()?;
        let typed = editor.field.read(cx).text().trim().to_string();
        let query = typed.to_lowercase();
        let keys: BTreeSet<&String> = editor.keys.iter().collect();
        let rows: Vec<Row> = library.rows().into_iter().filter(|row| keys.contains(&row.key)).collect();
        let tags = library.tags();
        let exists = library.tag_like(&typed).is_some();
        let list = tags.into_iter().filter(|(tag, _)| tag.to_lowercase().contains(&query)).enumerate().map(|(i, (tag, _))| {
            let holders = rows.iter().filter(|row| row.tags.contains(&tag)).count();
            let all = holders == rows.len();
            let mark = if all { Some(Icon::Check) } else if holders > 0 { Some(Icon::Minus) } else { None };
            let keys = editor.keys.clone();
            menu_row(("tag-editor-row", i))
                .child(div().w(px(16.)).flex_none().children(mark.map(|mark| menu_glyph(mark, 14.))))
                .child(menu_label(tag.clone()))
                .on_click(cx.listener(move |w, _, window, cx| { w.set_tag(&keys, &tag, !all, window, cx); }))
        }).collect::<Vec<_>>();
        let panel = menu_panel("tag-editor", 240., cx)
            .child(div().px(px(8.)).pb(px(4.)).child(field_well(&editor.field, window, cx)))
            .child(div().id("tag-editor-list").max_h(px(220.)).overflow_y_scroll().children(list))
            .when(!typed.is_empty() && !exists, |d| d.child(menu_row("tag-editor-create").child(menu_glyph(Icon::Plus, 14.)).child(menu_label(format!("Create “{typed}”"))).on_click(cx.listener({
                let typed = typed.clone();
                let keys = editor.keys.clone();
                let field = editor.field.clone();
                move |w, _, window, cx| if w.set_tag(&keys, &typed, true, window, cx) { field.update(cx, |f, cx| f.set_text("", cx)); }
            }))));
        Some(anchored_menu(Corner::TopLeft, editor.position, panel))
    }
}
