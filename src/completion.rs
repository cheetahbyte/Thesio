//! Completion: choices offered over text being written. A `Provider` says what
//! at the caret it can complete and what to offer. Its answer may take time,
//! so it is a task, and a newer query replaces one still running. The editor
//! owns the popup and everything that follows from a `Completion`.

use gpui::*;
use std::ops::Range;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    Citation,
    NoteLink,
}

/// A choice in the popup.
pub struct Item {
    pub title: String,
    pub detail: String,
    /// Longer text shown under the list while this item is selected.
    pub info: Option<String>,
    /// What replaces the range.
    pub insert: String,
    /// What to select within `insert` afterwards. Without it the caret lands
    /// after the text.
    pub selection: Option<Range<usize>>,
}

/// What a provider can complete at the caret.
pub struct Found {
    /// The bytes an accepted item replaces.
    pub range: Range<usize>,
    /// The text so far, which the provider matches against.
    pub query: String,
}

pub struct Query {
    pub text: String,
    /// The popup is opening, rather than following typing.
    pub opening: bool,
}

pub trait Provider {
    fn kind(&self) -> CompletionKind;
    fn find(&self, text: &str, caret: usize) -> Option<Found>;
    fn items(&self, query: Query, cx: &mut App) -> Task<Vec<Item>>;
}

/// The popup's state, over `range`.
pub struct Completion {
    pub kind: CompletionKind,
    pub range: Range<usize>,
    pub provider: Rc<dyn Provider>,
    pub query: String,
    pub items: Vec<Item>,
    pub selected: usize,
    /// The running query. Replacing it drops its answer.
    pub pending: Option<Task<()>>,
    pub scroll: UniformListScrollHandle,
}

impl Completion {
    pub fn new(provider: Rc<dyn Provider>, found: Found) -> Self {
        Self { kind: provider.kind(), range: found.range, provider, query: found.query, items: vec![], selected: 0, pending: None, scroll: UniformListScrollHandle::new() }
    }

    pub fn select(&mut self, index: usize) {
        let strategy = if index > self.selected { ScrollStrategy::Bottom } else { ScrollStrategy::Top };
        self.selected = index;
        self.scroll.scroll_to_item(index, strategy);
    }
}
