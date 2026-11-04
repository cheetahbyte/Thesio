use gpui::*;
use gpui::prelude::FluentBuilder;
use std::collections::BTreeMap;
use crate::{app::Workspace, theme::Theme};

pub struct Shortcuts { base: Vec<KeyBinding>, custom: BTreeMap<String, String> }
impl Global for Shortcuts {}
fn path() -> std::path::PathBuf { crate::store::Store::root_dir().join("shortcuts.json") }
pub fn init(cx: &mut App) {
    let base = cx.key_bindings().borrow().bindings().cloned().collect::<Vec<_>>();
    #[cfg(target_os = "windows")]
    let base = base.into_iter().filter_map(windows_binding).collect();
    let custom = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    cx.set_global(Shortcuts { base, custom });
    rebuild(cx);
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

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::windows_binding;
    use gpui::KeyBinding;
    #[test]
    fn copy_and_inline_code_keep_distinct_windows_shortcuts() {
        use crate::editor::keymap::{Copy, InlineCode};
        let copy = windows_binding(KeyBinding::new("cmd-c", Copy, Some("Editor"))).unwrap();
        let code = windows_binding(KeyBinding::new("cmd-ctrl-c", InlineCode, Some("Editor"))).unwrap();
        assert_eq!(copy.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>(), ["ctrl-c"]);
        assert_eq!(code.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>(), ["ctrl-shift-c"]);
        assert_ne!(copy.keystrokes(), code.keystrokes());
    }
}
/// The user's keys, for `hint` (which runs where no `App` is at hand).
static CUSTOM: std::sync::RwLock<BTreeMap<String, String>> = std::sync::RwLock::new(BTreeMap::new());
static HINTS: std::sync::Mutex<BTreeMap<String, &'static str>> = std::sync::Mutex::new(BTreeMap::new());

/// The key hint the UI shows for `action`: `default` (its built-in key,
/// written as the UI writes it) unless the user changed it in 设置.
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

pub fn rebuild(cx: &mut App) {
    *CUSTOM.write().unwrap() = cx.global::<Shortcuts>().custom.clone();
    let s = cx.global::<Shortcuts>();
    let bindings: Vec<_> = s.base.iter().filter(|b| !s.custom.get(b.action().name()).is_some_and(|key| key.is_empty())).map(|b| {
        s.custom.get(b.action().name()).and_then(|key| KeyBinding::load(key, b.action().boxed_clone(), b.predicate(), false, b.action_input(), &DummyKeyboardMapper).ok()).unwrap_or_else(|| b.clone())
    }).collect();
    cx.clear_key_bindings(); cx.bind_keys(bindings);
}

/// Settings groups, in order.
const GROUPS: [(&str, &str); 5] = [("应用与窗口", "App & windows"), ("文档", "Documents"), ("编辑", "Editing"), ("文字样式", "Text styles"), ("段落与块", "Paragraphs & blocks")];

/// (group, 中文, English) of the actions shown in Settings.
fn info(name: &str) -> Option<(usize, &'static str, &'static str)> {
    Some(match name.rsplit("::").next().unwrap_or(name) {
        "NewDoc" => (0, "新建文档", "New document"),
        "CloseWindow" => (0, "关闭窗口", "Close window"),
        "HideApp" => (0, "隐藏 Thesio", "Hide Thesio"),
        "Minimize" => (0, "最小化窗口", "Minimize window"),
        "ToggleFullScreen" => (0, "进入 / 退出全屏", "Enter / exit full screen"),
        "OpenSettings" => (0, "打开设置", "Open Settings"),
        "GoHome" => (0, "返回主页", "Go to Home"),
        "ToggleSidebar" => (0, "展开 / 收起侧栏", "Show / hide sidebar"),
        "FocusSearch" => (0, "搜索文档", "Search documents"),
        "ToggleShortcuts" => (0, "查看快捷键", "Show shortcuts"),
        "ImportDoc" => (1, "导入文档", "Import documents"),
        "Save" => (1, "保存文档", "Save document"),
        "EditMode" => (1, "编辑模式", "Edit mode"),
        "ReadMode" => (1, "阅读模式", "Reading mode"),
        "FindInDoc" => (1, "文档内查找", "Find in document"),
        "ReplaceInDoc" => (1, "查找和替换", "Find and replace"),
        "Undo" => (2, "撤销", "Undo"),
        "Redo" => (2, "重做", "Redo"),
        "Copy" => (2, "复制", "Copy"),
        "Cut" => (2, "剪切", "Cut"),
        "Paste" => (2, "粘贴", "Paste"),
        "SelectAll" => (2, "全选", "Select all"),
        "PastePlain" => (2, "粘贴为纯文本", "Paste as plain text"),
        "Link" => (2, "插入链接", "Insert link"),
        "InsertReminder" => (2, "插入提醒", "Insert reminder"),
        "MoveBlockUp" => (2, "上移当前块", "Move block up"),
        "MoveBlockDown" => (2, "下移当前块", "Move block down"),
        "ToggleTodo" => (2, "切换任务完成状态", "Check / uncheck to-do"),
        "Bold" => (3, "加粗", "Bold"),
        "Italic" => (3, "斜体", "Italic"),
        "Underline" => (3, "下划线", "Underline"),
        "Strike" => (3, "删除线", "Strikethrough"),
        "InlineCode" => (3, "行内代码", "Inline code"),
        "ApplyLastColor" => (3, "应用上次的文字色和背景色", "Apply last colors"),
        "TurnText" => (4, "正文", "Text"),
        "TurnH1" => (4, "1 级标题", "Heading 1"),
        "TurnH2" => (4, "2 级标题", "Heading 2"),
        "TurnH3" => (4, "3 级标题", "Heading 3"),
        "TurnH4" => (4, "4 级标题", "Heading 4"),
        "TurnH5" => (4, "5 级标题", "Heading 5"),
        "TurnH6" => (4, "6 级标题", "Heading 6"),
        "TurnH7" => (4, "7 级标题", "Heading 7"),
        "TurnH8" => (4, "8 级标题", "Heading 8"),
        "TurnH9" => (4, "9 级标题", "Heading 9"),
        "TurnOrdered" => (4, "有序列表", "Numbered list"),
        "TurnBullet" => (4, "无序列表", "Bulleted list"),
        "TurnTodo" => (4, "任务列表", "To-do list"),
        "TurnCode" => (4, "代码块", "Code block"),
        "TurnQuote" => (4, "引用", "Quote"),
        "InsertDivider" => (4, "分隔线", "Divider"),
        "InsertTable" => (4, "插入表格", "Insert table"),
        "AlignLeft" => (4, "左对齐", "Align left"),
        "AlignCenter" => (4, "居中对齐", "Align center"),
        "AlignRight" => (4, "右对齐", "Align right"),
        _ => return None,
    })
}
pub fn label(name: &str) -> Option<&'static str> { info(name).map(|(_, _, en)| en) }
fn default_key(s: &Shortcuts, id: &str) -> String {
    if id.ends_with("::NewDoc") { return if cfg!(target_os = "windows") { "ctrl-n" } else { "cmd-n" }.into(); }
    s.base.iter().find(|b| b.action().name() == id).map(|b| b.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>().join(" ")).unwrap_or_default()
}
/// The key an action runs on now.
pub fn key_of(cx: &App, id: &str) -> String {
    cx.try_global::<Shortcuts>().map(|s| s.custom.get(id).cloned().unwrap_or_else(|| default_key(s, id))).unwrap_or_default()
}
/// (action, label, key) of the shortcuts the user can change, in settings order.
pub fn rows(cx: &App) -> Vec<(String, String, String)> {
    let Some(s) = cx.try_global::<Shortcuts>() else { return vec![] };
    let mut rows: Vec<(usize, String, String, String)> = vec![];
    for b in &s.base {
        let name = b.action().name();
        if let Some(title) = label(name) {
            if rows.iter().any(|(_, id, _, _)| id == name) { continue; }
            rows.push((info(name).unwrap().0, name.into(), title.into(), key_of(cx, name)));
        }
    }
    rows.sort_by_key(|r| r.0);
    rows.into_iter().map(|(_, id, t, k)| (id, t, k)).collect()
}
pub fn pretty(key: &str) -> String {
    if key.is_empty() { return "Not set".into(); }
    key.split(' ').map(|stroke| {
        let Ok(ks) = Keystroke::parse(stroke) else { return stroke.to_string() };
        let m = ks.modifiers;
        let mut out: Vec<String> = [(m.control, "⌃"), (m.alt, "⌥"), (m.shift, "⇧"), (m.platform, "⌘")].into_iter().filter(|(on, _)| *on).map(|(_, s)| s.to_string()).collect();
        out.push(match ks.key.as_str() {
            "space" => "Space".into(), "enter" => "↩".into(), "backspace" => "⌫".into(), "delete" => "⌦".into(), "tab" => "⇥".into(), "escape" => "Esc".into(),
            "up" => "↑".into(), "down" => "↓".into(), "left" => "←".into(), "right" => "→".into(),
            k if k.chars().count() == 1 => k.to_uppercase(),
            k => { let mut c = k.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }
        });
        out.join(" ")
    }).collect::<Vec<_>>().join("  →  ")
}
fn sequence(key: &str) -> anyhow::Result<Vec<Keystroke>> {
    key.split_whitespace().map(|k| Keystroke::parse(k).map_err(Into::into)).collect()
}
fn overlaps(a: &[Keystroke], b: &[Keystroke]) -> bool {
    !a.is_empty() && !b.is_empty() && (a.starts_with(b) || b.starts_with(a))
}
/// Prefix conflicts also need replacing: a single key must not consume a chord.
fn conflicts(s: &Shortcuts, id: &str, key: &str) -> Vec<String> {
    let Ok(parsed) = sequence(key) else { return vec![] };
    let contexts: Vec<_> = s.base.iter().filter(|b| b.action().name() == id).map(|b| b.predicate().map(|p| p.to_string())).collect();
    let mut found = vec![];
    for b in &s.base {
        let name = b.action().name();
        if name == id || found.iter().any(|n| n == name) { continue; }
        let other = s.custom.get(name).cloned().unwrap_or_else(|| b.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>().join(" "));
        let same_context = contexts.contains(&b.predicate().map(|p| p.to_string()));
        if (label(name).is_some() || same_context) && sequence(&other).is_ok_and(|other| overlaps(&parsed, &other)) {
            found.push(name.to_string());
        }
    }
    found
}
fn change(id: &str, key: Option<String>, cx: &mut App) -> anyhow::Result<()> {
    let s = cx.global::<Shortcuts>();
    let mut custom = s.custom.clone();
    let key = key.unwrap_or_else(|| default_key(s, id));
    let parsed = sequence(&key)?;
    if !parsed.is_empty() {
        anyhow::ensure!(parsed.len() <= 4, "Use up to four strokes");
        let m = parsed[0].modifiers;
        anyhow::ensure!(m.platform || m.control || m.alt, "Start with ⌘, ⌃ or ⌥");
    }
    for other in conflicts(s, id, &key) { custom.insert(other, String::new()); }
    // Empty means explicitly unbound, rather than falling back to the default.
    if key == default_key(s, id) { custom.remove(id); } else { custom.insert(id.into(), key.clone()); }
    write(&custom)?;
    cx.global_mut::<Shortcuts>().custom = custom;
    rebuild(cx); cx.refresh_windows(); Ok(())
}
fn write(custom: &BTreeMap<String, String>) -> anyhow::Result<()> {
    use std::io::Write;
    let tmp = path().with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(custom)?)?;
    f.sync_all()?;
    std::fs::rename(tmp, path())?;
    Ok(())
}
impl Workspace {
    pub(crate) fn cancel_shortcut_recording(&mut self, cx: &mut Context<Self>) {
        self.shortcut_strokes.clear();
        if self.shortcut_recording.take().is_some() {
            rebuild(cx);
        }
    }

    pub(crate) fn start_recording(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_shortcut_recording(cx);
        self.shortcut_recording = Some(id.to_string());
        self.shortcut_error = None;
        window.focus(&self.focus);
        cx.clear_key_bindings();
        cx.notify();
    }

    pub(crate) fn reset_shortcut(&mut self, id: &str, cx: &mut Context<Self>) {
        self.cancel_shortcut_recording(cx);
        self.shortcut_error = change(id, None, cx).err().map(|e| (id.to_string(), e.to_string()));
        rebuild(cx);
        cx.notify();
    }

    fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.cancel_shortcut_recording(cx);
        let custom = BTreeMap::new();
        match write(&custom) {
            Ok(()) => { cx.global_mut::<Shortcuts>().custom = custom; self.shortcut_error = None; }
            Err(e) => self.shortcut_error = Some((String::new(), e.to_string())),
        }
        rebuild(cx);
        cx.refresh_windows();
        cx.notify();
    }

    fn save_recorded_shortcut(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.shortcut_recording.clone() else { return };
        if self.shortcut_strokes.is_empty() { return; }
        match change(&id, Some(self.shortcut_strokes.join(" ")), cx) {
            Ok(()) => { self.cancel_shortcut_recording(cx); self.shortcut_error = None; }
            Err(e) => { self.shortcut_error = Some((id, e.to_string())); cx.clear_key_bindings(); }
        }
        cx.notify();
    }

    fn clear_shortcut(&mut self, id: &str, cx: &mut Context<Self>) {
        self.cancel_shortcut_recording(cx);
        self.shortcut_error = change(id, Some(String::new()), cx).err().map(|e| (id.to_string(), e.to_string()));
        cx.notify();
    }

    pub(crate) fn record_shortcut(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.shortcut_recording.clone() else { return false };
        cx.stop_propagation();
        if ev.is_held { return true; }
        let plain = ev.keystroke.modifiers == Modifiers::default();
        if plain && ev.keystroke.key == "escape" { self.cancel_shortcut_recording(cx); cx.notify(); return true; }
        if plain && ev.keystroke.key == "enter" { self.save_recorded_shortcut(cx); return true; }
        if plain && ev.keystroke.key == "backspace" { self.shortcut_strokes.pop(); self.shortcut_error = None; cx.notify(); return true; }
        if matches!(ev.keystroke.key.as_str(), "shift" | "alt" | "ctrl" | "cmd" | "fn" | "control" | "platform" | "function") { return true; }
        self.shortcut_error = None;
        let m = ev.keystroke.modifiers;
        if self.shortcut_strokes.is_empty() && !(m.platform || m.control || m.alt) {
            self.shortcut_error = Some((id, "Start with ⌘, ⌃ or ⌥".into()));
        } else if self.shortcut_strokes.len() >= 4 {
            self.shortcut_error = Some((id, "Save or press Backspace to edit the sequence".into()));
        } else { self.shortcut_strokes.push(ev.keystroke.unparse()); }
        cx.notify(); true
    }

    fn shortcut_recorder(&self, cx: &mut Context<Self>) -> AnyElement {
        let key = self.shortcut_strokes.join(" ");
        let id = self.shortcut_recording.as_deref().unwrap_or("");
        let replaced = conflicts(cx.global::<Shortcuts>(), id, &key).iter().map(|n| label(n).unwrap_or(n).to_string()).collect::<Vec<_>>();
        div().flex().flex_col().gap(px(6.)).py(px(6.))
            .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child("Record up to four strokes, Enter to save · Esc to cancel · Backspace to edit"))
            .when(!replaced.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(format!("{}{}", "Saving will unbind: ", replaced.join("、")))))
            .child(div().flex().gap(px(8.))
                .when(!key.is_empty(), |d| d.child(crate::settings::link_button("shortcut-save", "Save").on_click(cx.listener(|w, _, _, cx| w.save_recorded_shortcut(cx)))))
                .child(crate::settings::link_button("shortcut-cancel", "Cancel").on_click(cx.listener(|w, _, _, cx| { w.cancel_shortcut_recording(cx); cx.notify(); }))))
            .into_any_element()
    }

    /// 快捷键's controls on the page title's line: 全部恢复默认 (once something
    /// is changed) and a compact search field, like a toolbar search.
    pub(crate) fn shortcut_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let customized = cx.try_global::<Shortcuts>().is_some_and(|s| s.custom.keys().any(|k| k != "global"));
        div().flex_none().flex().items_center().gap(px(8.))
            .when(customized, |d| d.child(crate::settings::link_button("shortcuts-reset-all", "Reset all").on_click(cx.listener(|w, _, _, cx| w.reset_all_shortcuts(cx)))))
            .child(div().w(px(200.)).h(px(28.)).px(px(8.)).flex().items_center().gap(px(6.)).rounded(px(8.)).bg(Theme::fill_hover())
                .child(crate::ui::icon("search", 14., Theme::text_caption()))
                .child(div().flex_1().min_w_0().child(self.settings_filter.clone())))
            .into_any_element()
    }

    pub(crate) fn shortcut_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let q = self.settings_filter.read(cx).text.trim().to_lowercase();
        let customized = cx.try_global::<Shortcuts>().map(|s| s.custom.keys().filter(|k| *k != "global").cloned().collect::<Vec<_>>()).unwrap_or_default();
        let mut groups: Vec<Vec<AnyElement>> = (0..GROUPS.len()).map(|_| vec![]).collect();
        for (id, title, key) in rows(cx) {
            let Some((g, zh, en)) = info(&id) else { continue };
            if !q.is_empty() && ![zh.to_lowercase(), en.to_lowercase(), pretty(&key).to_lowercase(), key.to_lowercase()].iter().any(|s| s.contains(&q)) { continue; }
            let recording = self.shortcut_recording.as_ref() == Some(&id);
            let error = self.shortcut_error.as_ref().filter(|(e, _)| *e == id).map(|(_, m)| m.clone());
            let is_custom = customized.contains(&id);
            let (rec, reset, clear) = (id.clone(), id.clone(), id.clone());
            let shown_key = if recording { self.shortcut_strokes.join(" ") } else { key.clone() };
            groups[g].push(div().group("shortcut-row").w_full().px(px(16.)).py(px(7.)).flex().flex_col().gap(px(2.))
                .child(div().w_full().min_h(px(28.)).flex().items_center().gap(px(12.))
                    .child(div().flex_1().min_w_0().text_size(px(13.)).line_height(px(20.)).child(title))
                    .when(is_custom && !recording, |d| d.child(div().id(SharedString::from(format!("reset-{reset}"))).h(px(26.)).px(px(8.)).flex().items_center().rounded(px(7.)).cursor_pointer()
                        .text_size(px(12.)).text_color(Theme::primary()).hover(|s| s.bg(Theme::fill_hover())).child("Reset")
                        .on_click(cx.listener(move |w, _, _, cx| w.reset_shortcut(&reset, cx)))))
                    .when(!recording && !key.is_empty(), |d| d.child(crate::settings::link_button(SharedString::from(format!("clear-{clear}")), "Remove").invisible().group_hover("shortcut-row", |s| s.visible()).on_click(cx.listener(move |w, _, _, cx| w.clear_shortcut(&clear, cx)))))
                    .child(crate::settings::keycap(SharedString::from(id.clone()), &shown_key, recording)
                        .on_click(cx.listener(move |w, _, window, cx| w.start_recording(&rec, window, cx)))))
                .when(recording, |d| d.child(self.shortcut_recorder(cx)))
                .children(error.map(|e| div().text_size(px(12.)).line_height(px(17.)).text_color(Theme::danger()).child(e)))
                .into_any_element());
        }
        let empty = groups.iter().all(|g| g.is_empty());
        let general_error = self.shortcut_error.as_ref().filter(|(id, _)| id.is_empty()).map(|(_, e)| e.clone());
        div().w_full().flex().flex_col().gap(px(28.))
            .children(general_error.map(|e| div().px(px(4.)).text_size(px(12.)).line_height(px(18.)).text_color(Theme::danger()).child(e)))
            .when(empty, |d| d.child(div().py(px(40.)).flex().justify_center().text_size(px(13.)).text_color(Theme::text_caption()).child("No matching shortcuts")))
            .children(groups.into_iter().enumerate().filter(|(_, rows)| !rows.is_empty()).map(|(g, rows)| {
                let n = rows.len();
                div().w_full().flex().flex_col().gap(px(8.))
                    .child(div().px(px(4.)).text_size(px(13.)).line_height(px(18.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::sidebar_section()).child(GROUPS[g].1))
                    .child(div().w_full().flex().flex_col().rounded(px(12.)).border_1().border_color(Theme::line_border()).overflow_hidden()
                        .children(rows.into_iter().enumerate().map(move |(i, r)| div().w_full().flex().flex_col().when(i + 1 < n, |d| d.border_b_1().border_color(crate::ui::list_line())).child(r))))
            }))
            .into_any_element()
    }
}
