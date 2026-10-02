//! Injected into factorio.exe by fwm-launch. Detours `agui::Window::center` so a
//! built-in window opens where the user last dragged that kind of window, and the
//! drag handlers so those positions get recorded.
#![cfg(windows)]

#[macro_use]
mod log;
mod game;
mod hooks;
mod init;
mod screen;

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HMODULE, TRUE};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

/// # Safety
/// Called by the Windows loader with this DLL's module handle.
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module: HMODULE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            DisableThreadLibraryCalls(module);
        }
        DLL_PROCESS_DETACH => hooks::flush(),
        _ => {}
    }
    TRUE
}

/// Run by fwm-launch in a remote thread after loading the DLL. Returns one of
/// [`fwm_core::protocol::code`].
///
/// # Safety
/// `param` must be null or a NUL-terminated UTF-16
/// [`fwm_core::protocol::InitConfig`], and this must run inside factorio.exe.
#[no_mangle]
pub unsafe extern "system" fn fwm_init(param: *mut c_void) -> u32 {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        init::run(param as *const u16)
    }))
    .unwrap_or_else(|_| {
        log!("startup panicked");
        fwm_core::protocol::code::PANIC
    })
}
