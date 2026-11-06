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
    /// `@` pointing at a label in the document, which needs no bibliography.
    Reference,
    NoteLink,
    Typst,
}

pub struct Item {
    /// What accepting it adds to the document.
    pub kind: CompletionKind,
    pub title: String,
    pub detail: String,
    /// Longer text shown under the list while this item is selected.
    pub info: Option<String>,
    /// What replaces the range.
    pub insert: String,
    /// What to select within `insert` afterwards. Without it the caret lands
    /// after the text.
    pub selection: Option<Range<usize>>,
    /// The other placeholders in `insert`, which Tab selects in order.
    pub stops: Vec<Range<usize>>,
    /// Where the replaced text starts, when an answer knows better than `find`.
    pub from: Option<usize>,
    /// Bytes after the caret it also replaces.
    pub tail: usize,
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
    /// The whole text and the caret, as of this query.
    pub document: String,
    pub caret: usize,
    /// The user opened the popup with ⌃Space.
    pub asked: bool,
}

/// How far a provider should look for something to complete.
#[derive(Clone, Copy)]
pub enum Reach {
    /// Only where typing opens the popup by itself.
    Typed,
    /// Anywhere: the user asked.
    Asked,
    /// Also wherever the provider's popup, open from this byte, can go on.
    Open(usize),
}

pub trait Provider {
    fn kind(&self) -> CompletionKind;
    fn find(&self, text: &str, caret: usize, reach: Reach) -> Option<Found>;
    fn items(&self, query: Query, cx: &mut App) -> Task<Vec<Item>>;
}

/// The popup's state, over `range`.
pub struct Completion {
    pub kind: CompletionKind,
    pub range: Range<usize>,
    pub provider: Rc<dyn Provider>,
    pub query: String,
    /// Opened with ⌃Space.
    pub asked: bool,
    pub items: Vec<Item>,
    pub selected: usize,
    /// The running query. Replacing it drops its answer.
    pub pending: Option<Task<()>>,
    pub scroll: UniformListScrollHandle,
}

impl Completion {
    pub fn new(provider: Rc<dyn Provider>, found: Found, asked: bool) -> Self {
        Self { kind: provider.kind(), range: found.range, provider, query: found.query, asked, items: vec![], selected: 0, pending: None, scroll: UniformListScrollHandle::new() }
    }

    pub fn select(&mut self, index: usize) {
        let strategy = if index > self.selected { ScrollStrategy::Bottom } else { ScrollStrategy::Top };
        self.selected = index;
        self.scroll.scroll_to_item(index, strategy);
    }
}
