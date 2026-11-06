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
    pub fn keep(&self, text: &str) -> Result<bool> {
        if self.list().first().is_some_and(|newest| self.read(*newest).is_ok_and(|kept| kept == text)) { return Ok(false); }
        std::fs::create_dir_all(&self.dir)?;
        let time = now();
        write_atomic(&self.file(time).unwrap_or_else(|| self.new_file(time)), text.as_bytes())?;
        self.prune(time);
        Ok(true)
    }

    /// As `keep`, at most once per `INTERVAL`, for use while writing.
    pub fn keep_paced(&self, text: &str) -> Result<bool> {
        if self.list().first().is_some_and(|last| now().saturating_sub(*last) < INTERVAL) { return Ok(false); }
        self.keep(text)
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

    pub(crate) fn move_to(&self, other: &Versions) -> std::io::Result<()> {
        if !self.dir.exists() { return Ok(()); }
        if let Some(parent) = other.dir.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::rename(&self.dir, &other.dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("thesio-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// Versions of `a.typ` holding these (time, text) files, as earlier runs left them.
    fn seeded(name: &str, kept: &[(u64, &str)]) -> Versions {
        let versions = Versions::of(&root(name), "a.typ");
        std::fs::create_dir_all(&versions.dir).unwrap();
        for (time, text) in kept { std::fs::write(versions.dir.join(format!("{time}.typ")), text).unwrap(); }
        versions
    }

    #[test]
    fn an_unchanged_document_adds_no_version() {
        let versions = seeded("same", &[(100, "one")]);
        assert!(!versions.keep("one").unwrap());
        assert_eq!(versions.list(), [100]);
    }

    #[test]
    fn a_changed_document_adds_one_and_the_old_stays_readable() {
        let versions = seeded("changed", &[(now() - 100, "one")]);
        assert!(versions.keep("two").unwrap());
        let times = versions.list();
        assert_eq!(times.len(), 2);
        assert_eq!(versions.read(times[0]).unwrap(), "two");
        assert_eq!(versions.read(times[1]).unwrap(), "one");
    }

    #[test]
    fn old_versions_thin_to_one_a_day() {
        // Aligned to a day boundary, so `now` cannot split the seeded days.
        let today = now() / DAY * DAY;
        let (a, b) = (today - 10 * DAY, today - 9 * DAY);
        let versions = seeded("prune", &[(a, "1"), (a + 60, "2"), (b, "3"), (b + 60, "4")]);
        versions.keep("latest").unwrap();
        let times = versions.list();
        assert_eq!(times[1..], [b + 60, a + 60]);
    }

    #[test]
    fn versions_past_the_daily_limit_go() {
        let versions = seeded("limit", &[(now() - (DAILY_DAYS + 1) * DAY, "old")]);
        versions.keep("latest").unwrap();
        assert_eq!(versions.list().len(), 1);
    }

    #[test]
    fn versions_of_the_last_day_all_stay() {
        let time = now();
        let versions = seeded("recent", &[(time - 200, "1"), (time - 100, "2")]);
        versions.keep("latest").unwrap();
        assert_eq!(versions.list().len(), 3);
    }

    #[test]
    fn writing_inside_the_interval_keeps_nothing() {
        let versions = seeded("paced-inside", &[(now() - INTERVAL + 60, "one")]);
        assert!(!versions.keep_paced("two").unwrap());
    }

    #[test]
    fn writing_past_the_interval_keeps_a_version() {
        let versions = seeded("paced-past", &[(now() - INTERVAL, "one")]);
        assert!(versions.keep_paced("two").unwrap());
    }

    #[test]
    fn versions_survive_a_rename_that_changes_the_extension() {
        let root = root("extension");
        let old = Versions::of(&root, "a.typ");
        std::fs::create_dir_all(&old.dir).unwrap();
        std::fs::write(old.dir.join("100.typ"), "text").unwrap();
        let new = Versions::of(&root, "a.txt");
        old.move_to(&new).unwrap();
        assert_eq!(new.list(), [100]);
        assert_eq!(new.read(100).unwrap(), "text");
    }
}
