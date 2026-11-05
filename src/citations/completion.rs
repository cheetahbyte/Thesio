//! `@key`: citations from the library and labels from the compiled document,
//! in one list.

use super::table::DEFAULT_SORT;
use crate::app::Workspace;
use crate::completion::{CompletionKind, Found, Item, Provider, Query, Reach};
use crate::session::DocumentSession;
use gpui::*;
use thesio_core::compiler::DocumentLabel;
use thesio_core::references::citation_at;

pub struct CitationKeys {
    workspace: WeakEntity<Workspace>,
    session: Option<WeakEntity<DocumentSession>>,
}

impl CitationKeys {
    pub fn new(workspace: WeakEntity<Workspace>, session: Option<WeakEntity<DocumentSession>>) -> Self { Self { workspace, session } }
}

impl Provider for CitationKeys {
    fn kind(&self) -> CompletionKind { CompletionKind::Citation }

    fn find(&self, text: &str, caret: usize, _: Reach) -> Option<Found> {
        let range = citation_at(text, caret)?;
        Some(Found { query: text[range.start + 1..caret].to_lowercase(), range })
    }

    fn items(&self, query: Query, cx: &mut App) -> Task<Vec<Item>> {
        let workspace = self.workspace.clone();
        // The session's own main file, so a chapter sees the whole document.
        let compilation = self.session.as_ref().and_then(WeakEntity::upgrade).and_then(|session| session.read(cx).document().cloned());
        cx.spawn(async move |cx| workspace.update(cx, |w, _| {
            let labels = compilation.as_ref().map_or(&[][..], |compilation| &compilation.labels);
            ranked(w.citation_items(&query), labels, &query.text)
        }).unwrap_or_default())
    }
}

fn label_item(label: &DocumentLabel) -> Item {
    let page = label.page.map(|page| format!("p. {page}"));
    Item {
        kind: CompletionKind::Reference,
        title: label.title.clone(),
        detail: [Some(label.kind.as_str()), page.as_deref(), Some(label.key.as_str())].into_iter().flatten().collect::<Vec<_>>().join(" · "),
        info: None,
        insert: format!("@{}", label.key),
        selection: None,
        stops: vec![],
        from: None,
    }
}

/// Sources in their order, each with whether its key starts with `query`, and
/// labels joined to them: what starts with `query` first, then whatever matches
/// further in, sources ahead of labels in both. `query` is lowercase.
fn ranked(mut items: Vec<(bool, Item)>, labels: &[DocumentLabel], query: &str) -> Vec<Item> {
    items.extend(labels.iter().filter_map(|label| {
        let key = label.key.to_lowercase();
        let by_key = key.starts_with(query);
        (by_key || key.contains(query) || label.title.to_lowercase().contains(query)).then(|| (by_key, label_item(label)))
    }));
    items.sort_by_key(|(by_key, _)| !*by_key);
    items.into_iter().map(|(_, item)| item).collect()
}

impl Workspace {
    /// The sources `@` offers, matching keys first, each with whether its key
    /// matched. An opening popup sees the library as it is on disk now.
    fn citation_items(&mut self, query: &Query) -> Vec<(bool, Item)> {
        if query.opening { self.reload_library(); }
        let mut rows = self.visible_rows(&query.text, DEFAULT_SORT);
        rows.sort_by_key(|row| !row.key.to_lowercase().starts_with(&query.text));
        rows.into_iter().map(|row| (row.key.to_lowercase().starts_with(&query.text), Item {
            kind: CompletionKind::Citation,
            detail: [row.authors.as_str(), row.year.as_str(), row.key.as_str()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · "),
            insert: format!("@{}", row.key),
            title: row.label().to_string(),
            info: None,
            selection: None,
            stops: vec![],
            from: None,
        })).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{CompletionKind, DocumentLabel, Item, label_item, ranked};

    fn label(key: &str, title: &str) -> DocumentLabel { DocumentLabel { key: key.into(), title: title.into(), kind: "Figure".into(), page: Some(3) } }

    fn source(key: &str, by_key: bool) -> (bool, Item) {
        (by_key, Item { kind: CompletionKind::Citation, title: key.into(), detail: String::new(), info: None, insert: format!("@{key}"), selection: None, stops: vec![], from: None })
    }

    fn inserted(items: &[Item]) -> Vec<&str> { items.iter().map(|item| item.insert.as_str()).collect() }

    #[test]
    fn matching_keys_come_first_and_a_source_comes_before_a_label() {
        let sources = vec![source("smith2020", true), source("jones2021", false)];
        let labels = [label("plot", "Smith's data"), label("smooth", "A curve"), label("other", "Nothing")];
        assert_eq!(inserted(&ranked(sources, &labels, "sm")), ["@smith2020", "@smooth", "@jones2021", "@plot"]);
    }

    #[test]
    fn with_nothing_typed_every_source_then_every_label() {
        let items = ranked(vec![source("a", true), source("b", true)], &[label("fig", "Plot")], "");
        assert_eq!(inserted(&items), ["@a", "@b", "@fig"]);
    }

    #[test]
    fn a_label_says_what_it_is_and_leaves_citing_alone() {
        let item = label_item(&label("fig:plot", "A plot"));
        assert_eq!((item.title.as_str(), item.detail.as_str(), item.insert.as_str()), ("A plot", "Figure · p. 3 · fig:plot", "@fig:plot"));
        assert_eq!(item.kind, CompletionKind::Reference);
        assert_eq!(label_item(&DocumentLabel { page: None, ..label("eq", "x") }).detail, "Figure · eq");
    }

    #[test]
    fn a_label_is_found_by_its_title_whatever_its_case() {
        let labels = [label("fig:Plot", "Growth Curve")];
        assert_eq!(inserted(&ranked(vec![], &labels, "growth")), ["@fig:Plot"]);
        assert_eq!(inserted(&ranked(vec![], &labels, "fig:p")), ["@fig:Plot"]);
        assert!(ranked(vec![], &labels, "table").is_empty());
    }
}
