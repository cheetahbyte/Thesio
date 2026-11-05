//! The PDF viewer: AppKit's PDFView above the GPUI surface. PDFKit brings text
//! selection, search, and zoom, and its annotations are standard PDF ones that
//! other readers show too. A native view hides GPUI drawing under it, so the
//! caller passes `covered` while a menu or dialog overlaps the pane. Highlights
//! are written into the PDF itself, as one annotation per page.
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use gpui::*;
use objc::{class, msg_send, runtime::{BOOL, NO, Object, YES}, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {
    static PDFAnnotationSubtypeHighlight: *mut Object;
}

/// `kPDFDisplaySinglePageContinuous`.
const CONTINUOUS: isize = 1;

/// `NSCaseInsensitiveSearch`.
const CASE_INSENSITIVE: usize = 1;

pub const fn supported() -> bool { true }

/// A highlight in the open paper.
#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    /// Page and position among the page's annotations.
    pub page: usize,
    pub index: usize,
    /// The page's own label; empty when it only repeats the position.
    pub label: String,
    pub quote: String,
    pub comment: String,
}

#[derive(Default)]
pub struct PdfPane { pane: RefCell<Option<Pane>>, highlights: RefCell<Vec<Highlight>>, opened: Cell<bool> }
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
                let pane = Pane { view, gpui, path: path.to_path_buf() };
                *self.highlights.borrow_mut() = pane.highlights();
                self.opened.set(true);
                *slot = Some(pane);
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

    pub fn close(&self) {
        self.pane.borrow_mut().take();
        self.highlights.borrow_mut().clear();
    }

    /// True once after a paper opened: its highlights are ready to show.
    pub fn take_opened(&self) -> bool { self.opened.take() }

    pub fn highlights(&self) -> Vec<Highlight> { self.highlights.borrow().clone() }

    /// Highlights the selected text and saves the paper. `Ok(false)` when nothing is selected.
    pub fn highlight_selection(&self) -> Result<bool, String> {
        let slot = self.pane.borrow();
        let Some(pane) = slot.as_ref() else { return Ok(false) };
        if !unsafe { pane.highlight_selection() } { return Ok(false); }
        self.changed(pane).map(|()| true)
    }

    pub fn set_comment(&self, highlight: &Highlight, comment: &str) -> Result<(), String> {
        let slot = self.pane.borrow();
        let Some(pane) = slot.as_ref() else { return Ok(()) };
        unsafe {
            let Some(annotation) = pane.annotation(highlight) else { return Ok(()) };
            let text = nsstring(comment);
            let _: () = msg_send![annotation, setContents: text];
        }
        self.changed(pane)
    }

    pub fn remove(&self, highlight: &Highlight) -> Result<(), String> {
        let slot = self.pane.borrow();
        let Some(pane) = slot.as_ref() else { return Ok(()) };
        unsafe {
            let Some(annotation) = pane.annotation(highlight) else { return Ok(()) };
            let page: *mut Object = msg_send![pane.document(), pageAtIndex: highlight.page];
            let _: () = msg_send![page, removeAnnotation: annotation];
        }
        self.changed(pane)
    }

    pub fn go_to(&self, highlight: &Highlight) {
        let slot = self.pane.borrow();
        let Some(pane) = slot.as_ref() else { return };
        unsafe {
            let Some(annotation) = pane.annotation(highlight) else { return };
            let page: *mut Object = msg_send![pane.document(), pageAtIndex: highlight.page];
            let bounds: CGRect = msg_send![annotation, bounds];
            let _: () = msg_send![pane.view, goToRect: bounds onPage: page];
        }
    }

    /// Selects the first match of each `|`-separated part of `text`, for scripted runs.
    pub fn select_text(&self, text: &str) {
        let slot = self.pane.borrow();
        let Some(pane) = slot.as_ref() else { return };
        unsafe {
            let mut selection = std::ptr::null_mut::<Object>();
            for part in text.split('|') {
                let found: *mut Object = msg_send![pane.document(), findString: nsstring(part) withOptions: CASE_INSENSITIVE];
                let first: *mut Object = msg_send![found, firstObject];
                if first.is_null() { continue; }
                // The next search frees the matches of this one.
                if selection.is_null() { selection = msg_send![first, retain]; } else { let _: () = msg_send![selection, addSelection: first]; }
            }
            if !selection.is_null() {
                let _: () = msg_send![pane.view, setCurrentSelection: selection];
                let _: () = msg_send![selection, release];
            }
        }
    }

    /// Hands the keyboard back to GPUI, for a click beside the viewer.
    pub fn release_keys(&self) {
        if let Some(pane) = self.pane.borrow().as_ref() { unsafe { pane.release_keys(); } }
    }

    fn changed(&self, pane: &Pane) -> Result<(), String> {
        *self.highlights.borrow_mut() = pane.highlights();
        pane.save()
    }
}

fn nsstring(text: &str) -> *mut Object {
    let text = std::ffi::CString::new(text.replace('\0', "")).unwrap_or_default();
    unsafe { msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()] }
}

fn text_of(string: *mut Object) -> String {
    if string.is_null() { return String::new(); }
    unsafe {
        let bytes: *const std::ffi::c_char = msg_send![string, UTF8String];
        if bytes.is_null() { return String::new(); }
        std::ffi::CStr::from_ptr(bytes).to_string_lossy().into_owned()
    }
}

/// The rectangle of quad `points[at..at + 4]`, in page space.
unsafe fn quad_rect(points: *mut Object, at: usize, origin: CGPoint) -> CGRect {
    let corner = |index: usize| -> CGPoint { unsafe {
        let value: *mut Object = msg_send![points, objectAtIndex: index];
        msg_send![value, pointValue]
    } };
    let (first, last) = (corner(at), corner(at + 3));
    let (x, y) = (first.x.min(last.x), first.y.min(last.y));
    CGRect::new(&CGPoint::new(origin.x + x, origin.y + y), &CGSize::new((last.x - first.x).abs(), (last.y - first.y).abs()))
}

impl Pane {
    unsafe fn document(&self) -> *mut Object { unsafe { msg_send![self.view, document] } }

    unsafe fn annotation(&self, highlight: &Highlight) -> Option<*mut Object> {
        unsafe {
            let page: *mut Object = msg_send![self.document(), pageAtIndex: highlight.page];
            let annotations: *mut Object = msg_send![page, annotations];
            let count: usize = msg_send![annotations, count];
            (highlight.index < count).then(|| msg_send![annotations, objectAtIndex: highlight.index])
        }
    }

    fn highlights(&self) -> Vec<Highlight> {
        let mut out = vec![];
        unsafe {
            let document = self.document();
            let pages: usize = msg_send![document, pageCount];
            for number in 0..pages {
                let page: *mut Object = msg_send![document, pageAtIndex: number];
                let label: *mut Object = msg_send![page, label];
                let label = Some(text_of(label)).filter(|label| *label != (number + 1).to_string()).unwrap_or_default();
                let annotations: *mut Object = msg_send![page, annotations];
                let count: usize = msg_send![annotations, count];
                for index in 0..count {
                    let annotation: *mut Object = msg_send![annotations, objectAtIndex: index];
                    let kind: *mut Object = msg_send![annotation, type];
                    if text_of(kind) != "Highlight" { continue; }
                    let contents: *mut Object = msg_send![annotation, contents];
                    out.push(Highlight { page: number, index, label: label.clone(), quote: quote_of(annotation, page), comment: text_of(contents) });
                }
            }
        }
        out
    }

    unsafe fn highlight_selection(&self) -> bool {
        unsafe {
            let selection: *mut Object = msg_send![self.view, currentSelection];
            if selection.is_null() { return false; }
            let lines: *mut Object = msg_send![selection, selectionsByLine];
            let count: usize = msg_send![lines, count];
            let mut pages: Vec<(*mut Object, Vec<CGRect>)> = vec![];
            for index in 0..count {
                let line: *mut Object = msg_send![lines, objectAtIndex: index];
                let on: *mut Object = msg_send![line, pages];
                let page: *mut Object = msg_send![on, firstObject];
                if page.is_null() { continue; }
                let rect: CGRect = msg_send![line, boundsForPage: page];
                match pages.iter_mut().find(|(known, _)| *known == page) {
                    Some((_, rects)) => rects.push(rect),
                    None => pages.push((page, vec![rect])),
                }
            }
            if pages.is_empty() { return false; }
            for (page, rects) in pages { add_highlight(page, &rects); }
            let _: () = msg_send![self.view, clearSelection];
            true
        }
    }

    /// Writes beside the paper and swaps it in, so a failure leaves the paper whole.
    fn save(&self) -> Result<(), String> {
        let name = self.path.file_name().map_or(String::new(), |name| name.to_string_lossy().into_owned());
        let partial = self.path.with_file_name(format!(".{name}.part"));
        let written: BOOL = unsafe { msg_send![self.document(), writeToFile: nsstring(&partial.to_string_lossy())] };
        if written != YES { return Err("the paper could not be written".to_string()); }
        std::fs::rename(&partial, &self.path).map_err(|e| e.to_string())
    }

    unsafe fn release_keys(&self) {
        unsafe {
            let window: *mut Object = msg_send![self.gpui, window];
            let responder: *mut Object = msg_send![window, firstResponder];
            if responder.is_null() { return; }
            let is_view: BOOL = msg_send![responder, isKindOfClass: class!(NSView)];
            if is_view != YES { return; }
            let inside: BOOL = msg_send![responder, isDescendantOf: self.view];
            if inside == YES { let _: BOOL = msg_send![window, makeFirstResponder: self.gpui]; }
        }
    }
}

unsafe fn quote_of(annotation: *mut Object, page: *mut Object) -> String {
    unsafe {
        let bounds: CGRect = msg_send![annotation, bounds];
        let points: *mut Object = msg_send![annotation, quadrilateralPoints];
        let count: usize = if points.is_null() { 0 } else { msg_send![points, count] };
        let rects: Vec<CGRect> = if count >= 4 { (0..count / 4).map(|quad| quad_rect(points, quad * 4, bounds.origin)).collect() } else { vec![bounds] };
        let parts: Vec<String> = rects.into_iter().map(|rect| {
            let selection: *mut Object = msg_send![page, selectionForRect: rect];
            if selection.is_null() { return String::new(); }
            let text: *mut Object = msg_send![selection, string];
            text_of(text).split_whitespace().collect::<Vec<_>>().join(" ")
        }).collect();
        parts.join(" ")
    }
}

unsafe fn add_highlight(page: *mut Object, rects: &[CGRect]) {
    let (left, bottom) = rects.iter().fold((f64::MAX, f64::MAX), |(x, y), r| (x.min(r.origin.x), y.min(r.origin.y)));
    let (right, top) = rects.iter().fold((f64::MIN, f64::MIN), |(x, y), r| (x.max(r.origin.x + r.size.width), y.max(r.origin.y + r.size.height)));
    let bounds = CGRect::new(&CGPoint::new(left, bottom), &CGSize::new(right - left, top - bottom));
    unsafe {
        let points: *mut Object = msg_send![class!(NSMutableArray), array];
        for rect in rects {
            let (x, y) = (rect.origin.x - left, rect.origin.y - bottom);
            let (w, h) = (rect.size.width, rect.size.height);
            for (px, py) in [(x, y + h), (x + w, y + h), (x, y), (x + w, y)] {
                let value: *mut Object = msg_send![class!(NSValue), valueWithPoint: CGPoint::new(px, py)];
                let _: () = msg_send![points, addObject: value];
            }
        }
        let nil = std::ptr::null_mut::<Object>();
        let annotation: *mut Object = msg_send![class!(PDFAnnotation), alloc];
        let annotation: *mut Object = msg_send![annotation, initWithBounds: bounds forType: PDFAnnotationSubtypeHighlight withProperties: nil];
        let color: *mut Object = msg_send![class!(NSColor), colorWithSRGBRed: 1f64 green: 0.86f64 blue: 0.2f64 alpha: 0.55f64];
        let _: () = msg_send![annotation, setColor: color];
        let _: () = msg_send![annotation, setQuadrilateralPoints: points];
        let _: () = msg_send![page, addAnnotation: annotation];
        let _: () = msg_send![annotation, release];
    }
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
        self.release_keys();
        let _: () = msg_send![self.view, removeFromSuperview];
        let _: () = msg_send![self.view, release];
    } }
}
