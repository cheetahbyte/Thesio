//! Earlier versions of a document, as plain files in `.thesio/history/<id>/`,
//! each named for the second it was taken. Readable and recoverable by hand.

use crate::store::{STATE_DIR, write_atomic};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The least time between versions taken while writing, in seconds.
pub const INTERVAL: u64 = 5 * 60;
const DAY: u64 = 86_400;
/// Every version stays for a day, then one a day for this many days.
const DAILY_DAYS: u64 = 90;
/// Lines of context around a change.
const CONTEXT: usize = 2;
/// Past this many cells a line diff costs too much: show a full replacement.
const MAX_DIFF_CELLS: usize = 4_000_000;

/// The versions in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<u64> {
    let Ok(read) = std::fs::read_dir(dir) else { return vec![] };
    let mut times: Vec<u64> = read.flatten().filter_map(|item| item.path().file_stem()?.to_str()?.parse().ok()).collect();
    times.sort_unstable_by(|a, b| b.cmp(a));
    times
}

/// A version's file, with the extension of the file it is a version of.
fn version_file(dir: &Path, time: u64) -> PathBuf {
    let extension = dir.extension().and_then(|extension| extension.to_str()).unwrap_or("txt");
    dir.join(format!("{time}.{extension}"))
}

pub fn read(dir: &Path, time: u64) -> Result<String> {
    std::fs::read_to_string(version_file(dir, time)).with_context(|| format!("Could not read the version from {time}"))
}

/// Where the versions of a file live. `id` must be a checked vault path.
pub fn dir(root: &Path, id: &str) -> PathBuf {
    root.join(STATE_DIR).join("history").join(id)
}

/// Keeps `text` as a version unless the newest one already holds it.
pub fn save(dir: &Path, text: &str, now: u64) -> Result<bool> {
    let times = list(dir);
    if times.first().is_some_and(|time| read(dir, *time).is_ok_and(|newest| newest == text)) { return Ok(false); }
    std::fs::create_dir_all(dir)?;
    write_atomic(&version_file(dir, now), text.as_bytes())?;
    prune(dir, now);
    Ok(true)
}

/// Thins out old versions. The newest always stays, however old.
fn prune(dir: &Path, now: u64) {
    let mut kept_day = None;
    for (i, time) in list(dir).into_iter().enumerate() {
        let age = now.saturating_sub(time);
        if i == 0 || age < DAY { continue; }
        let day = time / DAY;
        let keep = age < DAILY_DAYS * DAY && kept_day != Some(day);
        if keep { kept_day = Some(day); } else { let _ = std::fs::remove_file(version_file(dir, time)); }
    }
}

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

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("thesio-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn an_unchanged_document_adds_no_version() {
        let dir = folder("same");
        assert!(save(&dir, "one", 100).unwrap());
        assert!(!save(&dir, "one", 500).unwrap());
        assert!(save(&dir, "two", 900).unwrap());
        assert_eq!(list(&dir), [900, 100]);
        assert_eq!(read(&dir, 100).unwrap(), "one");
    }

    #[test]
    fn old_versions_thin_to_one_a_day_and_the_newest_stays() {
        let dir = folder("prune");
        let now = 200 * DAY;
        // Three versions on each of two old days, one recent, one past the limit.
        for (i, time) in [now - 100 * DAY, now - 10 * DAY, now - 10 * DAY + 60, now - 9 * DAY, now - 9 * DAY + 60, now - 100].into_iter().enumerate() {
            save(&dir, &i.to_string(), time).unwrap();
        }
        save(&dir, "latest", now).unwrap();
        assert_eq!(list(&dir), [now, now - 100, now - 9 * DAY + 60, now - 10 * DAY + 60]);
        let lonely = folder("lonely");
        save(&lonely, "old", 0).unwrap();
        prune(&lonely, now);
        assert_eq!(list(&lonely), [0]);
    }

    #[test]
    fn a_diff_shows_changes_with_context_and_gaps() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
        let new = "a\nb\nc\nd\nE\nf\ng\nh\ni\nj";
        let lines = diff(old, new);
        let kinds: Vec<(LineKind, &str)> = lines.iter().map(|line| (line.kind, line.text.as_str())).collect();
        use LineKind::*;
        assert_eq!(kinds, [(Same, "c"), (Same, "d"), (Removed, "e"), (Added, "E"), (Same, "f"), (Same, "g")]);
        let far = diff("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl", "A\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nL");
        assert!(far.iter().any(|line| line.kind == Gap));
        assert_eq!(diff("same\n", "same\n"), []);
        assert_eq!(diff("", "new").iter().map(|line| line.kind).collect::<Vec<_>>(), [Added]);
    }
}
