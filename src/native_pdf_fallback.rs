//! Other platforms open PDFs in the system's app.
use gpui::{Bounds, Pixels, Window};
use std::path::Path;

pub const fn supported() -> bool { false }

#[derive(Default)]
pub struct PdfPane;
impl PdfPane {
    pub fn show(&self, _: &Path, _: Bounds<Pixels>, _: bool, _: &Window) -> bool { false }
    pub fn close(&self) {}
}
