//! Design tokens: the app's palette, type scale, shapes and shadows (light).
//! The layout departs in one place: a sunken sidebar on the window surface
//! and a full-bleed white note plane instead of floating panels.

use gpui::{BoxShadow, Hsla, Rgba, point, px, rgb};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ---- dark mode
//
// Every colour here is designed for the light look. In dark mode the
// palette helpers (`c`, `ca`, `mix`, `ta`) and the theme files' colours
// pass through `adapt`, which mirrors lightness: the white page turns dark
// gray, near-black text near-white, tints dark tints, while vivid colours
// (the accent, coloured text) keep their hue and stay readable. Text
// highlights keep their light colours instead (`mark`, `on_mark`).
// `rgbaf` stays literal for shadows, scrims and controls drawn over images.

static DARK: AtomicBool = AtomicBool::new(false);
static ACCENT: AtomicU32 = AtomicU32::new(craft::B500);

thread_local! {
    /// Exports and printing draw light whatever the window shows.
    static FORCE_LIGHT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ADAPTED: std::cell::RefCell<std::collections::HashMap<[u32; 4], Hsla>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

pub fn dark() -> bool { DARK.load(Ordering::Relaxed) && !FORCE_LIGHT.with(|f| f.get()) }
/// The accent as `0xrrggbb`: the system's on macOS, else `craft::B500`.
pub fn accent() -> u32 { ACCENT.load(Ordering::Relaxed) }

/// Read the system accent; true when it changed. Call on the main thread.
#[cfg(target_os = "macos")]
pub fn sync_accent() -> bool {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    let hex = objc::rc::autoreleasepool(|| unsafe {
        let accent: *mut Object = msg_send![class!(NSColor), controlAccentColor];
        let space: *mut Object = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let color: *mut Object = msg_send![accent, colorUsingColorSpace: space];
        if color.is_null() { return None }
        let (r, g, b): (f64, f64, f64) = (msg_send![color, redComponent], msg_send![color, greenComponent], msg_send![color, blueComponent]);
        let byte = |v: f64| (v.clamp(0., 1.) * 255.).round() as u32;
        Some(byte(r) << 16 | byte(g) << 8 | byte(b))
    });
    hex.is_some_and(|hex| ACCENT.swap(hex, Ordering::Relaxed) != hex)
}
#[cfg(not(target_os = "macos"))]
pub fn sync_accent() -> bool { false }

/// Switch the look; true when it changed.
pub fn set_dark(on: bool) -> bool { DARK.swap(on, Ordering::Relaxed) != on }

/// The colour to draw for light-look colour `c`: itself, or in dark mode its dark counterpart.
pub fn adapt(c: Hsla) -> Hsla {
    if !dark() { return c }
    let key = [c.h.to_bits(), c.s.to_bits(), c.l.to_bits(), c.a.to_bits()];
    if let Some(d) = ADAPTED.with(|m| m.borrow().get(&key).copied()) { return d }
    let d = to_dark(c);
    ADAPTED.with(|m| m.borrow_mut().insert(key, d));
    d
}

/// Lightness mirrored in OKLab: white → L 0.23 (the page, #1c1c1c), the
/// text (L 0.25) → L 0.91, black → white. Chroma and hue are kept; vivid
/// colours (chroma ≥ 0.14) keep their own lightness within 0.62–0.86, pale
/// ones follow the mirror, with a smooth blend between.
fn to_dark(c: Hsla) -> Hsla {
    let Rgba { r, g, b, a } = Rgba::from(c);
    let [ll, oa, ob] = oklab(r, g, b);
    let chroma = (oa * oa + ob * ob).sqrt();
    let mirror = (1.137 - 0.907 * ll).clamp(0., 1.);
    let t = ((chroma - 0.04) / 0.10).clamp(0., 1.);
    let t = t * t * (3. - 2. * t);
    let l2 = mirror + (ll.clamp(0.62, 0.86) - mirror) * t;
    srgb(l2, oa, ob, a)
}

// sRGB ⇄ OKLab with Björn Ottosson's matrices, kept digit for digit.
#[allow(clippy::excessive_precision)]
fn oklab(r: f32, g: f32, b: f32) -> [f32; 3] {
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s, 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s]
}
/// OKLab → linear sRGB (unclamped: out of [0, 1] means out of gamut).
#[allow(clippy::excessive_precision)]
fn lrgb(l: f32, oa: f32, ob: f32) -> [f32; 3] {
    let l_ = l + 0.3963377774 * oa + 0.2158037573 * ob;
    let m_ = l - 0.1055613458 * oa - 0.0638541728 * ob;
    let s_ = l - 0.0894841775 * oa - 1.2914855480 * ob;
    let (l_, m_, s_) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_, -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_, -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_]
}
fn srgb(l: f32, oa: f32, ob: f32, a: f32) -> Hsla {
    let [r, g, b] = lrgb(l, oa, ob);
    let enc = |v: f32| { let v = v.clamp(0., 1.); if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1. / 2.4) - 0.055 } };
    Rgba { r: enc(r), g: enc(g), b: enc(b), a }.into()
}

pub fn c(hex: u32) -> Hsla {
    adapt(rgb(hex).into())
}

/// A surface above the page (menus, dialogs, toasts, buttons, the picked
/// segment): `light` in the light look; in dark mode a step lighter than the
/// page, as raised surfaces are.
pub fn raised(light: Hsla) -> Hsla { if dark() { rgbaf(0x2c, 0x2c, 0x2e, 1.) } else { light } }

/// A literal colour, never adapted (shadows, scrims, glyphs over images).
pub fn rgbaf(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a }.into()
}

/// `rgbaf` for the page and chrome: adapted in dark mode.
pub fn ta(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    adapt(rgbaf(r, g, b, a))
}

/// `hex` at opacity `a`.
pub fn ca(hex: u32, a: f32) -> Hsla {
    ta((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, a)
}

/// Colour blend: `a` moved `t` of the way to `b` (opaque result).
pub fn mix(a: u32, b: u32, t: f32) -> Hsla {
    let ch = |s: u32| -> (f32, f32) { (((a >> s) & 0xff) as f32, ((b >> s) & 0xff) as f32) };
    let m = |(x, y): (f32, f32)| (x + (y - x) * t).round() as u8;
    ta(m(ch(16)), m(ch(8)), m(ch(0)), 1.)
}

/// The palette on the ramp names the code was written against
/// (`N*` neutrals, `R*` / `O*` reds and oranges), so every
/// `craft::N300` still means "border", `N600` "secondary text" and so on.
/// Neutrals are measured greys; `R*` / `O*` are Tailwind v4 ramps.
#[allow(dead_code)]
pub mod craft {
    pub const N00: u32 = 0xffffff;
    /// Code block fill.
    pub const N50: u32 = 0xfbfbfb;
    /// Popover / quick-entry body.
    pub const N100: u32 = 0xf7f7f7;
    /// Inactive chrome.
    pub const N200: u32 = 0xf1f2f3;
    /// Code block border, card hairlines (opaque on white).
    pub const N300: u32 = 0xeaebeb;
    /// Selected sidebar row, opaque.
    pub const N350: u32 = 0xdedfe0;
    pub const N400: u32 = 0xc9cacb;
    /// Quaternary text, inactive glyphs.
    pub const N500: u32 = 0x9c9fa0;
    /// Secondary text (dates, meta).
    pub const N600: u32 = 0x737477;
    pub const N650: u32 = 0x5d6064;
    /// Sidebar rows (text at 85% on the sidebar).
    pub const N700: u32 = 0x3f4143;
    /// Toolbar glyphs.
    pub const N800: u32 = 0x303335;
    /// Text (`.page` color).
    pub const N900: u32 = 0x1f2225;
    pub const N950: u32 = 0x141517;
    pub const N1000: u32 = 0x000000;
    /// The accent where the system offers none (see [`super::accent`]), and syntax and search hits.
    pub const B500: u32 = 0x4c6ef5;
    pub const B700: u32 = 0x3751c0;
    pub const R50: u32 = 0xfef2f2;
    pub const R100: u32 = 0xffe2e2;
    pub const R200: u32 = 0xffc9c9;
    pub const R300: u32 = 0xffa2a2;
    pub const R350: u32 = 0xff6467;
    pub const R400: u32 = 0xfb2c36;
    pub const R500: u32 = 0xe7000b;
    pub const R600: u32 = 0xc10007;
    pub const R700: u32 = 0x9f0712;
    pub const R800: u32 = 0x82181a;
    pub const R900: u32 = 0x460809;
    pub const O50: u32 = 0xfff7ed;
    pub const O100: u32 = 0xffedd4;
    pub const O200: u32 = 0xffd6a8;
    pub const O300: u32 = 0xffb86a;
    pub const O350: u32 = 0xff8904;
    pub const O400: u32 = 0xff6900;
    pub const O500: u32 = 0xf54900;
    pub const O600: u32 = 0xca3500;
    pub const O700: u32 = 0x9f2d00;
    pub const O800: u32 = 0x7e2a0c;
    pub const O900: u32 = 0x441306;

    /// The secondary colours some controls use: a green and an orange.
    pub mod old {
        pub const G500: u32 = 0x00c950;
        pub const O500: u32 = 0xfe9a00;
    }

    /// Window surfaces (measured on the key window).
    /// The sunken surface: the sidebar material, here the whole window
    /// behind the note.
    pub const WINDOW: u32 = 0xeff0f1;
    /// The chrome between floating panels.
    pub const CHROME: u32 = 0xe6e7e8;
}

/// Shadows stay black; on the dark page they need more weight to show.
fn shadow(a: f32, y: f32, blur: f32, spread: f32) -> BoxShadow {
    let a = if dark() { (a * 3.).min(0.6) } else { a };
    BoxShadow { color: rgbaf(0, 0, 0, a), offset: point(px(0.), px(y)), blur_radius: px(blur), spread_radius: px(spread) }
}

/// Menus and dropdowns: macOS 26 menu panel, soft and wide.
pub fn shadow_menu() -> Vec<BoxShadow> {
    vec![shadow(0.05, 1., 3., 0.), shadow(0.10, 8., 24., 0.)]
}

/// Second-level panels, search popups, toasts.
pub fn shadow_panel() -> Vec<BoxShadow> {
    vec![shadow(0.05, 1., 4., 0.), shadow(0.12, 12., 32., 0.)]
}

/// Modal dialogs (the modal container shadow is a deep, soft 9-slice).
pub fn shadow_dialog() -> Vec<BoxShadow> {
    vec![shadow(0.06, 2., 8., 0.), shadow(0.16, 24., 64., 0.)]
}

/// Larger menus (「…」, home menus, icon picker): same family as menus.
pub fn shadow_s4_down() -> Vec<BoxShadow> {
    vec![shadow(0.05, 1., 3., 0.), shadow(0.11, 10., 28., 0.)]
}

pub const UI_FONT: &str = ".SystemUIFont";
/// SF Mono.
#[cfg(target_os = "macos")]
pub const MONO_FONT: &str = ".AppleSystemUIFontMonospaced";
#[cfg(not(target_os = "macos"))]
pub const MONO_FONT: &str = "Consolas";

pub struct Theme;

impl Theme {
    pub fn text() -> Hsla { c(craft::N900) }
    /// Secondary text: dates, meta, captions of controls.
    pub fn text_caption() -> Hsla { c(craft::N600) }
    pub fn text_placeholder() -> Hsla { c(craft::N500) }
    pub fn text_disabled() -> Hsla { ca(craft::N900, 0.3) }
    /// Toolbar glyphs.
    pub fn text_code() -> Hsla { c(craft::N800) }
    /// The link colour: the content accent (#0087FF) blended half-way to the
    /// text colour, #0F5492 measured.
    pub fn link() -> Hsla { mix(craft::B500, craft::N900, 0.5) }
    pub fn primary() -> Hsla { c(accent()) }
    pub fn primary_500() -> Hsla { c(accent()) }
    /// The accent, darker: pressed buttons.
    pub fn primary_strong() -> Hsla { mix(accent(), craft::N1000, 0.2) }
    /// The note: white page.
    pub fn bg_body() -> Hsla { c(craft::N00) }
    /// Light fills inside the note (code blocks, quiet cards).
    pub fn bg_overlay() -> Hsla { c(craft::N100) }
    pub fn line_divider() -> Hsla { ca(craft::N1000, 0.06) }
    pub fn line_border() -> Hsla { ca(craft::N1000, 0.08) }
    pub fn fill_hover() -> Hsla { ca(craft::N1000, 0.045) }
    pub fn fill_hover_strong() -> Hsla { ca(craft::N1000, 0.06) }
    pub fn fill_pressed() -> Hsla { ca(craft::N1000, 0.09) }
    /// Grouped rows: settings and forms, set off by tone instead of an outline.
    pub fn fill_group() -> Hsla { ca(craft::N1000, 0.03) }
    /// A filled control held down.
    pub fn fill_active() -> Hsla { ca(craft::N1000, 0.13) }
    /// Text selection: the accent, light.
    pub fn selection() -> Hsla { ca(accent(), 0.24) }
    pub fn scrollbar_thumb() -> Hsla { ca(craft::N1000, 0.22) }
    pub fn pinned() -> Hsla { c(accent()) }
    pub fn danger() -> Hsla { c(craft::R500) }

    // ---- Thesio shell
    /// A light gray tint separates the sidebar from the white page while keeping wallpaper colors quiet.
    /// The document pane paints its own opaque white surface.
    pub fn window() -> Hsla { if crate::native_header::supported() { ca(craft::WINDOW, 0.) } else { ca(craft::WINDOW, 0.84) } }
    /// Lifts the sidebar off the glass behind it. Fully clear glass settles
    /// near mid gray, where no text colour alone stays readable over a busy
    /// wallpaper.
    pub fn sidebar_veil() -> Hsla { ca(craft::N00, 0.5) }
    /// Sidebar row label.
    pub fn sidebar_text() -> Hsla { c(craft::N900) }
    /// Sidebar glyphs that stay neutral: the header buttons and chevrons.
    pub fn sidebar_icon() -> Hsla { ca(craft::N900, 0.8) }
    /// Row glyphs: the accent, as in Finder.
    pub fn sidebar_glyph() -> Hsla { c(accent()) }
    /// A sidebar button that cannot be used.
    pub fn sidebar_disabled() -> Hsla { ca(craft::N900, 0.45) }
    pub fn sidebar_hover() -> Hsla { ca(craft::N1000, 0.07) }
    /// Selected sidebar row.
    pub fn sidebar_selected() -> Hsla { ca(craft::N1000, 0.11) }
    /// Section headers on the sidebar.
    pub fn sidebar_section() -> Hsla { c(0x333739) }
    /// Chrome glyphs (sidebar rows, header buttons): lighter than text so
    /// they don't pull the eye from the labels and the page. Inferred: the
    /// SF Symbols sit near secondary label; this is between N700 and N600.
    pub fn icon() -> Hsla { ca(craft::N900, 0.6) }
    /// Line above the sidebar footer (1pt, black 3.5%, #E8E8E9 on the sidebar).
    pub fn sidebar_divider() -> Hsla { ca(craft::N1000, 0.035) }
    /// Line under the page title.
    /// White pill buttons on the note (back / + / share): hairline.
    pub fn button_border() -> Hsla { ca(craft::N1000, 0.075) }
    /// Captions under media and code (weight 300).
    #[allow(dead_code)]
    pub fn caption() -> Hsla { c(0x7e8287) }
    /// Menu row highlight: quiet, so the label stays as it is.
    pub fn menu_highlight() -> Hsla { ca(craft::N1000, 0.07) }
    /// Menus / popovers body (quick-entry & native menus).
    pub fn popover_bg() -> Hsla { c(0xfafafa) }
}

// ---- layout metrics (pt)

/// The sidebar is 273pt wide plus its 7pt inset; the sunken
/// sidebar keeps that footprint.
pub const SIDEBAR_WIDTH: f32 = 280.0;
pub const SIDEBAR_MIN: f32 = 240.0;
pub const SIDEBAR_MAX: f32 = 800.0;
/// The editor keeps at least this much room when the sidebar is dragged wider.
pub const SIDEBAR_MIN_CONTENT: f32 = 540.0;
/// Left inset of the header controls that sit beside the macOS traffic lights.
pub const TRAFFIC_LIGHTS_W: f32 = if cfg!(target_os = "macos") { 92.0 } else { 16.0 };
