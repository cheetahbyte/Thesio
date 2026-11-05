//! `@key` citations, from the library.

use super::table::DEFAULT_SORT;
use crate::app::Workspace;
use crate::completion::{CompletionKind, Found, Item, Provider, Query};
use gpui::*;
use thesio_core::references::citation_at;

pub struct CitationKeys(pub WeakEntity<Workspace>);

impl Provider for CitationKeys {
    fn kind(&self) -> CompletionKind { CompletionKind::Citation }

    fn find(&self, text: &str, caret: usize) -> Option<Found> {
        let range = citation_at(text, caret)?;
        Some(Found { query: text[range.start + 1..caret].to_lowercase(), range })
    }

    fn items(&self, query: Query, cx: &mut App) -> Task<Vec<Item>> {
        let workspace = self.0.clone();
        cx.spawn(async move |cx| workspace.update(cx, |w, _| w.citation_items(&query)).unwrap_or_default())
    }
}

impl Workspace {
    /// What `@` offers, matching keys first. An opening popup sees the library
    /// as it is on disk now.
    fn citation_items(&mut self, query: &Query) -> Vec<Item> {
        if query.opening { self.reload_library(); }
        let mut rows = self.visible_rows(&query.text, DEFAULT_SORT);
        rows.sort_by_key(|row| !row.key.to_lowercase().starts_with(&query.text));
        rows.into_iter().map(|row| Item {
            detail: [row.authors.as_str(), row.year.as_str(), row.key.as_str()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · "),
            insert: format!("@{}", row.key),
            title: row.label().to_string(),
            info: None,
            selection: None,
        }).collect()
    }
}
