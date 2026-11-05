use gpui::*;
use std::collections::BTreeMap;

fn path() -> std::path::PathBuf { crate::store::Store::root_dir().join(thesio_core::paths::STATE_DIR).join("shortcuts.json") }
/// Applies `.thesio/shortcuts.json` over the built-in keys. An empty key unbinds an action.
pub fn init(cx: &mut App) {
    let base = cx.key_bindings().borrow().bindings().cloned().collect::<Vec<_>>();
    #[cfg(target_os = "windows")]
    let base: Vec<KeyBinding> = base.into_iter().filter_map(windows_binding).collect();
    let custom: BTreeMap<String, String> = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let bindings: Vec<_> = base.iter().filter(|b| !custom.get(b.action().name()).is_some_and(|key| key.is_empty())).map(|b| {
        custom.get(b.action().name()).and_then(|key| KeyBinding::load(key, b.action().boxed_clone(), b.predicate(), false, b.action_input(), &DummyKeyboardMapper).ok()).unwrap_or_else(|| b.clone())
    }).collect();
    *CUSTOM.write().unwrap() = custom;
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
}

#[cfg(target_os = "windows")]
fn windows_binding(binding: KeyBinding) -> Option<KeyBinding> {
    let keys = binding.keystrokes().iter().map(|k| k.unparse().replace("win-", "cmd-")).collect::<Vec<_>>().join(" ");
    // Cocoa's control-key navigation conflicts with Windows editing shortcuts.
    if matches!(keys.as_str(), "ctrl-a" | "ctrl-b" | "ctrl-d" | "ctrl-e" | "ctrl-f" | "ctrl-h" | "ctrl-n" | "ctrl-p" | "ctrl-backspace") { return None; }
    let keys = if binding.action().name().ends_with("::InlineCode") { "ctrl-shift-c".to_string() } else { keys.split(' ').map(|key| match key {
        "cmd-left" => "home".into(), "cmd-right" => "end".into(),
        "cmd-shift-left" => "shift-home".into(), "cmd-shift-right" => "shift-end".into(),
        "cmd-up" => "ctrl-home".into(), "cmd-down" => "ctrl-end".into(),
        "cmd-shift-up" => "ctrl-shift-home".into(), "cmd-shift-down" => "ctrl-shift-end".into(),
        "alt-left" | "alt-right" | "alt-shift-left" | "alt-shift-right" | "alt-backspace" | "alt-delete" => key.replacen("alt-", "ctrl-", 1),
        _ => key.replace("cmd-", "ctrl-"),
    }).collect::<Vec<String>>().join(" ") };
    KeyBinding::load(&keys, binding.action().boxed_clone(), binding.predicate(), false, binding.action_input(), &DummyKeyboardMapper).ok()
}

/// The user's keys, for `hint` (which runs where no `App` is at hand).
static CUSTOM: std::sync::RwLock<BTreeMap<String, String>> = std::sync::RwLock::new(BTreeMap::new());
static HINTS: std::sync::Mutex<BTreeMap<String, &'static str>> = std::sync::Mutex::new(BTreeMap::new());

/// The key hint the UI shows for `action`: `default` (its built-in key,
/// written as the UI writes it) unless `shortcuts.json` changes it.
pub fn hint(action: &str, default: &'static str) -> &'static str {
    let Some(key) = CUSTOM.read().unwrap().get(action).cloned() else { return default };
    let mut cache = HINTS.lock().unwrap();
    if let Some(h) = cache.get(&key) { return h }
    let text = key.split(' ').map(|stroke| {
        let Ok(k) = Keystroke::parse(stroke) else { return stroke.to_string() };
        let m = k.modifiers;
        let mut parts: Vec<String> = [(m.platform, "⌘"), (m.alt, "⌥"), (m.shift, "Shift"), (m.control, "Control")].into_iter().filter(|(on, _)| *on).map(|(_, s)| s.to_string()).collect();
        parts.push(match k.key.as_str() { "space" => "Space".into(), "enter" => "Enter".into(), k if k.chars().count() == 1 => k.to_uppercase(), k => { let mut c = k.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() } });
        parts.join(" + ")
    }).collect::<Vec<_>>().join(", ");
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    cache.insert(key, leaked);
    leaked
}
