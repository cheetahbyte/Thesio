//! Where two versions of a text differ, and ranges carried across that edit.
//! Offsets are bytes on char boundaries.

use std::ops::Range;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
