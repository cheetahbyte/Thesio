//! Assets compiled into the binary, and SF Symbols drawn by AppKit on macOS.

use gpui::{AssetSource, SharedString};
#[cfg(target_os = "macos")]
use gpui::{DevicePixels, Size};
use std::borrow::Cow;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES.iter().find(|(p, _)| *p == path).map(|(_, b)| Cow::Borrowed(*b)))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(FILES.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| SharedString::from(*p)).collect())
    }

    #[cfg(target_os = "macos")]
    fn render_mask(&self, path: &str, size: Size<DevicePixels>) -> Option<(Size<DevicePixels>, Vec<u8>)> {
        let symbol = path.strip_prefix("sf/")?;
        let (symbol, weight) = symbol.strip_suffix("/medium").map_or((symbol, 0.), |s| (s, 0.23));
        symbol_mask(symbol, weight, size.width.0.min(size.height.0) as f64 * SYMBOL_SCALE)
    }
}

/// Point size of a symbol in its icon box. Symbols run wider than their point
/// size, and the Lucide boxes these sizes came from carried padding.
#[cfg(target_os = "macos")]
const SYMBOL_SCALE: f64 = 0.78;

/// `name` at `points`, as an alpha mask of its own pixel size.
#[cfg(target_os = "macos")]
fn symbol_mask(name: &str, weight: f64, points: f64) -> Option<(Size<DevicePixels>, Vec<u8>)> {
    use core_graphics::base::kCGImageAlphaPremultipliedLast;
    use core_graphics::color_space::CGColorSpace;
    use core_graphics::context::CGContext;
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use foreign_types::ForeignType;
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};

    let name = std::ffi::CString::new(name).ok()?;
    objc::rc::autoreleasepool(|| unsafe {
        let name: *mut Object = msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
        let nil = std::ptr::null_mut::<Object>();
        let image: *mut Object = msg_send![class!(NSImage), imageWithSystemSymbolName: name accessibilityDescription: nil];
        if image.is_null() { return None; }
        let config: *mut Object = msg_send![class!(NSImageSymbolConfiguration), configurationWithPointSize: points weight: weight scale: 2isize];
        let image: *mut Object = msg_send![image, imageWithSymbolConfiguration: config];
        if image.is_null() { return None; }
        let drawn: CGSize = msg_send![image, size];
        let (width, height) = (drawn.width.ceil() as usize, drawn.height.ceil() as usize);
        if width == 0 || height == 0 { return None; }
        let mut bitmap = CGContext::create_bitmap_context(None, width, height, 8, width * 4, &CGColorSpace::create_device_rgb(), kCGImageAlphaPremultipliedLast);
        let context: *mut Object = msg_send![class!(NSGraphicsContext), graphicsContextWithCGContext: bitmap.as_ptr() as *mut Object flipped: false];
        let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
        let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext: context];
        let rect = CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(width as f64, height as f64));
        let whole = CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(0., 0.));
        let _: () = msg_send![image, drawInRect: rect fromRect: whole operation: 2usize fraction: 1f64];
        let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
        let mask = bitmap.data().iter().skip(3).step_by(4).copied().collect();
        Some((Size { width: DevicePixels(width as i32), height: DevicePixels(height as i32) }, mask))
    })
}
