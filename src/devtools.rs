//! Scripted UI driver for development: `THESIO_SCRIPT=steps.txt` replays input
//! into the window and saves shots to `THESIO_SHOTS` (default /tmp). `COMMANDS`
//! is the script grammar, and tools/harness/README.md documents each command;
//! a test keeps the two together.

use gpui::*;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn fps() -> f64 {
    std::env::var("THESIO_RECORD_FPS").ok().and_then(|v| v.parse().ok()).filter(|&f: &f64| f > 0.).unwrap_or(30.)
}

/// Last pointer position, and the cursor log of the running recording.
static POINTER: Mutex<(f32, f32)> = Mutex::new((640., 400.));
static CURSOR_LOG: Mutex<Option<(Instant, std::fs::File)>> = Mutex::new(None);

fn log_pointer(x: f32, y: f32, what: &str) {
    use std::io::Write;
    *POINTER.lock().unwrap() = (x, y);
    if let Some((start, f)) = CURSOR_LOG.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{:.4} {x:.1} {y:.1} {what}", start.elapsed().as_secs_f64());
    }
}

fn ease(t: f32) -> f32 {
    if t < 0.5 { 4. * t * t * t } else { 1. - (-2. * t + 2.).powi(3) / 2. }
}

#[derive(Debug)]
enum Step {
    Wait(u64),
    Key(String),
    Type(String),
    Nkey(String),
    Mark(String),
    Commit(String),
    Move(f32, f32),
    Click(f32, f32),
    Dclick(f32, f32),
    Rclick(f32, f32),
    Cmdclick(f32, f32),
    Drag(f32, f32, f32, f32),
    Down(f32, f32, i64),
    Dragto(f32, f32),
    Up(f32, f32, i64),
    Wheel(f32, f32, f32),
    Shot(String),
    Burst(String, usize),
    Deskshot(String, f64),
    Clip(PathBuf),
    Pick(Vec<PathBuf>),
    Pdfselect(String),
    Resize(f32, f32),
    Place(f32, f32),
    Bench { times: usize, label: String, step: Box<Step> },
    Noop,
    Record(Option<String>),
    Glide(f32, f32, f32),
    Scroll(f32, f32, f32, f32),
    Caption(String),
    Activate,
    Quit,
}

fn nums<const N: usize>(s: &str) -> Option<[f32; N]> {
    let n: Vec<f32> = s.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    n.get(..N)?.try_into().ok()
}

fn text(s: &str) -> Option<String> { Some(s.trim().to_string()).filter(|s| !s.is_empty()) }

/// `x y` and an optional click count.
fn press(s: &str) -> Option<(f32, f32, i64)> {
    let [x, y] = nums(s)?;
    Some((x, y, nums::<3>(s).map_or(1, |[_, _, clicks]| clicks as i64)))
}

fn bench(s: &str) -> Option<Step> {
    let (times, label) = s.trim().split_once(' ').unwrap_or((s.trim(), "noop"));
    let step = parse(label).filter(|step| !matches!(step, Step::Bench { .. } | Step::Quit))?;
    Some(Step::Bench { times: times.parse().ok()?, label: label.to_string(), step: Box::new(step) })
}

type Parse = fn(&str) -> Option<Step>;

/// The script grammar: each command and what its arguments make.
const COMMANDS: &[(&str, Parse)] = &[
    ("wait", |s| Some(Step::Wait(s.trim().parse().ok()?))),
    ("key", |s| text(s).map(Step::Key)),
    // Only leading whitespace is insignificant: `type ab ` types a trailing space.
    ("type", |s| Some(Step::Type(s.to_string()))),
    ("nkey", |s| text(s).map(Step::Nkey)),
    ("mark", |s| Some(Step::Mark(s.to_string()))),
    ("commit", |s| Some(Step::Commit(s.to_string()))),
    ("move", |s| nums(s).map(|[x, y]| Step::Move(x, y))),
    ("click", |s| nums(s).map(|[x, y]| Step::Click(x, y))),
    ("dclick", |s| nums(s).map(|[x, y]| Step::Dclick(x, y))),
    ("rclick", |s| nums(s).map(|[x, y]| Step::Rclick(x, y))),
    ("cmdclick", |s| nums(s).map(|[x, y]| Step::Cmdclick(x, y))),
    ("drag", |s| nums(s).map(|[x0, y0, x1, y1]| Step::Drag(x0, y0, x1, y1))),
    ("down", |s| press(s).map(|(x, y, clicks)| Step::Down(x, y, clicks))),
    ("dragto", |s| nums(s).map(|[x, y]| Step::Dragto(x, y))),
    ("up", |s| press(s).map(|(x, y, clicks)| Step::Up(x, y, clicks))),
    ("wheel", |s| nums(s).map(|[x, y, dy]| Step::Wheel(x, y, dy))),
    ("shot", |s| text(s).map(Step::Shot)),
    ("burst", |s| {
        let (name, n) = s.trim().split_once(' ').unwrap_or((s.trim(), "10"));
        Some(Step::Burst(text(name)?, n.trim().parse().ok()?))
    }),
    ("deskshot", |s| {
        let (name, margin) = s.trim().split_once(' ').unwrap_or((s.trim(), "48"));
        Some(Step::Deskshot(text(name)?, margin.trim().parse().ok()?))
    }),
    ("clip", |s| s.trim().strip_prefix("text ").map(|path| Step::Clip(path.trim().into()))),
    ("pick", |s| Some(Step::Pick(s.trim().split('|').map(PathBuf::from).collect()))),
    ("pdfselect", |s| text(s).map(Step::Pdfselect)),
    ("resize", |s| nums(s).map(|[w, h]| Step::Resize(w, h))),
    ("place", |s| nums(s).map(|[x, y]| Step::Place(x, y))),
    ("bench", bench),
    ("noop", |_| Some(Step::Noop)),
    ("record", |s| Some(Step::Record(text(s).filter(|path| path != "stop")))),
    ("glide", |s| nums(s).map(|[x, y, ms]| Step::Glide(x, y, ms))),
    ("scroll", |s| nums(s).map(|[x, y, dy, ms]| Step::Scroll(x, y, dy, ms))),
    ("caption", |s| text(s).map(Step::Caption)),
    ("activate", |_| Some(Step::Activate)),
    ("quit", |_| Some(Step::Quit)),
];

fn parse(line: &str) -> Option<Step> {
    let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
    COMMANDS.iter().find(|(name, _)| *name == command).and_then(|(_, parse)| parse(rest))
}

fn parse_script(script: &str) -> Vec<Step> {
    script.lines().map(str::trim_start).filter(|l| !l.trim().is_empty() && !l.starts_with('#')).filter_map(|line| {
        let step = parse(line);
        if step.is_none() { eprintln!("devtools: bad step: {line}"); }
        step
    }).collect()
}

/// Lets the frame settle before the next step.
const SETTLE: Duration = Duration::from_millis(120);

struct Driver {
    handle: AnyWindowHandle,
    #[cfg(target_os = "macos")]
    view: usize,
    recording: Option<Recording>,
}

impl Driver {
    fn in_window(&self, cx: &mut AsyncApp, step: impl FnOnce(&mut Window, &mut App)) {
        let _ = self.handle.update(cx, |_, window, cx| step(window, cx));
    }

    async fn stop_recording(&mut self, cx: &mut AsyncApp) {
        let Some(Recording { stop, writer }) = self.recording.take() else { return };
        stop.set(true);
        *CURSOR_LOG.lock().unwrap() = None;
        let _ = cx.background_spawn(async move { writer.join() }).await;
    }

    fn wheel(&self, x: f32, y: f32, dy: f32, cx: &mut AsyncApp) {
        // Positive dy scrolls the content up (reading on).
        let event = ScrollWheelEvent { position: point(px(x), px(y)), delta: ScrollDelta::Pixels(point(px(0.), px(-dy))), ..Default::default() };
        self.in_window(cx, |window, cx| { window.dispatch_event(PlatformInput::ScrollWheel(event), cx); });
    }

    /// Times `step`, drawing a frame synchronously after each run.
    async fn bench(&mut self, times: usize, label: &str, step: &Step, cx: &mut AsyncApp) {
        let mut samples = vec![];
        for _ in 0..times {
            let t0 = Instant::now();
            self.step(step, cx).await;
            let t1 = Instant::now();
            self.in_window(cx, |window, cx| window.draw(cx).clear());
            samples.push(((t1 - t0).as_secs_f64() * 1e3, t1.elapsed().as_secs_f64() * 1e3));
            cx.background_executor().timer(Duration::from_millis(10)).await;
        }
        let stat = |f: fn(&(f64, f64)) -> f64| {
            let mut v: Vec<f64> = samples.iter().map(f).collect();
            v.sort_by(f64::total_cmp);
            let at = |q: f64| v.get(((v.len() as f64 - 1.) * q).round() as usize).copied().unwrap_or(0.);
            format!("p50 {:.2} p95 {:.2} max {:.2}", at(0.5), at(0.95), at(1.))
        };
        eprintln!("bench {label:?} x{}: step [{}] frame [{}] ms", samples.len(), stat(|s| s.0), stat(|s| s.1));
    }

    /// Runs one step and returns how long its frame needs to settle. Mouse
    /// input goes through AppKit, so it runs outside of any `App` borrow.
    async fn step(&mut self, step: &Step, cx: &mut AsyncApp) -> Duration {
        #[cfg(target_os = "macos")]
        use native::*;
        #[cfg(target_os = "macos")]
        let view = self.view;
        match step {
            Step::Wait(ms) => {
                // Draw invalidated frames ourselves: the display link stops
                // while the screen is locked, and nothing would render.
                let end = Instant::now() + Duration::from_millis(*ms);
                while Instant::now() < end {
                    cx.background_executor().timer(Duration::from_millis(16).min(end - Instant::now())).await;
                    self.in_window(cx, |window, cx| if window.needs_draw() { window.draw(cx).clear(); });
                }
                return Duration::ZERO;
            }
            Step::Key(keys) => self.in_window(cx, |window, cx| {
                for k in keys.split_whitespace() {
                    match Keystroke::parse(k) {
                        Ok(ks) => {
                            let handled = window.dispatch_keystroke(ks, cx);
                            if std::env::var_os("THESIO_TRACE").is_some() {
                                eprintln!("devtools: key {k} handled={handled} focused={:?}", window.focused(cx).is_some());
                            }
                        }
                        Err(e) => eprintln!("devtools: bad keystroke {k}: {e:?}"),
                    }
                }
            }),
            Step::Type(text) => self.in_window(cx, |window, cx| {
                for ch in text.chars() {
                    let s = ch.to_string();
                    let ks = Keystroke { modifiers: Modifiers::default(), key: if ch == ' ' { "space".into() } else { s.clone() }, key_char: Some(s) };
                    window.dispatch_keystroke(ks, cx);
                }
            }),
            #[cfg(target_os = "macos")]
            Step::Nkey(letter) => key(view, letter),
            #[cfg(target_os = "macos")]
            Step::Mark(text) => ime(view, false, text),
            #[cfg(target_os = "macos")]
            Step::Commit(text) => ime(view, true, text),
            #[cfg(target_os = "macos")]
            Step::Move(x, y) => {
                log_pointer(*x, *y, "move");
                send(view, MOVED, *x, *y, 0);
            }
            #[cfg(target_os = "macos")]
            Step::Click(x, y) => {
                log_pointer(*x, *y, "click");
                send(view, MOVED, *x, *y, 0);
                send(view, LEFT_DOWN, *x, *y, 1);
                send(view, LEFT_UP, *x, *y, 1);
            }
            #[cfg(target_os = "macos")]
            Step::Dclick(x, y) => {
                log_pointer(*x, *y, "click");
                send(view, LEFT_DOWN, *x, *y, 1);
                send(view, LEFT_UP, *x, *y, 1);
                send(view, LEFT_DOWN, *x, *y, 2);
                send(view, LEFT_UP, *x, *y, 2);
            }
            // Synthesized right-button NSEvents report buttonNumber 0, so GPUI would
            // read them as left clicks; feed the right click to GPUI directly.
            Step::Rclick(x, y) => {
                log_pointer(*x, *y, "click");
                let position = point(px(*x), px(*y));
                self.in_window(cx, |window, cx| {
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers: Modifiers::default() }), cx);
                    click(MouseButton::Right, position, Modifiers::default(), window, cx);
                });
            }
            Step::Cmdclick(x, y) => {
                log_pointer(*x, *y, "click");
                self.in_window(cx, |window, cx| click(MouseButton::Left, point(px(*x), px(*y)), Modifiers { platform: true, ..Default::default() }, window, cx));
            }
            #[cfg(target_os = "macos")]
            Step::Drag(x0, y0, x1, y1) => {
                log_pointer(*x0, *y0, "down");
                send(view, MOVED, *x0, *y0, 0);
                send(view, LEFT_DOWN, *x0, *y0, 1);
                for i in 1..=8 {
                    let t = i as f32 / 8.;
                    let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                    log_pointer(x, y, "move");
                    send(view, LEFT_DRAGGED, x, y, 1);
                }
                log_pointer(*x1, *y1, "up");
                send(view, LEFT_UP, *x1, *y1, 1);
            }
            #[cfg(target_os = "macos")]
            Step::Down(x, y, clicks) => {
                log_pointer(*x, *y, "down");
                send(view, MOVED, *x, *y, 0);
                send(view, LEFT_DOWN, *x, *y, *clicks);
            }
            #[cfg(target_os = "macos")]
            Step::Dragto(x, y) => {
                log_pointer(*x, *y, "move");
                send(view, LEFT_DRAGGED, *x, *y, 1);
            }
            #[cfg(target_os = "macos")]
            Step::Up(x, y, clicks) => {
                log_pointer(*x, *y, "up");
                send(view, LEFT_UP, *x, *y, *clicks);
            }
            Step::Wheel(x, y, dy) => {
                log_pointer(*x, *y, "move");
                self.wheel(*x, *y, *dy, cx);
            }
            Step::Shot(name) => self.in_window(cx, |window, cx| {
                let path = shot_path(name);
                match capture(window, cx, &path) {
                    Ok(()) => eprintln!("devtools: saved {path}"),
                    Err(e) => eprintln!("devtools: capture failed: {e}"),
                }
            }),
            // Captures ~8ms apart, to catch transient frames.
            Step::Burst(name, n) => {
                for i in 0..*n {
                    let path = shot_path(&format!("{name}-{i:02}"));
                    self.in_window(cx, |window, cx| { let _ = capture(window, cx, &path); });
                    cx.background_executor().timer(Duration::from_millis(8)).await;
                }
                return Duration::ZERO;
            }
            #[cfg(target_os = "macos")]
            Step::Deskshot(name, margin) => self.in_window(cx, |window, _| {
                let path = shot_path(name);
                match desk_grab(window, *margin).and_then(|(w, h, mut px)| {
                    for p in px.as_chunks_mut::<4>().0 { p.swap(0, 2); p[3] = 255; }
                    image::save_buffer(&path, &px, w as u32, h as u32, image::ColorType::Rgba8).map_err(Into::into)
                }) {
                    Ok(()) => eprintln!("devtools: saved {path}"),
                    Err(e) => eprintln!("devtools: capture failed: {e}"),
                }
            }),
            Step::Clip(path) => match std::fs::read_to_string(path) {
                Ok(text) => crate::system::script_clipboard(text),
                Err(e) => eprintln!("devtools: cannot read {}: {e}", path.display()),
            },
            Step::Pick(paths) => crate::system::script_pick(paths.clone()),
            Step::Pdfselect(text) => crate::system::script_pdf_selection(text.clone()),
            Step::Resize(w, h) => self.in_window(cx, |window, _| window.resize(size(px(*w), px(*h)))),
            #[cfg(target_os = "macos")]
            Step::Place(x, y) => place_top_left(view, *x as f64, *y as f64),
            Step::Bench { times, label, step } => {
                Box::pin(self.bench(*times, label, step, cx)).await;
                return Duration::ZERO;
            }
            Step::Noop => self.in_window(cx, |window, _| window.refresh()),
            Step::Record(path) => {
                self.stop_recording(cx).await;
                if let Some(path) = path { self.recording = Some(start_recording(self.handle, path, cx)); }
                return Duration::ZERO;
            }
            #[cfg(target_os = "macos")]
            Step::Glide(x, y, ms) => {
                let (x0, y0) = *POINTER.lock().unwrap();
                let steps = (ms / 16.).max(1.) as usize;
                for i in 1..=steps {
                    let t = ease(i as f32 / steps as f32);
                    let (x, y) = (x0 + (x - x0) * t, y0 + (y - y0) * t);
                    send(view, MOVED, x, y, 0);
                    log_pointer(x, y, "move");
                    cx.background_executor().timer(Duration::from_millis(16)).await;
                }
                return Duration::ZERO;
            }
            Step::Scroll(x, y, dy, ms) => {
                log_pointer(*x, *y, "move");
                let steps = (ms / 16.).max(1.) as usize;
                let mut done = 0.;
                for i in 1..=steps {
                    let to = dy * ease(i as f32 / steps as f32);
                    self.wheel(*x, *y, to - done, cx);
                    done = to;
                    cx.background_executor().timer(Duration::from_millis(16)).await;
                }
                return Duration::ZERO;
            }
            Step::Caption(text) => {
                let (x, y) = *POINTER.lock().unwrap();
                log_pointer(x, y, &format!("caption {text}"));
                return Duration::ZERO;
            }
            #[cfg(target_os = "macos")]
            Step::Activate => {
                activate(view);
                return Duration::from_millis(300);
            }
            Step::Quit => {
                self.stop_recording(cx).await;
                let _ = cx.update(|cx| cx.quit());
            }
            #[cfg(not(target_os = "macos"))]
            other => eprintln!("devtools: {other:?} needs macOS"),
        }
        SETTLE
    }
}

fn click(button: MouseButton, position: Point<Pixels>, modifiers: Modifiers, window: &mut Window, cx: &mut App) {
    window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button, position, modifiers, click_count: 1, first_mouse: false }), cx);
    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position, modifiers, click_count: 1 }), cx);
}

fn shot_path(name: &str) -> String {
    format!("{}/{name}.png", std::env::var("THESIO_SHOTS").unwrap_or_else(|_| "/tmp".into()))
}

pub fn start(window: &mut Window, cx: &mut App) {
    let Ok(path) = std::env::var("THESIO_SCRIPT") else { return };
    let Ok(script) = std::fs::read_to_string(&path) else {
        eprintln!("devtools: cannot read {path}");
        return;
    };
    let steps = parse_script(&script);
    let handle = window.window_handle();
    #[cfg(target_os = "macos")]
    let Some(view) = ns_view_of(window) else { return };
    #[cfg(target_os = "macos")]
    native::no_app_nap();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(800)).await;
        #[cfg(target_os = "macos")]
        if std::env::var_os("THESIO_NOPARK").is_none() {
            let mut parked = false;
            for origin in native::park_candidates(view) {
                native::place(view, origin);
                cx.background_executor().timer(Duration::from_millis(250)).await;
                if native::is_visible(view) {
                    parked = true;
                    break;
                }
            }
            if !parked {
                eprintln!("devtools: no visible off-screen spot; window may stop rendering");
            }
        }
        let mut driver = Driver {
            handle,
            #[cfg(target_os = "macos")]
            view,
            recording: None,
        };
        for step in &steps {
            let settle = driver.step(step, cx).await;
            if matches!(step, Step::Quit) { return; }
            cx.background_executor().timer(settle).await;
        }
    })
    .detach();
}

/// Mouse input goes through AppKit (the GPUI view's NSResponder methods) so
/// it takes the same path as real events; it must run outside of any `App` borrow.
#[cfg(target_os = "macos")]
mod native {
    use core_graphics::geometry::{CGPoint, CGRect};
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    pub const MOVED: u64 = 5;
    pub const LEFT_DOWN: u64 = 1;
    pub const LEFT_UP: u64 = 2;
    pub const RIGHT_DOWN: u64 = 3;
    pub const RIGHT_UP: u64 = 4;
    pub const LEFT_DRAGGED: u64 = 6;

    /// Places scripted windows almost entirely past a screen edge that has no
    /// neighbouring display, so they neither cover the user's windows nor get
    /// covered (fully occluded windows stop rendering). Returns the candidate
    /// origins, best first; the caller tries them until one stays visible
    /// (a sliver behind the Dock counts as occluded).
    pub fn park_candidates(ns_view: usize) -> Vec<CGPoint> {
        use core_graphics::geometry::CGSize;
        const SLIVER: f64 = 24.;
        fn intersects(a: &CGRect, b: &CGRect) -> bool {
            a.origin.x < b.origin.x + b.size.width
                && b.origin.x < a.origin.x + a.size.width
                && a.origin.y < b.origin.y + b.size.height
                && b.origin.y < a.origin.y + a.size.height
        }
        unsafe {
            let view = ns_view as *mut Object;
            let win: *mut Object = msg_send![view, window];
            let w: CGRect = msg_send![win, frame];
            let (ww, wh) = (w.size.width, w.size.height);
            let list: *mut Object = msg_send![class!(NSScreen), screens];
            let n: usize = msg_send![list, count];
            let screens: Vec<CGRect> = (0..n)
                .map(|i| {
                    let sc: *mut Object = msg_send![list, objectAtIndex: i];
                    let f: CGRect = msg_send![sc, frame];
                    f
                })
                .collect();
            let mut out = vec![];
            for (i, s) in screens.iter().enumerate() {
                let (sx, sy, sw, sh) = (s.origin.x, s.origin.y, s.size.width, s.size.height);
                let mid_y = sy + (sh - wh) / 2.;
                for o in [
                    CGPoint::new(sx + sw - SLIVER, mid_y),
                    CGPoint::new(sx - ww + SLIVER, mid_y),
                    // Bottom corners first: a centred Dock hides the middle.
                    CGPoint::new(sx, sy - wh + SLIVER),
                    CGPoint::new(sx + sw - ww, sy - wh + SLIVER),
                    CGPoint::new(sx + (sw - ww) / 2., sy - wh + SLIVER),
                ] {
                    let r = CGRect::new(&o, &CGSize::new(ww, wh));
                    if !screens.iter().enumerate().any(|(j, f)| j != i && intersects(&r, f)) {
                        out.push(o);
                    }
                }
            }
            out
        }
    }

    /// `place` with the origin given as the top-left corner in screen
    /// coordinates (y down from the main display's top).
    pub fn place_top_left(ns_view: usize, x: f64, y: f64) {
        unsafe {
            let win: *mut Object = msg_send![ns_view as *mut Object, window];
            let frame: CGRect = msg_send![win, frame];
            let list: *mut Object = msg_send![class!(NSScreen), screens];
            let main: *mut Object = msg_send![list, objectAtIndex: 0usize];
            let screen: CGRect = msg_send![main, frame];
            place(ns_view, CGPoint::new(x, screen.size.height - y - frame.size.height));
        }
    }

    pub fn place(ns_view: usize, origin: CGPoint) {
        unsafe {
            let win: *mut Object = msg_send![ns_view as *mut Object, window];
            let _: () = msg_send![win, setLevel: 3isize]; // NSFloatingWindowLevel
            // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary:
            // stays visible (so it keeps rendering) when the user is in
            // another Space or a full-screen app.
            let _: () = msg_send![win, setCollectionBehavior: (1usize | 1 << 4 | 1 << 6 | 1 << 8)];
            let _: () = msg_send![win, setFrameOrigin: origin];
        }
    }

    /// Opts out of App Nap, which otherwise stretches timers to ~1s while
    /// the (unfocused, mostly off-screen) window is scripted.
    pub fn no_app_nap() {
        unsafe {
            let info: *mut Object = msg_send![class!(NSProcessInfo), processInfo];
            let reason: *mut Object = msg_send![class!(NSString), alloc];
            let text = "scripted UI run";
            let reason: *mut Object = msg_send![reason, initWithBytes: text.as_ptr() length: text.len() encoding: 4usize];
            // NSActivityUserInitiatedAllowingIdleSystemSleep | NSActivityLatencyCritical
            let activity: *mut Object = msg_send![info, beginActivityWithOptions: 0x00EF_FFFFu64 | 0xFF_0000_0000u64 reason: reason];
            let _: *mut Object = msg_send![activity, retain]; // held for the process lifetime
        }
    }

    pub fn is_visible(ns_view: usize) -> bool {
        const NS_WINDOW_OCCLUSION_STATE_VISIBLE: u64 = 1 << 1;
        unsafe {
            let win: *mut Object = msg_send![ns_view as *mut Object, window];
            let state: u64 = msg_send![win, occlusionState];
            state & NS_WINDOW_OCCLUSION_STATE_VISIBLE != 0
        }
    }

    /// IME composition through the view's NSTextInputClient methods:
    /// `mark` = setMarkedText (pinyin in progress), `commit` = insertText.
    pub fn ime(ns_view: usize, commit: bool, text: &str) {
        #[repr(C)]
        struct NSRange {
            location: usize,
            length: usize,
        }
        unsafe impl objc::Encode for NSRange {
            fn encode() -> objc::Encoding {
                unsafe { objc::Encoding::from_str("{_NSRange=QQ}") }
            }
        }
        unsafe {
            let view = ns_view as *mut Object;
            let s: *mut Object = msg_send![class!(NSString), alloc];
            let s: *mut Object = msg_send![s,
                initWithBytes: text.as_ptr()
                length: text.len()
                encoding: 4usize]; // NSUTF8StringEncoding
            let none = NSRange { location: isize::MAX as usize, length: 0 }; // NSNotFound (NSIntegerMax)
            if commit {
                let _: () = msg_send![view, insertText: s replacementRange: none];
            } else {
                let n = text.encode_utf16().count();
                let sel = NSRange { location: n, length: 0 };
                let _: () = msg_send![view, setMarkedText: s selectedRange: sel replacementRange: none];
            }
        }
    }

    /// Makes the window key in the active app, so the input method serves it.
    /// Only for IME checks: it takes focus from whatever the user is doing.
    pub fn activate(ns_view: usize) {
        unsafe {
            let win: *mut Object = msg_send![ns_view as *mut Object, window];
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, activateIgnoringOtherApps: true];
            let nil: *mut Object = std::ptr::null_mut();
            let _: () = msg_send![win, makeKeyAndOrderFront: nil];
        }
    }

    /// Key down + up for a lowercase letter, delivered to the view's
    /// `keyDown:` so it goes through `interpretKeyEvents:` and the input method.
    pub fn key(ns_view: usize, letter: &str) {
        const CODES: &str = "asdfhgzxcv_bqweryt123465=97-80]ou[ip_lj'k;\\,/nm.";
        let Some(code) = CODES.find(letter) else { return };
        unsafe {
            let view = ns_view as *mut Object;
            let win: *mut Object = msg_send![view, window];
            let number: isize = msg_send![win, windowNumber];
            let nil: *mut Object = std::ptr::null_mut();
            let s: *mut Object = msg_send![class!(NSString), alloc];
            let s: *mut Object = msg_send![s, initWithBytes: letter.as_ptr() length: letter.len() encoding: 4usize];
            for (kind, sel) in [(10u64, true), (11u64, false)] {
                let ev: *mut Object = msg_send![class!(NSEvent),
                    keyEventWithType: kind
                    location: CGPoint::new(0., 0.)
                    modifierFlags: 0u64
                    timestamp: 0f64
                    windowNumber: number
                    context: nil
                    characters: s
                    charactersIgnoringModifiers: s
                    isARepeat: false
                    keyCode: code as u16];
                if sel {
                    let _: () = msg_send![view, keyDown: ev];
                } else {
                    let _: () = msg_send![view, keyUp: ev];
                }
            }
        }
    }

    pub fn send(ns_view: usize, kind: u64, x: f32, y: f32, clicks: i64) {
        unsafe {
            let view = ns_view as *mut Object;
            let win: *mut Object = msg_send![view, window];
            let frame: CGRect = msg_send![view, frame];
            let loc = CGPoint::new(x as f64, frame.size.height - y as f64);
            let number: isize = msg_send![win, windowNumber];
            let nil: *mut Object = std::ptr::null_mut();
            let ev: *mut Object = msg_send![class!(NSEvent),
                mouseEventWithType: kind
                location: loc
                modifierFlags: 0u64
                timestamp: 0f64
                windowNumber: number
                context: nil
                eventNumber: 0isize
                clickCount: clicks
                pressure: 1.0f32];
            // Deliver straight to the view: going through `-[NSWindow sendEvent:]`
            // would require the window to be key, i.e. steal focus from the user.
            match kind {
                LEFT_DOWN => msg_send![view, mouseDown: ev],
                LEFT_UP => msg_send![view, mouseUp: ev],
                RIGHT_DOWN => msg_send![view, rightMouseDown: ev],
                RIGHT_UP => msg_send![view, rightMouseUp: ev],
                LEFT_DRAGGED => msg_send![view, mouseDragged: ev],
                _ => msg_send![view, mouseMoved: ev],
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn ns_view_of(window: &Window) -> Option<usize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let RawWindowHandle::AppKit(h) = HasWindowHandle::window_handle(window).ok()?.as_raw() else { return None };
    Some(h.ns_view.as_ptr() as usize)
}

/// The window's content as tightly packed BGRA, rendered offscreen so it
/// works even while the window doesn't (occluded, screen locked). Falls back
/// to the window server's copy, which also has the system-drawn traffic lights.
fn frame(window: &mut Window, cx: &mut App) -> anyhow::Result<(usize, usize, Vec<u8>)> {
    if std::env::var_os("THESIO_WINDOW_CAPTURE").is_none()
        && let Some(image) = window.render_offscreen(cx)
    {
        return Ok(image);
    }
    grab(window)
}

#[cfg(target_os = "macos")]
fn capture(window: &mut Window, cx: &mut App, path: &str) -> anyhow::Result<()> {
    let (w, h, mut px) = frame(window, cx)?;
    for p in px.as_chunks_mut::<4>().0 {
        // BGRA (little-endian premultiplied first) → opaque RGBA.
        p.swap(0, 2);
        p[3] = 255;
    }
    image::save_buffer(path, &px, w as u32, h as u32, image::ColorType::Rgba8)?;
    Ok(())
}

/// The screen around the window (`margin` pt each side): the window and
/// everything below it, as tightly packed BGRA.
#[cfg(target_os = "macos")]
fn desk_grab(window: &Window, margin: f64) -> anyhow::Result<(usize, usize, Vec<u8>)> {
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use core_graphics::window::{create_image, kCGWindowImageBestResolution, kCGWindowListOptionIncludingWindow, kCGWindowListOptionOnScreenBelowWindow};
    use objc::{class, msg_send, sel, sel_impl};
    let view = ns_view_of(window).ok_or_else(|| anyhow::anyhow!("not an AppKit window"))? as *mut objc::runtime::Object;
    let (number, frame, screen_h): (isize, CGRect, f64) = unsafe {
        let win: *mut objc::runtime::Object = msg_send![view, window];
        let list: *mut objc::runtime::Object = msg_send![class!(NSScreen), screens];
        let main: *mut objc::runtime::Object = msg_send![list, objectAtIndex: 0usize];
        let screen: CGRect = msg_send![main, frame];
        (msg_send![win, windowNumber], msg_send![win, frame], screen.size.height)
    };
    let top = screen_h - frame.origin.y - frame.size.height;
    let rect = CGRect::new(&CGPoint::new(frame.origin.x - margin, top - margin), &CGSize::new(frame.size.width + 2. * margin, frame.size.height + 2. * margin));
    let img = create_image(rect, kCGWindowListOptionOnScreenBelowWindow | kCGWindowListOptionIncludingWindow, number as u32, kCGWindowImageBestResolution)
        .ok_or_else(|| anyhow::anyhow!("CGWindowListCreateImage returned null"))?;
    let (w, h, stride) = (img.width(), img.height(), img.bytes_per_row());
    let data = img.data();
    let bytes = data.bytes();
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        out.extend_from_slice(&bytes[y * stride..y * stride + w * 4]);
    }
    Ok((w, h, out))
}

/// The window's pixels as tightly packed BGRA.
#[cfg(target_os = "macos")]
fn grab(window: &Window) -> anyhow::Result<(usize, usize, Vec<u8>)> {
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use core_graphics::window::{create_image, kCGWindowImageBoundsIgnoreFraming, kCGWindowListOptionIncludingWindow};
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let RawWindowHandle::AppKit(h) = HasWindowHandle::window_handle(window).map_err(|e| anyhow::anyhow!("{e:?}"))?.as_raw() else { anyhow::bail!("not an AppKit window") };
    let view = h.ns_view.as_ptr() as *mut objc::runtime::Object;
    let number: isize = unsafe {
        let ns_window: *mut objc::runtime::Object = msg_send![view, window];
        msg_send![ns_window, windowNumber]
    };
    let img = create_image(CGRect::new(&CGPoint::new(f64::INFINITY, f64::INFINITY), &CGSize::new(0., 0.)), kCGWindowListOptionIncludingWindow, number as u32, kCGWindowImageBoundsIgnoreFraming)
        .ok_or_else(|| anyhow::anyhow!("CGWindowListCreateImage returned null"))?;
    let (w, h, stride) = (img.width(), img.height(), img.bytes_per_row());
    let data = img.data();
    let bytes = data.bytes();
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        out.extend_from_slice(&bytes[y * stride..y * stride + w * 4]);
    }
    Ok((w, h, out))
}

/// A running capture: `stop` ends it, and `writer` then drains into ffmpeg.
struct Recording {
    stop: Rc<Cell<bool>>,
    writer: std::thread::JoinHandle<()>,
}

fn start_recording(handle: AnyWindowHandle, path: &str, cx: &mut AsyncApp) -> Recording {
    let stop = Rc::new(Cell::new(false));
    let start = Instant::now();
    *CURSOR_LOG.lock().unwrap() = std::fs::File::create(format!("{path}.cursor")).ok().map(|f| (start, f));
    let (x, y) = *POINTER.lock().unwrap();
    log_pointer(x, y, "move");
    let (tx, rx) = std::sync::mpsc::channel::<(f64, usize, usize, Vec<u8>)>();
    let out = path.to_string();
    let fps = fps();
    // Encoding runs off the main thread; frames that arrive late are repeated
    // so the video keeps wall-clock time.
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let mut ffmpeg: Option<std::process::Child> = None;
        let mut next = 0u64;
        while let Ok((t, w, h, mut px)) = rx.recv() {
            let enc = ffmpeg.get_or_insert_with(|| {
                std::process::Command::new("ffmpeg")
                    .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "bgra", "-s", &format!("{w}x{h}"), "-r", &fps.to_string(), "-i", "-"])
                    .args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "6", "-pix_fmt", "yuv444p", &out])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .expect("ffmpeg")
            });
            for p in px.as_chunks_mut::<4>().0 {
                p[3] = 255;
            }
            let stdin = enc.stdin.as_mut().unwrap();
            while next as f64 <= t * fps {
                if stdin.write_all(&px).is_err() {
                    return;
                }
                next += 1;
            }
        }
        if let Some(mut enc) = ffmpeg {
            drop(enc.stdin.take());
            let _ = enc.wait();
        }
        eprintln!("devtools: recorded {out} ({next} frames)");
    });
    let flag = stop.clone();
    cx.spawn(async move |cx| {
        while !flag.get() {
            let t = start.elapsed().as_secs_f64();
            if let Ok(Ok((w, h, px))) = handle.update(cx, |_, window, cx| frame(window, cx)) {
                let _ = tx.send((t, w, h, px));
            }
            cx.background_executor().timer(Duration::from_millis(12)).await;
        }
    })
    .detach();
    Recording { stop, writer }
}

#[cfg(not(target_os = "macos"))]
fn capture(_: &mut Window, _: &mut App, _: &str) -> anyhow::Result<()> {
    anyhow::bail!("capture is only implemented on macOS")
}

#[cfg(not(target_os = "macos"))]
fn grab(_: &Window) -> anyhow::Result<(usize, usize, Vec<u8>)> {
    anyhow::bail!("capture is only implemented on macOS")
}

#[cfg(test)]
mod tests {
    use super::COMMANDS;

    #[test]
    fn readme_documents_the_commands() {
        let readme = include_str!("../tools/harness/README.md");
        let table = readme.split("## Script commands").nth(1).unwrap().split("Environment variables").next().unwrap();
        let documented: Vec<&str> = table.lines().filter_map(|row| row.strip_prefix('|')?.split('|').next())
            .flat_map(|cell| cell.split('`').skip(1).step_by(2)).filter_map(|command| command.split(' ').next()).collect();
        for (name, _) in COMMANDS { assert!(documented.contains(name), "{name} is not in the README"); }
        for name in documented { assert!(COMMANDS.iter().any(|(known, _)| *known == name), "{name} is not a command"); }
    }
}
