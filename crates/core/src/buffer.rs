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

/// Where two texts differ, on char boundaries of both: the bytes of `old`, and
/// the length of what replaced them.
pub fn difference(old: &str, new: &str) -> (Range<usize>, usize) {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) { start -= 1; }
    let longest = old.len().min(new.len()) - start;
    let mut tail = old.bytes().rev().zip(new.bytes().rev()).take(longest).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - tail) || !new.is_char_boundary(new.len() - tail) { tail -= 1; }
    (start..old.len() - tail, new.len() - tail - start)
}

/// Moves `range` with an edit around it, which replaced the bytes `edit` with
/// `new_len` others. False when the edit reached into it.
pub fn follow(range: &mut Range<usize>, edit: &Range<usize>, new_len: usize) -> bool {
    if range.end <= edit.start { return true; }
    if range.start < edit.end { return false; }
    *range = range.start + new_len - edit.len()..range.end + new_len - edit.len();
    true
}

/// `follow` for a range that describes text rather than holds it: an edit
/// wholly inside resizes it, unless nothing is left.
pub fn stretch(range: &mut Range<usize>, edit: &Range<usize>, new_len: usize) -> bool {
    let beside = range.end <= edit.start || edit.end <= range.start;
    if beside || edit.start < range.start || range.end < edit.end { return follow(range, edit, new_len); }
    range.end = range.end + new_len - edit.len();
    range.start < range.end
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

    /// `offset` in the text and on a char boundary, whatever text it came from.
    pub fn snapped(&self, offset: usize) -> usize { self.text.floor_char_boundary(offset) }

    /// `range`, or the text beside it when it covers nothing that can be
    /// underlined: Typst puts a missing token between two characters.
    pub fn visible(&self, range: Range<usize>) -> Range<usize> {
        if !self.text[range.clone()].trim_matches(['\r', '\n']).is_empty() { return range; }
        let at = range.start;
        let line = self.line_range(self.line_of(at));
        if !self.text[at..line.end].trim_end().is_empty() { return at..self.next_grapheme(at); }
        let end = self.text[..at].trim_end().len();
        self.prev_grapheme(end)..end
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
    fn the_difference_of_two_texts_never_splits_a_character() {
        let cases = [("a€b", "a₭b", 1..4, 3), ("£", "㣣", 0..2, 3), ("same", "same", 4..4, 0), ("ab", "aXb", 1..1, 1), ("aXb", "ab", 1..2, 0), ("aa", "a", 1..2, 0), ("", "é", 0..0, 2)];
        for (old, new, edit, new_len) in cases {
            assert_eq!(difference(old, new), (edit, new_len), "{old} to {new}");
        }
    }

    #[test]
    fn a_range_follows_edits_outside_it_and_ends_at_one_reaching_in() {
        let moved = |edit: Range<usize>, new_len| {
            let mut range = 4..8;
            follow(&mut range, &edit, new_len).then_some(range)
        };
        assert_eq!(moved(8..8, 3), Some(4..8), "after it");
        assert_eq!(moved(4..4, 3), Some(7..11), "at its start");
        assert_eq!(moved(1..3, 0), Some(2..6), "before it");
        assert_eq!(moved(5..6, 1), None);
        assert_eq!(moved(2..5, 3), None);
    }

    #[test]
    fn a_stretching_range_grows_and_shrinks_with_edits_inside_it() {
        let moved = |edit: Range<usize>, new_len| {
            let mut range = 4..8;
            stretch(&mut range, &edit, new_len).then_some(range)
        };
        assert_eq!(moved(5..5, 3), Some(4..11));
        assert_eq!(moved(7..8, 0), Some(4..7), "its last character");
        assert_eq!(moved(4..5, 0), Some(4..7), "its first character");
        assert_eq!(moved(8..8, 3), Some(4..8), "typing after it");
        assert_eq!(moved(4..4, 3), Some(7..11), "typing before it");
        assert_eq!(moved(4..8, 0), None, "all of it");
        assert_eq!(moved(2..5, 3), None, "across its edge");
        assert_eq!(moved(7..9, 1), None, "across its edge");
    }

    #[test]
    fn an_offset_from_older_text_lands_on_a_character() {
        let buffer = Buffer::new("aé");
        assert_eq!(buffer.snapped(2), 1);
        assert_eq!(buffer.snapped(3), 3);
        assert_eq!(buffer.snapped(99), 3);
    }

    #[test]
    fn what_covers_nothing_is_shown_on_the_text_beside_it() {
        let visible = |text: &str, range: Range<usize>| {
            let buffer = Buffer::new(text);
            let shown = buffer.visible(range);
            buffer.text()[shown].to_string()
        };
        assert_eq!(visible("#let x", 1..4), "let", "a range with text stays");
        assert_eq!(visible("ab cd", 3..3), "c", "the next character");
        assert_eq!(visible("e\u{301}x", 0..0), "e\u{301}", "a whole grapheme");
        assert_eq!(visible("ab\ncd", 2..2), "b", "at a line's end");
        assert_eq!(visible("ab\ncd", 2..3), "b", "only a newline");
        assert_eq!(visible("ab\r\ncd", 2..2), "b", "before a carriage return");
        assert_eq!(visible("ab  \n\n", 6..6), "b", "past blank lines at the end");
        assert_eq!(visible("日本", 6..6), "本", "at the end of the text");
        assert_eq!(visible("\n\n", 1..1), "", "nothing to show");
        assert_eq!(visible("", 0..0), "");
    }

    #[test]
    fn a_default_buffer_has_one_line() {
        assert_eq!(Buffer::default().line_count(), 1);
    }
}
