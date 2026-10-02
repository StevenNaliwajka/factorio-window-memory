//! Size of the game window's client area, used to keep restored windows on screen.

use fwm_core::geometry::Size;
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
};

static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

pub fn client_size() -> Option<Size> {
    unsafe {
        let mut hwnd = MAIN_WINDOW.load(Relaxed) as HWND;
        if hwnd.is_null() || IsWindow(hwnd) == 0 {
            hwnd = find_main_window()?;
            MAIN_WINDOW.store(hwnd as isize, Relaxed);
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let size = Size {
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        };
        (size.width > 0 && size.height > 0).then_some(size)
    }
}

/// The largest visible top-level window of this process.
unsafe fn find_main_window() -> Option<HWND> {
    struct Search {
        pid: u32,
        best: HWND,
        area: i64,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == search.pid && IsWindowVisible(hwnd) != 0 {
            let mut rect: RECT = std::mem::zeroed();
            if GetClientRect(hwnd, &mut rect) != 0 {
                let area = (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64;
                if area > search.area {
                    search.area = area;
                    search.best = hwnd;
                }
            }
        }
        TRUE
    }

    let mut search = Search {
        pid: GetCurrentProcessId(),
        best: std::ptr::null_mut(),
        area: 0,
    };
    EnumWindows(Some(visit), &mut search as *mut Search as LPARAM);
    (!search.best.is_null()).then_some(search.best)
}
