use gpui::*;

#[derive(Clone, Copy, Default)]
struct Size(Option<[f32; 2]>);
impl Global for Size {}

pub fn remember(window: &Window, cx: &mut App) {
    if let WindowBounds::Windowed(bounds) = window.window_bounds() {
        cx.set_global(Size(Some([f32::from(bounds.size.width), f32::from(bounds.size.height)])));
    }
}

pub fn persist(cx: &mut App) {
    let (Some(size), Some(store)) = (cx.try_global::<Size>().copied(), cx.try_global::<crate::shared_store::SharedStore>()) else { return };
    let mut settings = store.settings();
    if size.0.is_some() { settings.window_size = size.0 }
    if let Err(e) = store.save_settings(&settings) { eprintln!("无法保存窗口尺寸：{e}"); }
}

pub fn main_window_size(cx: &App) -> gpui::Size<Pixels> {
    let s = cx.global::<crate::shared_store::SharedStore>().settings().window_size.unwrap_or([1280., 800.]);
    size(px(s[0].max(480.)), px(s[1].max(480.)))
}
