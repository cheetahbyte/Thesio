//! Other platforms open PDFs in the system's app.
use gpui::{Bounds, Pixels, Window};
use std::path::Path;

pub const fn supported() -> bool { false }

#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub page: usize,
    pub index: usize,
    pub label: String,
    pub quote: String,
    pub comment: String,
}

#[derive(Default)]
pub struct PdfPane;
impl PdfPane {
    pub fn show(&self, _: &Path, _: Bounds<Pixels>, _: bool, _: &Window) -> bool { false }
    pub fn close(&self) {}
    pub fn take_opened(&self) -> bool { false }
    pub fn highlights(&self) -> Vec<Highlight> { vec![] }
    pub fn highlight_selection(&self) -> Result<bool, String> { Ok(false) }
    pub fn set_comment(&self, _: &Highlight, _: &str) -> Result<(), String> { Ok(()) }
    pub fn remove(&self, _: &Highlight) -> Result<(), String> { Ok(()) }
    pub fn go_to(&self, _: &Highlight) {}
    pub fn select_text(&self, _: &str) {}
    pub fn release_keys(&self) {}
}
