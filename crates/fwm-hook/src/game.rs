//! Calling into Factorio's GUI code and reading its objects.
//!
//! Layouts confirmed from factorio.pdb: `agui::Window` derives from `agui::Frame`
//! derives from `agui::Widget`, all at offset 0, so a window pointer is a widget
//! pointer. `agui::Point` is `{i32 x, y}`, `agui::Rectangle` is `{i32 x, y, width, height}`.

use fwm_core::geometry::{Point, Rect, Size};
use fwm_core::rtti::class_name_from_type_descriptor;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

pub type Widget = c_void;

// MSVC x64 member functions: `this` in rcx; a returned class goes through a hidden
// pointer passed right after `this`.
type GetLocationFn = unsafe extern "C" fn(*const Widget) -> *const Point;
type SetLocationFn = unsafe extern "C" fn(*mut Widget, i32, i32);
type GetAbsoluteRectangleFn = unsafe extern "C" fn(*const Widget, *mut Rect) -> *mut Rect;
type GetDragTargetFn = unsafe extern "C" fn(*mut Widget) -> *mut Widget;

pub struct Game {
    image_start: usize,
    image_end: usize,
    get_location: GetLocationFn,
    set_location: SetLocationFn,
    get_absolute_rectangle: GetAbsoluteRectangleFn,
    /// vtable index of `agui::Widget::getDragTarget`, if it could be found.
    drag_target_slot: Option<usize>,
    /// vtable address -> class name, so RTTI is only parsed once per class.
    class_names: Mutex<HashMap<usize, Option<Arc<str>>>>,
}

impl Game {
    /// # Safety
    /// The addresses must be the named functions inside the image at `base`.
    pub unsafe fn new(
        base: usize,
        image_size: usize,
        get_location: usize,
        set_location: usize,
        get_absolute_rectangle: usize,
        drag_target_slot: Option<usize>,
    ) -> Game {
        Game {
            image_start: base,
            image_end: base + image_size,
            get_location: std::mem::transmute::<usize, GetLocationFn>(get_location),
            set_location: std::mem::transmute::<usize, SetLocationFn>(set_location),
            get_absolute_rectangle: std::mem::transmute::<usize, GetAbsoluteRectangleFn>(
                get_absolute_rectangle,
            ),
            drag_target_slot,
            class_names: Mutex::new(HashMap::new()),
        }
    }

    fn in_image(&self, address: usize) -> bool {
        (self.image_start..self.image_end).contains(&address)
    }

    pub unsafe fn location(&self, widget: *const Widget) -> Point {
        *(self.get_location)(widget)
    }

    pub unsafe fn set_location(&self, widget: *mut Widget, p: Point) {
        (self.set_location)(widget, p.x, p.y)
    }

    pub unsafe fn size(&self, widget: *const Widget) -> Size {
        let mut rect = Rect::default();
        (self.get_absolute_rectangle)(widget, &mut rect);
        Size {
            width: rect.width,
            height: rect.height,
        }
    }

    /// The window a widget drags when grabbed (title bars point at their window), or null.
    pub unsafe fn drag_target(&self, widget: *mut Widget) -> *mut Widget {
        let Some(slot) = self.drag_target_slot else {
            return std::ptr::null_mut();
        };
        let vtable = *(widget as *const usize);
        if !self.in_image(vtable) {
            return std::ptr::null_mut();
        }
        let function = *(vtable as *const usize).add(slot);
        if !self.in_image(function) {
            return std::ptr::null_mut();
        }
        std::mem::transmute::<usize, GetDragTargetFn>(function)(widget)
    }

    /// The most-derived C++ class of `widget`, e.g. `InventoryGui`. This is the
    /// key windows are remembered by.
    pub unsafe fn class_name(&self, widget: *const Widget) -> Option<Arc<str>> {
        let vtable = *(widget as *const usize);
        let mut cache = self.class_names.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .entry(vtable)
            .or_insert_with(|| {
                self.rtti_type_name(vtable)
                    .and_then(|raw| class_name_from_type_descriptor(&raw))
                    .map(Arc::from)
            })
            .clone()
    }

    /// MSVC x64 RTTI: vtable[-1] is a CompleteObjectLocator
    /// `{u32 signature = 1, offset, cd_offset, type_descriptor_rva, class_descriptor_rva, self_rva}`;
    /// the TypeDescriptor's decorated name starts 16 bytes in.
    unsafe fn rtti_type_name(&self, vtable: usize) -> Option<String> {
        if !self.in_image(vtable) || !self.in_image(vtable - 8) {
            return None;
        }
        let locator = *((vtable - 8) as *const usize);
        if !self.in_image(locator) || !self.in_image(locator + 24) {
            return None;
        }
        let fields = locator as *const u32;
        if *fields != 1 || self.image_start + *fields.add(5) as usize != locator {
            return None;
        }
        let name = self.image_start + *fields.add(3) as usize + 16;
        let mut bytes = Vec::with_capacity(64);
        for i in 0..512 {
            if !self.in_image(name + i) {
                return None;
            }
            match *((name + i) as *const u8) {
                0 => return String::from_utf8(bytes).ok(),
                b => bytes.push(b),
            }
        }
        None
    }
}

/// Index of `function` in the vtable at `vtable`, scanning at most `limit` entries.
/// The first match wins: entries past the end of this vtable belong to others.
///
/// # Safety
/// `vtable` must point at a vtable inside the image `[image_start, image_end)`.
pub unsafe fn find_vtable_slot(
    vtable: usize,
    function: usize,
    limit: usize,
    image_start: usize,
    image_end: usize,
) -> Option<usize> {
    (0..limit)
        .map(|i| *(vtable as *const usize).add(i))
        .take_while(|entry| (image_start..image_end).contains(entry))
        .position(|entry| entry == function)
}
