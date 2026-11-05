//! What changed between two versions of a text, by line.

/// Lines of context around a change.
const CONTEXT: usize = 2;
/// Past this many cells a line diff costs too much: show a full replacement.
const MAX_DIFF_CELLS: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind { Same, Added, Removed, Gap }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: String,
}

/// What changed by line from `old` to `new`, with some context. Empty when equal.
pub fn diff(old: &str, new: &str) -> Vec<DiffLine> {
    let (old, new): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let start = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - start;
    let end = old.iter().rev().zip(new.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let (a, b) = (&old[start..old.len() - end], &new[start..new.len() - end]);
    if a.is_empty() && b.is_empty() { return vec![]; }
    let line = |kind, text: &str| DiffLine { kind, text: text.to_string() };
    let mut all: Vec<DiffLine> = old[..start].iter().map(|text| line(LineKind::Same, text)).collect();
    if a.len() * b.len() > MAX_DIFF_CELLS {
        all.extend(a.iter().map(|text| line(LineKind::Removed, text)));
        all.extend(b.iter().map(|text| line(LineKind::Added, text)));
    } else {
        // Longest common subsequence, walked from the front.
        let mut table = vec![vec![0u32; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                table[i][j] = if a[i] == b[j] { table[i + 1][j + 1] + 1 } else { table[i + 1][j].max(table[i][j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                all.push(line(LineKind::Same, a[i]));
                (i, j) = (i + 1, j + 1);
            } else if i < a.len() && (j == b.len() || table[i + 1][j] >= table[i][j + 1]) {
                all.push(line(LineKind::Removed, a[i]));
                i += 1;
            } else {
                all.push(line(LineKind::Added, b[j]));
                j += 1;
            }
        }
    }
    all.extend(old[old.len() - end..].iter().map(|text| line(LineKind::Same, text)));
    let near = |i: usize| all[i.saturating_sub(CONTEXT)..(i + CONTEXT + 1).min(all.len())].iter().any(|line| line.kind != LineKind::Same);
    let mut out = vec![];
    for i in 0..all.len() {
        if near(i) { out.push(all[i].clone()); } else if out.last().is_some_and(|line: &DiffLine| line.kind != LineKind::Gap) { out.push(line(LineKind::Gap, "")); }
    }
    if out.last().is_some_and(|line| line.kind == LineKind::Gap) { out.pop(); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_shows_with_its_context() {
        let lines = diff("a\nb\nc\nd\ne\nf\ng\nh\ni\nj", "a\nb\nc\nd\nE\nf\ng\nh\ni\nj");
        let kinds: Vec<(LineKind, &str)> = lines.iter().map(|line| (line.kind, line.text.as_str())).collect();
        use LineKind::*;
        assert_eq!(kinds, [(Same, "c"), (Same, "d"), (Removed, "e"), (Added, "E"), (Same, "f"), (Same, "g")]);
    }

    #[test]
    fn distant_changes_have_a_gap_between_them() {
        let far = diff("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl", "A\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nL");
        assert!(far.iter().any(|line| line.kind == LineKind::Gap));
    }

    #[test]
    fn equal_texts_have_no_lines() {
        assert_eq!(diff("same\n", "same\n"), []);
    }

    #[test]
    fn everything_is_added_to_an_empty_text() {
        assert_eq!(diff("", "new").iter().map(|line| line.kind).collect::<Vec<_>>(), [LineKind::Added]);
    }
}
