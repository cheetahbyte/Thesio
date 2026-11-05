//! The source text being edited: selection, line index, and undo as a list of
//! edits. Offsets are bytes on char boundaries.

use std::ops::Range;
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

const COALESCE: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind { Typing, Deleting, Other }

#[derive(Clone, Debug)]
struct Edit {
    at: usize,
    old: String,
    new: String,
    sel_before: Range<usize>,
    sel_after: Range<usize>,
}

/// What an edit did to the text, for whatever keeps a copy by line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The bytes replaced, as they were before.
    pub range: Range<usize>,
    /// The length of what replaced them.
    pub new_len: usize,
    /// The lines replaced, as they were before.
    pub lines: Range<usize>,
    /// How many lines replace them.
    pub new_lines: usize,
}

pub struct Buffer {
    text: String,
    line_starts: Vec<usize>,
    pub sel: Range<usize>,
    /// The caret sits at `sel.start` rather than `sel.end`.
    pub reversed: bool,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    last: Option<(EditKind, Instant)>,
}

impl Default for Buffer {
    fn default() -> Self { Self::new("") }
}

impl Buffer {
    pub fn new(text: impl Into<String>) -> Self {
        let mut buffer = Self { text: text.into(), line_starts: vec![], sel: 0..0, reversed: false, undo: vec![], redo: vec![], last: None };
        buffer.index_lines();
        buffer
    }

    pub fn text(&self) -> &str { &self.text }
    pub fn caret(&self) -> usize { if self.reversed { self.sel.start } else { self.sel.end } }
    pub fn line_count(&self) -> usize { self.line_starts.len() }

    fn index_lines(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        self.line_starts.extend(self.text.match_indices('\n').map(|(i, _)| i + 1));
    }

    /// The line's bytes, without its newline.
    pub fn line_range(&self, line: usize) -> Range<usize> {
        let start = self.line_starts[line];
        let end = self.line_starts.get(line + 1).map_or(self.text.len(), |next| next - 1);
        start..end
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= offset) - 1
    }

    pub fn move_to(&mut self, offset: usize) {
        self.sel = offset..offset;
        self.reversed = false;
        self.last = None;
    }

    pub fn select_to(&mut self, offset: usize) {
        let anchor = if self.reversed { self.sel.end } else { self.sel.start };
        self.reversed = offset < anchor;
        self.sel = anchor.min(offset)..anchor.max(offset);
        self.last = None;
    }

    pub fn select_all(&mut self) {
        self.sel = 0..self.text.len();
        self.reversed = false;
        self.last = None;
    }

    pub fn selected(&self) -> &str { &self.text[self.sel.clone()] }

    /// Replaces `range`, leaving the caret after the new text.
    pub fn replace(&mut self, range: Range<usize>, new: &str, kind: EditKind) -> Change {
        let sel_after = range.start + new.len()..range.start + new.len();
        self.apply(range, new, sel_after, kind)
    }

    /// Replaces `range` and selects `sel` afterwards (input-method composition).
    pub fn replace_selecting(&mut self, range: Range<usize>, new: &str, sel: Range<usize>, kind: EditKind) -> Change {
        self.apply(range, new, sel, kind)
    }

    fn splice(&mut self, range: Range<usize>, new: &str) -> Change {
        let first = self.line_of(range.start);
        let lines = first..self.line_of(range.end) + 1;
        self.text.replace_range(range.clone(), new);
        self.index_lines();
        let new_lines = self.line_of(range.start + new.len()) + 1 - first;
        Change { range, new_len: new.len(), lines, new_lines }
    }

    fn apply(&mut self, range: Range<usize>, new: &str, sel_after: Range<usize>, kind: EditKind) -> Change {
        let now = Instant::now();
        let old = self.text[range.clone()].to_string();
        let sel_before = self.sel.clone();
        let change = self.splice(range.clone(), new);
        self.sel = sel_after.clone();
        self.reversed = false;
        self.redo.clear();
        let recent = self.last.is_some_and(|(k, t)| k == kind && kind != EditKind::Other && now - t < COALESCE);
        self.last = Some((kind, now));
        if recent && let Some(top) = self.undo.last_mut() {
            if kind == EditKind::Typing && top.at + top.new.len() == range.start && old.is_empty() {
                top.new.push_str(new);
                top.sel_after = sel_after;
                return change;
            }
            if kind == EditKind::Deleting && top.new.is_empty() && range.end == top.at && new.is_empty() {
                top.old.insert_str(0, &old);
                top.at = range.start;
                top.sel_after = sel_after;
                return change;
            }
        }
        self.undo.push(Edit { at: range.start, old, new: new.to_string(), sel_before, sel_after });
        change
    }

    /// Replaces text outside the undo history, for text derived from the
    /// document rather than edited in it.
    pub fn replace_silently(&mut self, range: Range<usize>, new: &str, sel: Range<usize>) -> Change {
        let change = self.splice(range, new);
        self.sel = sel;
        self.reversed = false;
        self.last = None;
        change
    }

    pub fn undo(&mut self) -> Option<Change> { self.step(true) }
    pub fn redo(&mut self) -> Option<Change> { self.step(false) }

    fn step(&mut self, back: bool) -> Option<Change> {
        while let Some(edit) = if back { self.undo.pop() } else { self.redo.pop() } {
            let (remove, insert, sel) = if back { (&edit.new, &edit.old, &edit.sel_before) } else { (&edit.old, &edit.new, &edit.sel_after) };
            // A silent replacement can overwrite an edit. That one is gone,
            // and the step goes to the edit before it.
            if self.text.get(edit.at..edit.at + remove.len()) != Some(remove.as_str()) { continue; }
            let change = self.splice(edit.at..edit.at + remove.len(), insert);
            self.sel = sel.clone();
            self.reversed = false;
            self.last = None;
            if back { self.redo.push(edit); } else { self.undo.push(edit); }
            return Some(change);
        }
        None
    }

    pub fn prev_grapheme(&self, offset: usize) -> usize {
        self.text[..offset].grapheme_indices(true).next_back().map_or(0, |(i, _)| i)
    }

    pub fn next_grapheme(&self, offset: usize) -> usize {
        self.text[offset..].graphemes(true).next().map_or(offset, |g| offset + g.len())
    }

    pub fn prev_word(&self, offset: usize) -> usize {
        self.text[..offset].unicode_word_indices().next_back().map_or(0, |(i, _)| i)
    }

    pub fn next_word(&self, offset: usize) -> usize {
        self.text[offset..].unicode_word_indices().next().map_or(self.text.len(), |(i, w)| offset + i + w.len())
    }

    /// The word around `offset`, for double clicks.
    pub fn word_at(&self, offset: usize) -> Range<usize> {
        self.text.split_word_bound_indices().map(|(i, w)| i..i + w.len()).find(|r| r.contains(&offset)).unwrap_or(offset..offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_overwritten_silently_does_not_cost_an_undo() {
        let mut buffer = Buffer::new("text\nblock");
        buffer.replace(0..0, "more ", EditKind::Other);
        buffer.replace(10..15, "BLOCK", EditKind::Other);
        buffer.replace_silently(10..15, "fresh", 0..0);
        assert!(buffer.undo().is_some());
        assert_eq!(buffer.text(), "text\nfresh");
        assert!(buffer.undo().is_none());
    }

    #[test]
    fn a_default_buffer_has_one_line() {
        assert_eq!(Buffer::default().line_count(), 1);
    }
}
