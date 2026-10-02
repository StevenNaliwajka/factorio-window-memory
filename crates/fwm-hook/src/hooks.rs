//! The detours. Each one calls the original function exactly once; our own logic
//! runs inside `catch_unwind` so a bug here can never unwind into the game.

use crate::game::{Game, Widget};
use crate::screen;
use fwm_core::geometry::{bounds_from_centered, clamp_to_bounds, Point};
use fwm_core::store::Store;
use fwm_core::targets::is_generic_window_class;
use retour::RawDetour;
use std::collections::HashSet;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

type CenterFn = unsafe extern "C" fn(*mut Widget);
type DragFn = unsafe extern "C" fn(*mut Widget, *const c_void) -> bool;

static GAME: OnceLock<Game> = OnceLock::new();
static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
/// Window classes logged so far; each is logged once unless verbose.
static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static VERBOSE: AtomicBool = AtomicBool::new(false);

const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn guarded<R>(f: impl FnOnce() -> R) -> Option<R> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}

static CENTER: AtomicUsize = AtomicUsize::new(0);

/// Let Factorio centre the window, then move it to its saved spot if there is one.
unsafe extern "C" fn window_center(window: *mut Widget) {
    let original: CenterFn = std::mem::transmute(CENTER.load(Relaxed));
    original(window);
    guarded(|| after_center(window));
}

unsafe fn after_center(window: *mut Widget) {
    let (Some(game), Some(store)) = (GAME.get(), STORE.get()) else {
        return;
    };
    let Some(class) = game.class_name(window) else {
        return;
    };
    let centered = game.location(window);
    let size = game.size(window);
    let (saved, ignored) = {
        let store = lock(store);
        (
            store.get(&class),
            store.is_ignored(&class) || is_generic_window_class(&class),
        )
    };
    let bounds = screen::client_size().unwrap_or_else(|| bounds_from_centered(centered, size));
    let first = SEEN
        .get()
        .is_some_and(|seen| lock(seen).insert(class.to_string()));
    let verbose = VERBOSE.load(Relaxed);
    if first || verbose {
        let saved_text = saved.map_or_else(|| "none".to_owned(), |p| p.to_string());
        let note = if ignored { " ignored" } else { "" };
        log!("seen class={class} centered={centered} size={size} bounds={bounds} saved={saved_text}{note}");
    }
    let Some(saved) = saved.filter(|_| !ignored) else {
        return;
    };
    let target = clamp_to_bounds(saved, size, bounds);
    game.set_location(window, target);
    if first || verbose {
        log!(
            "placed class={class} target={target} now={}",
            game.location(window)
        );
    }
}

/// Dragging a window either reaches `agui::Window::mouseDrag` directly or goes
/// through a title-bar widget whose drag target is the window. Either way: note
/// where the window was, let the game move it, and record where it ended up.
macro_rules! drag_hook {
    ($detour:ident, $original:ident, $via:literal, $self_is_window:literal) => {
        static $original: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn $detour(widget: *mut Widget, event: *const c_void) -> bool {
            let original: DragFn = std::mem::transmute($original.load(Relaxed));
            let before = guarded(|| drag_start(widget, $self_is_window)).flatten();
            let handled = original(widget, event);
            if let Some((window, from)) = before {
                guarded(|| drag_end(window, from, $via));
            }
            handled
        }
    };
}

drag_hook!(window_mouse_drag, WINDOW_DRAG, "window", true);
drag_hook!(widget_mouse_drag, WIDGET_DRAG, "widget", false);
drag_hook!(label_mouse_drag, LABEL_DRAG, "label", false);
drag_hook!(layout_mouse_drag, LAYOUT_DRAG, "layout", false);
drag_hook!(
    empty_widget_mouse_drag,
    EMPTY_WIDGET_DRAG,
    "empty-widget",
    false
);

unsafe fn drag_start(widget: *mut Widget, self_is_window: bool) -> Option<(*mut Widget, Point)> {
    let game = GAME.get()?;
    let window = if self_is_window {
        widget
    } else {
        game.drag_target(widget)
    };
    (!window.is_null()).then(|| (window, game.location(window)))
}

unsafe fn drag_end(window: *mut Widget, from: Point, via: &str) {
    let (Some(game), Some(store)) = (GAME.get(), STORE.get()) else {
        return;
    };
    let to = game.location(window);
    if to == from {
        return;
    }
    let Some(class) = game.class_name(window) else {
        return;
    };
    if is_generic_window_class(&class) {
        return;
    }
    {
        let mut store = lock(store);
        if store.is_ignored(&class) {
            return;
        }
        store.set(&class, to);
    }
    if VERBOSE.load(Relaxed) {
        log!("moved class={class} to={to} via={via}");
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Center,
    WindowDrag,
    WidgetDrag,
    LabelDrag,
    LayoutDrag,
    EmptyWidgetDrag,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Center => "Window::center",
            Kind::WindowDrag => "Window::mouseDrag",
            Kind::WidgetDrag => "Widget::mouseDrag",
            Kind::LabelDrag => "Label::mouseDrag",
            Kind::LayoutDrag => "Layout::mouseDrag",
            Kind::EmptyWidgetDrag => "EmptyWidget::mouseDrag",
        }
    }

    fn detour(self) -> (*const (), &'static AtomicUsize) {
        match self {
            Kind::Center => (window_center as *const (), &CENTER),
            Kind::WindowDrag => (window_mouse_drag as *const (), &WINDOW_DRAG),
            Kind::WidgetDrag => (widget_mouse_drag as *const (), &WIDGET_DRAG),
            Kind::LabelDrag => (label_mouse_drag as *const (), &LABEL_DRAG),
            Kind::LayoutDrag => (layout_mouse_drag as *const (), &LAYOUT_DRAG),
            Kind::EmptyWidgetDrag => (empty_widget_mouse_drag as *const (), &EMPTY_WIDGET_DRAG),
        }
    }
}

pub struct HookSpec {
    pub kind: Kind,
    pub target: usize,
    pub required: bool,
}

/// Detour every function in `specs` and start saving positions in the background.
/// An optional hook that fails is logged and skipped; a required one aborts.
///
/// Threads aren't frozen while patching: at startup the game's main thread is
/// still suspended, and when attaching the patched functions only run while a
/// window opens or is dragged. Freezing threads risks deadlocking on the heap lock.
///
/// # Safety
/// Every `target` must be the start of the function its `kind` names.
pub unsafe fn install(
    game: Game,
    store: Store,
    verbose: bool,
    specs: &[HookSpec],
) -> Result<Vec<Kind>, String> {
    VERBOSE.store(verbose, Relaxed);
    GAME.set(game).map_err(|_| "hooks are already installed")?;
    STORE
        .set(Mutex::new(store))
        .map_err(|_| "hooks are already installed")?;
    let _ = SEEN.set(Mutex::new(HashSet::new()));

    let mut installed = Vec::new();
    let mut detours = Vec::new();
    for spec in specs {
        let (detour, original) = spec.kind.detour();
        let result = RawDetour::new(spec.target as *const (), detour).and_then(|d| {
            original.store(d.trampoline() as *const () as usize, Relaxed);
            d.enable()?;
            Ok(d)
        });
        match result {
            Ok(d) => {
                detours.push(d);
                installed.push(spec.kind);
            }
            Err(e) if spec.required => return Err(format!("{}: {e}", spec.kind.name())),
            Err(e) => log!("skipping {}: {e}", spec.kind.name()),
        }
    }
    // Never unhook: dropping a RawDetour would restore the original bytes.
    std::mem::forget(detours);

    std::thread::spawn(|| loop {
        std::thread::sleep(FLUSH_INTERVAL);
        flush();
    });
    Ok(installed)
}

/// Write positions.json if anything moved since the last write.
pub fn flush() {
    let Some(store) = STORE.get() else { return };
    let Ok(mut store) = store.try_lock() else {
        return;
    };
    if !store.is_dirty() {
        return;
    }
    match store.save() {
        Ok(()) => log!(
            "saved windows={} path={}",
            store.len(),
            store.path().display()
        ),
        Err(e) => log!("couldn't save {}: {e}", store.path().display()),
    }
}
