//! Earlier versions of a file, as plain files in `.thesio/history/<id>/`, each
//! named for the second it was taken. Readable and recoverable by hand.

use crate::paths::{STATE_DIR, write_atomic};
use crate::time::{DAY, now};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The least time between versions taken while writing, in seconds.
const INTERVAL: u64 = 5 * 60;
/// Every version stays for a day, then one a day for this many days.
const DAILY_DAYS: u64 = 90;

/// The versions of one file.
pub struct Versions {
    pub(crate) dir: PathBuf,
}

impl Versions {
    /// `id` must be a checked vault path.
    pub(crate) fn of(root: &Path, id: &str) -> Self {
        Self { dir: root.join(STATE_DIR).join("history").join(id) }
    }

    /// Their times, newest first.
    pub fn list(&self) -> Vec<u64> {
        let Ok(read) = std::fs::read_dir(&self.dir) else { return vec![] };
        let mut times: Vec<u64> = read.flatten().filter_map(|item| item.path().file_stem()?.to_str()?.parse().ok()).collect();
        times.sort_unstable_by(|a, b| b.cmp(a));
        times
    }

    /// The file of the version from `time`. A rename can change the file's
    /// extension, and versions keep the one they were taken with.
    fn file(&self, time: u64) -> Option<PathBuf> {
        let stem = time.to_string();
        std::fs::read_dir(&self.dir).ok()?.flatten().map(|item| item.path()).find(|path| path.file_stem().is_some_and(|name| *name == *stem))
    }

    fn new_file(&self, time: u64) -> PathBuf {
        let extension = self.dir.extension().and_then(|extension| extension.to_str()).unwrap_or("txt");
        self.dir.join(format!("{time}.{extension}"))
    }

    pub fn read(&self, time: u64) -> Result<String> {
        let file = self.file(time).with_context(|| format!("Could not find the version from {time}"))?;
        std::fs::read_to_string(file).with_context(|| format!("Could not read the version from {time}"))
    }

    /// Keeps `text` unless the newest version already holds it.
    pub fn keep(&self, text: &str) -> Result<bool> { self.keep_at(text, now()) }

    /// As `keep`, at most once per `INTERVAL`, for use while writing.
    pub fn keep_paced(&self, text: &str) -> Result<bool> { self.keep_paced_at(text, now()) }

    fn keep_paced_at(&self, text: &str, time: u64) -> Result<bool> {
        if self.list().first().is_some_and(|last| time.saturating_sub(*last) < INTERVAL) { return Ok(false); }
        self.keep_at(text, time)
    }

    fn keep_at(&self, text: &str, time: u64) -> Result<bool> {
        if self.list().first().is_some_and(|newest| self.read(*newest).is_ok_and(|kept| kept == text)) { return Ok(false); }
        std::fs::create_dir_all(&self.dir)?;
        write_atomic(&self.file(time).unwrap_or_else(|| self.new_file(time)), text.as_bytes())?;
        self.prune(time);
        Ok(true)
    }

    /// Thins out old versions. The newest always stays, however old.
    fn prune(&self, now: u64) {
        let mut kept_day = None;
        for (i, time) in self.list().into_iter().enumerate() {
            let age = now.saturating_sub(time);
            if i == 0 || age < DAY { continue; }
            let day = time / DAY;
            let keep = age < DAILY_DAYS * DAY && kept_day != Some(day);
            if keep { kept_day = Some(day); } else if let Some(file) = self.file(time) { let _ = std::fs::remove_file(file); }
        }
    }

    /// Moves them to follow their file.
    pub(crate) fn move_to(&self, other: &Versions) -> std::io::Result<()> {
        if !self.dir.exists() { return Ok(()); }
        if let Some(parent) = other.dir.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::rename(&self.dir, &other.dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(name: &str, id: &str) -> Versions {
        let root = std::env::temp_dir().join(format!("thesio-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Versions::of(&root, id)
    }

    #[test]
    fn an_unchanged_document_adds_no_version() {
        let versions = versions("same", "a.typ");
        assert!(versions.keep_at("one", 100).unwrap());
        assert!(!versions.keep_at("one", 500).unwrap());
        assert!(versions.keep_at("two", 900).unwrap());
        assert_eq!(versions.list(), [900, 100]);
        assert_eq!(versions.read(100).unwrap(), "one");
    }

    #[test]
    fn old_versions_thin_to_one_a_day_and_the_newest_stays() {
        let kept = versions("prune", "a.typ");
        let now = 200 * DAY;
        // Three versions on each of two old days, one recent, one past the limit.
        for (i, time) in [now - 100 * DAY, now - 10 * DAY, now - 10 * DAY + 60, now - 9 * DAY, now - 9 * DAY + 60, now - 100].into_iter().enumerate() {
            kept.keep_at(&i.to_string(), time).unwrap();
        }
        kept.keep_at("latest", now).unwrap();
        assert_eq!(kept.list(), [now, now - 100, now - 9 * DAY + 60, now - 10 * DAY + 60]);
        let lonely = versions("lonely", "a.typ");
        lonely.keep_at("old", 0).unwrap();
        lonely.prune(now);
        assert_eq!(lonely.list(), [0]);
    }

    #[test]
    fn writing_keeps_at_most_one_version_per_interval() {
        let versions = versions("paced", "a.typ");
        assert!(versions.keep_paced_at("one", 1000).unwrap());
        assert!(!versions.keep_paced_at("two", 1000 + INTERVAL - 1).unwrap());
        assert!(versions.keep_paced_at("three", 1000 + INTERVAL).unwrap());
        assert_eq!(versions.list(), [1000 + INTERVAL, 1000]);
    }

    #[test]
    fn versions_survive_a_rename_that_changes_the_extension() {
        let old = versions("extension", "a.typ");
        old.keep_at("text", 100).unwrap();
        let root = old.dir.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
        let new = Versions::of(&root, "a.txt");
        old.move_to(&new).unwrap();
        assert_eq!(new.list(), [100]);
        assert_eq!(new.read(100).unwrap(), "text");
    }
}
