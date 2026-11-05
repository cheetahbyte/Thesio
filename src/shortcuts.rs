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
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
}

#[cfg(target_os = "windows")]
fn windows_binding(binding: KeyBinding) -> Option<KeyBinding> {
    let keys = binding.keystrokes().iter().map(|k| k.unparse().replace("win-", "cmd-")).collect::<Vec<_>>().join(" ");
    // Cocoa's control-key navigation conflicts with Windows editing shortcuts.
    if matches!(keys.as_str(), "ctrl-a" | "ctrl-b" | "ctrl-d" | "ctrl-e" | "ctrl-f" | "ctrl-h" | "ctrl-n" | "ctrl-p" | "ctrl-backspace") { return None; }
    let keys = keys.split(' ').map(|key| match key {
        "cmd-left" => "home".into(), "cmd-right" => "end".into(),
        "cmd-shift-left" => "shift-home".into(), "cmd-shift-right" => "shift-end".into(),
        "cmd-up" => "ctrl-home".into(), "cmd-down" => "ctrl-end".into(),
        "cmd-shift-up" => "ctrl-shift-home".into(), "cmd-shift-down" => "ctrl-shift-end".into(),
        "alt-left" | "alt-right" | "alt-shift-left" | "alt-shift-right" | "alt-backspace" | "alt-delete" => key.replacen("alt-", "ctrl-", 1),
        _ => key.replace("cmd-", "ctrl-"),
    }).collect::<Vec<String>>().join(" ");
    KeyBinding::load(&keys, binding.action().boxed_clone(), binding.predicate(), false, binding.action_input(), &DummyKeyboardMapper).ok()
}

fn key_name(key: &str) -> String {
    if key.chars().count() == 1 { return key.to_uppercase(); }
    let mut chars = key.chars();
    chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

fn stroke_name(stroke: &Keystroke, separator: &str) -> String {
    let m = stroke.modifiers;
    let held = [(m.platform, "⌘"), (m.alt, "⌥"), (m.shift, "Shift"), (m.control, "Control")].into_iter().filter(|(on, _)| *on).map(|(_, name)| name.to_string());
    held.chain([key_name(&stroke.key)]).collect::<Vec<_>>().join(separator)
}

fn keys(action: &dyn Action, separator: &str, cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).next()?;
    Some(binding.keystrokes().iter().map(|stroke| stroke_name(stroke.inner(), separator)).collect::<Vec<_>>().join(", "))
}

/// The key the keymap holds for `action`, as the UI writes it; None when it has none.
pub fn hint(action: &dyn Action, cx: &App) -> Option<String> { keys(action, " + ", cx) }

/// `hint` for a tight space.
pub fn compact_hint(action: &dyn Action, cx: &App) -> Option<String> { keys(action, " ", cx) }
