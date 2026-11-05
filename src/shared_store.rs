use crate::store::*;
use anyhow::Result;
use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc};

#[derive(Clone)]
pub struct SharedStore(Rc<RefCell<Store>>);
impl gpui::Global for SharedStore {}
impl SharedStore {
    pub fn open() -> Result<Self> { Ok(Self(Rc::new(RefCell::new(Store::open()?)))) }
    pub fn root(&self) -> PathBuf { self.0.borrow().root().to_path_buf() }
    pub fn take_warnings(&self) -> Vec<String> { std::mem::take(&mut self.0.borrow_mut().warnings) }
    pub fn list(&mut self) -> Vec<Entry> { self.0.borrow_mut().list() }
    pub fn load(&self, id: &str) -> Result<String> { self.0.borrow().load(id) }
    pub fn save(&mut self, id: &str, text: &str) -> Result<()> { self.0.borrow_mut().save(id, text) }
    pub fn create(&mut self, parent: Option<&str>) -> Result<String> { self.0.borrow_mut().create(parent) }
    pub fn create_from(&mut self, parent: Option<&str>, title: &str, text: &str) -> Result<String> { self.0.borrow_mut().create_from(parent, title, text) }
    pub fn create_folder(&mut self, parent: Option<&str>, title: &str) -> Result<String> { self.0.borrow_mut().create_folder(parent, title) }
    pub fn delete(&mut self, id: &str) -> Result<()> { self.0.borrow_mut().delete(id) }
    pub fn list_trash(&mut self) -> Vec<TrashEntry> { self.0.borrow_mut().list_trash() }
    pub fn restore(&mut self, id: &str) -> Result<bool> { self.0.borrow_mut().restore(id) }
    pub fn purge(&mut self, id: &str) -> Result<()> { self.0.borrow_mut().purge(id) }
    pub fn import(&mut self, path: &Path) -> Result<String> { self.0.borrow_mut().import(path) }
    pub fn settings(&self) -> Settings { self.0.borrow().settings() }
    pub fn save_settings(&self, s: &Settings) -> Result<()> { self.0.borrow().save_settings(s) }
    pub fn rename(&mut self, id: &str, title: &str) -> Result<(String, Vec<String>)> { self.0.borrow_mut().rename(id, title) }
    pub fn touch(&mut self, id: &str) -> Result<()> { self.0.borrow_mut().touch(id) }
    pub fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<(String, Vec<String>)> { self.0.borrow_mut().set_parent(id, parent) }
    pub fn set_starred(&mut self, id: &str, on: bool) -> Result<u64> { self.0.borrow_mut().set_starred(id, on) }
    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> { self.0.borrow_mut().set_pinned(id, pinned) }
}
