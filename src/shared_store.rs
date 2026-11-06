use thesio_core::store::Store;
use anyhow::Result;
use std::{cell::{Ref, RefCell, RefMut}, rc::Rc};

/// The vault's `Store`, one per process and shared by every window. Hold a
/// borrow for one call: a `match` on `borrow_mut()` keeps it through its arms.
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
    pub fn borrow(&self) -> Ref<'_, Store> { self.store.borrow() }
    pub fn borrow_mut(&self) -> RefMut<'_, Store> { self.store.borrow_mut() }
    pub fn take_opening_warnings(&self) -> Vec<String> { std::mem::take(&mut self.opening.borrow_mut()) }
}
