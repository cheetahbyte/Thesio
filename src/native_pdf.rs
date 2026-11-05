//! The PDF viewer: AppKit's PDFView above the GPUI surface. PDFKit brings text
//! selection, search, and zoom, and its annotations are standard PDF ones that
//! other readers show too. A native view hides GPUI drawing under it, so the
//! caller passes `covered` while a menu or dialog overlaps the pane.
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use gpui::*;
use objc::{class, msg_send, runtime::{BOOL, NO, Object, YES}, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {}

/// `kPDFDisplaySinglePageContinuous`.
const CONTINUOUS: isize = 1;

pub const fn supported() -> bool { true }

#[derive(Default)]
pub struct PdfPane { pane: RefCell<Option<Pane>> }
struct Pane { view: *mut Object, gpui: *mut Object, path: PathBuf }

impl PdfPane {
    /// Shows `path` over `bounds`. False when PDFKit cannot read it.
    pub fn show(&self, path: &Path, bounds: Bounds<Pixels>, covered: bool, window: &Window) -> bool {
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return false };
        let gpui = handle.ns_view.as_ptr() as *mut Object;
        let mut slot = self.pane.borrow_mut();
        if slot.as_ref().is_some_and(|pane| pane.path != path) { *slot = None; }
        unsafe {
            let parent: *mut Object = msg_send![gpui, superview];
            if parent.is_null() { return false; }
            if slot.is_none() {
                let Some(view) = open(path, parent, gpui) else { return false };
                *slot = Some(Pane { view, gpui, path: path.to_path_buf() });
            }
            let Some(pane) = slot.as_ref() else { return false };
            let base: CGRect = msg_send![gpui, frame];
            let flipped: BOOL = msg_send![parent, isFlipped];
            let (top, height) = (f32::from(bounds.top()) as f64, f32::from(bounds.size.height) as f64);
            let y = if flipped == YES { top } else { base.size.height - top - height };
            let frame = CGRect::new(&CGPoint::new(f32::from(bounds.left()) as f64, y), &CGSize::new(f32::from(bounds.size.width) as f64, height));
            let _: () = msg_send![pane.view, setFrame: frame];
            let _: () = msg_send![pane.view, setHidden: if covered { YES } else { NO }];
        }
        true
    }

    pub fn close(&self) { self.pane.borrow_mut().take(); }
}

unsafe fn open(path: &Path, parent: *mut Object, gpui: *mut Object) -> Option<*mut Object> {
    let path = std::ffi::CString::new(path.to_str()?).ok()?;
    unsafe {
        let text: *mut Object = msg_send![class!(NSString), stringWithUTF8String: path.as_ptr()];
        let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: text];
        let document: *mut Object = msg_send![class!(PDFDocument), alloc];
        let document: *mut Object = msg_send![document, initWithURL: url];
        if document.is_null() { return None; }
        let view: *mut Object = msg_send![class!(PDFView), alloc];
        let view: *mut Object = msg_send![view, initWithFrame: CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(1., 1.))];
        let _: () = msg_send![view, setDisplayMode: CONTINUOUS];
        let _: () = msg_send![view, setAutoScales: YES];
        let background: *mut Object = msg_send![class!(NSColor), windowBackgroundColor];
        let _: () = msg_send![view, setBackgroundColor: background];
        let _: () = msg_send![view, setDocument: document];
        let _: () = msg_send![document, release];
        let _: () = msg_send![parent, addSubview: view positioned: 1isize relativeTo: gpui];
        Some(view)
    }
}

impl Drop for Pane {
    fn drop(&mut self) { unsafe {
        // Keys go back to GPUI when the viewer held them.
        let window: *mut Object = msg_send![self.gpui, window];
        let responder: *mut Object = msg_send![window, firstResponder];
        let inside: BOOL = if responder.is_null() { NO } else {
            let is_view: BOOL = msg_send![responder, isKindOfClass: class!(NSView)];
            if is_view == YES { msg_send![responder, isDescendantOf: self.view] } else { NO }
        };
        if inside == YES { let _: BOOL = msg_send![window, makeFirstResponder: self.gpui]; }
        let _: () = msg_send![self.view, removeFromSuperview];
        let _: () = msg_send![self.view, release];
    } }
}
