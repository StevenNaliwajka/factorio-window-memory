//! The Factorio functions the hook uses, by MSVC-mangled name. They are looked up
//! in factorio.pdb on every launch, so game patches only break the hook if Wube
//! renames or changes the signature of one of these.

/// `private: void agui::Window::center()` - every built-in window calls this to centre itself.
pub const WINDOW_CENTER: &str = "?center@Window@agui@@AEAAXXZ";
/// `virtual agui::Point const& agui::Widget::getLocation() const`
pub const WIDGET_GET_LOCATION: &str = "?getLocation@Widget@agui@@UEBAAEBVPoint@2@XZ";
/// `virtual void agui::Widget::setLocation(int, int)`
pub const WIDGET_SET_LOCATION: &str = "?setLocation@Widget@agui@@UEAAXHH@Z";
/// `agui::Rectangle agui::Widget::getAbsoluteRectangle() const`
pub const WIDGET_GET_ABSOLUTE_RECTANGLE: &str =
    "?getAbsoluteRectangle@Widget@agui@@QEBA?AVRectangle@2@XZ";

/// `virtual bool agui::Window::mouseDrag(agui::MouseEvent const&)`
pub const WINDOW_MOUSE_DRAG: &str = "?mouseDrag@Window@agui@@UEAA_NAEBVMouseEvent@2@@Z";
/// Title bars are labels/layouts/empty widgets whose drag target is their window.
pub const WIDGET_MOUSE_DRAG: &str = "?mouseDrag@Widget@agui@@UEAA_NAEBVMouseEvent@2@@Z";
pub const LABEL_MOUSE_DRAG: &str = "?mouseDrag@Label@agui@@UEAA_NAEBVMouseEvent@2@@Z";
pub const LAYOUT_MOUSE_DRAG: &str = "?mouseDrag@Layout@agui@@UEAA_NAEBVMouseEvent@2@@Z";
pub const EMPTY_WIDGET_MOUSE_DRAG: &str = "?mouseDrag@EmptyWidget@agui@@UEAA_NAEBVMouseEvent@2@@Z";

/// `virtual agui::Window* agui::Frame::getDragTarget()`, used to find the vtable slot.
pub const FRAME_GET_DRAG_TARGET: &str = "?getDragTarget@Frame@agui@@UEAAPEAVWindow@2@XZ";
/// `agui::Frame::vftable`
pub const FRAME_VFTABLE: &str = "??_7Frame@agui@@6B@";

/// Without these the hook can't do anything useful.
pub const REQUIRED: &[&str] = &[
    WINDOW_CENTER,
    WIDGET_GET_LOCATION,
    WIDGET_SET_LOCATION,
    WIDGET_GET_ABSOLUTE_RECTANGLE,
    WINDOW_MOUSE_DRAG,
];

/// Nice to have: these catch drags that start on a title bar rather than the window itself.
pub const OPTIONAL: &[&str] = &[
    WIDGET_MOUSE_DRAG,
    LABEL_MOUSE_DRAG,
    LAYOUT_MOUSE_DRAG,
    EMPTY_WIDGET_MOUSE_DRAG,
    FRAME_GET_DRAG_TARGET,
    FRAME_VFTABLE,
];

/// Functions that get a detour. Each must have an address no other symbol shares,
/// otherwise the linker folded identical code and patching it would hit other functions too.
pub const HOOKED: &[&str] = &[
    WINDOW_CENTER,
    WINDOW_MOUSE_DRAG,
    WIDGET_MOUSE_DRAG,
    LABEL_MOUSE_DRAG,
    LAYOUT_MOUSE_DRAG,
    EMPTY_WIDGET_MOUSE_DRAG,
];

/// Window classes too generic to remember: plain `agui::Window` covers unrelated
/// dialogs (the loading box, confirmation prompts), so one saved spot would move them all.
pub const GENERIC_WINDOW_CLASSES: &[&str] = &["agui::Window"];

pub fn is_generic_window_class(class: &str) -> bool {
    GENERIC_WINDOW_CLASSES.contains(&class)
}

pub fn all() -> impl Iterator<Item = &'static str> {
    REQUIRED.iter().chain(OPTIONAL).copied()
}
