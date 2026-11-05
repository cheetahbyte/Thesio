use crate::store::*;
use anyhow::Result;
use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc};

#[derive(Clone)]
pub struct SharedStore {
    store: Rc<RefCell<Store>>,
    /// Problems found opening the vault, until a window shows them.
    opening: Rc<RefCell<Vec<String>>>,
}
impl gpui::Global for SharedStore {}
impl SharedStore {
    pub fn open() -> Result<Self> {
        let (store, warnings) = Store::open()?;
        Ok(Self { store: Rc::new(RefCell::new(store)), opening: Rc::new(RefCell::new(warnings)) })
    }
    pub fn root(&self) -> PathBuf { self.store.borrow().root().to_path_buf() }
    pub fn path(&self, id: &str) -> Result<PathBuf> { self.store.borrow().path(id) }
    pub fn take_opening_warnings(&self) -> Vec<String> { std::mem::take(&mut self.opening.borrow_mut()) }
    pub fn list(&mut self) -> Vec<Entry> { self.store.borrow_mut().list() }
    pub fn load(&self, id: &str) -> Result<String> { self.store.borrow().load(id) }
    pub fn save(&mut self, id: &str, text: &str) -> Result<()> { self.store.borrow_mut().save(id, text) }
    pub fn create(&mut self, parent: Option<&str>) -> Result<String> { self.store.borrow_mut().create(parent) }
    pub fn create_from(&mut self, parent: Option<&str>, title: &str, text: &str) -> Result<String> { self.store.borrow_mut().create_from(parent, title, text) }
    pub fn create_folder(&mut self, parent: Option<&str>, title: &str) -> Result<String> { self.store.borrow_mut().create_folder(parent, title) }
    pub fn delete(&mut self, id: &str) -> Result<Vec<String>> { self.store.borrow_mut().delete(id) }
    pub fn list_trash(&self) -> Vec<TrashEntry> { self.store.borrow().list_trash() }
    pub fn purge_expired(&mut self) -> Vec<String> { self.store.borrow_mut().purge_expired() }
    pub fn restore(&mut self, id: &str) -> Result<Restored> { self.store.borrow_mut().restore(id) }
    pub fn purge(&mut self, id: &str) -> Result<()> { self.store.borrow_mut().purge(id) }
    pub fn import(&mut self, path: &Path) -> Result<String> { self.store.borrow_mut().import(path) }
    pub fn settings(&self) -> Settings { self.store.borrow().settings() }
    pub fn save_settings(&self, s: &Settings) -> Result<()> { self.store.borrow().save_settings(s) }
    pub fn attach(&mut self, from: &Path, id: &str) -> Result<()> { self.store.borrow_mut().attach(from, id) }
    pub fn rename(&mut self, id: &str, title: &str) -> Result<Moved> { self.store.borrow_mut().rename(id, title) }
    pub fn versions(&self, id: &str) -> Result<thesio_core::history::Versions> { self.store.borrow().versions(id) }
    pub fn touch(&mut self, id: &str) -> Result<()> { self.store.borrow_mut().touch(id) }
    pub fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<Moved> { self.store.borrow_mut().set_parent(id, parent) }
    pub fn set_starred(&mut self, id: &str, on: bool) -> Result<u64> { self.store.borrow_mut().set_starred(id, on) }
    pub fn set_page_offset(&mut self, id: &str, offset: i32) -> Result<()> { self.store.borrow_mut().set_page_offset(id, offset) }
    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> { self.store.borrow_mut().set_pinned(id, pinned) }
}
