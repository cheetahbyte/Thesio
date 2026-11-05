//! Preferences: `.thesio/preferences.json` in the vault.

use crate::app::Workspace;
use gpui::*;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use thesio_core::paths::{STATE_DIR, write_atomic};

static PREFS: RwLock<Option<Preferences>> = RwLock::new(None);
/// Why `preferences.json` could not be read. Nothing writes to it meanwhile.
static UNREADABLE: RwLock<Option<String>> = RwLock::new(None);

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Light or dark look: "light", "dark", or empty to follow the system.
    pub appearance: String,
    /// Keep `library.yaml` and the papers out of the sidebar. They stay in the vault.
    pub hide_library_files: bool,
    /// Sent to Unpaywall, which requires one, to find open-access papers.
    pub unpaywall_email: String,
    /// Library column widths by column name, where they were resized.
    pub library_columns: std::collections::BTreeMap<String, f32>,
    /// Keep the Library's details panel closed.
    pub library_details_hidden: bool,
}

impl Default for Preferences {
    fn default() -> Self { Preferences { appearance: String::new(), hide_library_files: true, unpaywall_email: String::new(), library_columns: Default::default(), library_details_hidden: false } }
}


fn path() -> std::path::PathBuf { crate::store::Store::root_dir().join(STATE_DIR).join("preferences.json") }

pub fn load() {
    let read = match std::fs::read(path()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(e) => Err(e.to_string()),
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
    };
    let (preferences, problem) = match read {
        Ok(preferences) => (preferences, None),
        Err(problem) => (Preferences::default(), Some(problem)),
    };
    *UNREADABLE.write().unwrap() = problem;
    apply(preferences);
}

fn apply(p: Preferences) {
    *PREFS.write().unwrap() = Some(p);
}

/// The current preferences (loaded on first use).
pub fn prefs() -> Preferences {
    if PREFS.read().unwrap().is_none() { load(); }
    PREFS.read().unwrap().clone().unwrap_or_default()
}

fn save(p: &Preferences) -> anyhow::Result<()> {
    if let Some(problem) = &*UNREADABLE.read().unwrap() { anyhow::bail!("preferences.json could not be read and was left alone: {problem}"); }
    std::fs::create_dir_all(crate::store::Store::root_dir().join(STATE_DIR))?;
    write_atomic(&path(), &serde_json::to_vec_pretty(p)?)?;
    apply(p.clone());
    Ok(())
}

impl Workspace {
    /// Change preferences and refresh what depends on them.
    pub(crate) fn set_pref(&mut self, f: impl FnOnce(&mut Preferences), cx: &mut Context<Self>) {
        let old = prefs();
        let mut p = old.clone();
        f(&mut p);
        if let Err(e) = save(&p) {
            self.show_toast_kind(crate::ui::ToastKind::Error, &format!("Could not save settings: {e}"), cx);
            return;
        }
        if p.appearance != old.appearance { crate::appearance::apply_appearance(&p.appearance); }
        if crate::appearance::sync_dark(cx) || p.hide_library_files != old.hide_library_files {
            crate::appearance::refresh_all(cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::Preferences;

    #[test]
    fn files_from_older_builds_load() {
        let old: Preferences = serde_json::from_str(r#"{"english":true,"quick_hotkey":false,"appearance":"dark","library_details_hidden":true}"#).unwrap();
        assert_eq!(old.appearance, "dark");
        assert!(old.library_details_hidden);
    }
}
