use crate::shared_store::SharedStore;
use gpui::*;

#[derive(Clone, Copy)]
struct Size([f32; 2]);
impl Global for Size {}

pub fn remember(window: &Window, cx: &mut App) {
    if let WindowBounds::Windowed(bounds) = window.window_bounds() {
        cx.set_global(Size([f32::from(bounds.size.width), f32::from(bounds.size.height)]));
    }
}

pub fn persist(store: &SharedStore, cx: &App) {
    let Some(Size(size)) = cx.try_global::<Size>().copied() else { return };
    if let Err(e) = store.borrow().update_settings(|settings| settings.window_size = Some(size)) { eprintln!("Could not save window size: {e}"); }
}

pub fn main_window_size(cx: &App) -> gpui::Size<Pixels> {
    let s = cx.global::<SharedStore>().borrow().settings().window_size.unwrap_or([1280., 800.]);
    size(px(s[0].max(480.)), px(s[1].max(480.)))
}
