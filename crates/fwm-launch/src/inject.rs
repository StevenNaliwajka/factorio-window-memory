//! Classic DLL injection: write the DLL path into the target, run `LoadLibraryW`
//! there in a remote thread, then run the DLL's `fwm_init` export the same way.

use fwm_core::protocol::{self, InitConfig};
use std::cell::Cell;
use std::ffi::{c_void, OsStr, OsString};
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{
    CloseHandle, FreeLibrary, GetLastError, ERROR_BAD_LENGTH, FALSE, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LoadLibraryExW, DONT_RESOLVE_DLL_REFERENCES,
};
use windows_sys::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, CreateRemoteThread, GetExitCodeProcess, GetExitCodeThread, OpenProcess,
    ResumeThread, WaitForInputIdle, WaitForSingleObject, CREATE_SUSPENDED, INFINITE,
    PROCESS_CREATE_THREAD, PROCESS_INFORMATION, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE, STARTF_USESHOWWINDOW, STARTUPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE;

const LOAD_TIMEOUT_MS: u32 = 30_000;
/// Reading the 380 MB factorio.pdb takes a second or two; allow for a slow disk.
const INIT_TIMEOUT_MS: u32 = 180_000;

#[derive(Debug)]
pub enum InjectError {
    Win32 { what: &'static str, code: u32 },
    LoadLibraryFailed,
    ModuleNotFound,
    ExportNotFound,
    Timeout(&'static str),
    Init(u32),
}

impl InjectError {
    fn last(what: &'static str) -> InjectError {
        InjectError::Win32 {
            what,
            code: unsafe { GetLastError() },
        }
    }

    /// Exit code for `--fwm-no-wait`: 10 + the `fwm_init` code, or 2 if the DLL never ran.
    pub fn exit_code(&self) -> i32 {
        match self {
            InjectError::Init(code) => 10 + *code as i32,
            _ => 2,
        }
    }

    /// Injection plumbing failures are worth retrying once the game is running;
    /// an error reported by `fwm_init` itself is not.
    pub fn worth_retrying_late(&self) -> bool {
        !matches!(self, InjectError::Init(_) | InjectError::ExportNotFound)
    }
}

impl fmt::Display for InjectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InjectError::Win32 { what, code } => write!(f, "{what} failed (Windows error {code})"),
            InjectError::LoadLibraryFailed => write!(f, "the game couldn't load the plugin DLL"),
            InjectError::ModuleNotFound => {
                write!(f, "the plugin DLL didn't show up in the game's module list")
            }
            InjectError::ExportNotFound => {
                write!(f, "the DLL has no {} export", protocol::INIT_EXPORT)
            }
            InjectError::Timeout(what) => write!(f, "timed out waiting for {what}"),
            InjectError::Init(code) => write!(
                f,
                "{} (code {code}; see fwm.log)",
                protocol::describe(*code)
            ),
        }
    }
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// A process started suspended by [`Child::spawn_suspended`].
pub struct Child {
    info: PROCESS_INFORMATION,
    resumed: Cell<bool>,
}

impl Child {
    pub fn spawn_suspended(
        exe: &Path,
        args: &[OsString],
        no_activate: bool,
    ) -> std::io::Result<Child> {
        let application = wide(exe.as_os_str());
        let mut command_line = fwm_core::args::build_command_line(exe.as_os_str(), args);
        command_line.push(0);
        unsafe {
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            if no_activate {
                startup.dwFlags = STARTF_USESHOWWINDOW;
                startup.wShowWindow = SW_SHOWNOACTIVATE as u16;
            }
            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                FALSE,
                CREATE_SUSPENDED,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut info,
            );
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Child {
                info,
                resumed: Cell::new(false),
            })
        }
    }

    pub fn process(&self) -> HANDLE {
        self.info.hProcess
    }

    pub fn pid(&self) -> u32 {
        self.info.dwProcessId
    }

    pub fn resume(&self) {
        if !self.resumed.replace(true) {
            unsafe { ResumeThread(self.info.hThread) };
        }
    }

    /// Resume and give the game a moment to finish starting up.
    pub fn resume_and_settle(&self) {
        self.resume();
        unsafe { WaitForInputIdle(self.info.hProcess, LOAD_TIMEOUT_MS) };
    }

    pub fn wait(&self) -> u32 {
        let mut code = 0;
        unsafe {
            WaitForSingleObject(self.info.hProcess, INFINITE);
            GetExitCodeProcess(self.info.hProcess, &mut code);
        }
        code
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.resume();
        unsafe {
            CloseHandle(self.info.hThread);
            CloseHandle(self.info.hProcess);
        }
    }
}

/// An open handle to a process we didn't start.
pub struct OpenedProcess(HANDLE);

impl OpenedProcess {
    pub fn open(pid: u32) -> Result<OpenedProcess, InjectError> {
        let access = PROCESS_CREATE_THREAD
            | PROCESS_QUERY_INFORMATION
            | PROCESS_VM_OPERATION
            | PROCESS_VM_WRITE
            | PROCESS_VM_READ;
        let handle = unsafe { OpenProcess(access, FALSE, pid) };
        if handle.is_null() {
            return Err(InjectError::last("OpenProcess"));
        }
        Ok(OpenedProcess(handle))
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OpenedProcess {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// Memory allocated in another process, freed on drop.
struct RemoteBuffer {
    process: HANDLE,
    address: *mut c_void,
}

impl RemoteBuffer {
    fn new(process: HANDLE, data: &[u16]) -> Result<RemoteBuffer, InjectError> {
        let bytes = std::mem::size_of_val(data);
        unsafe {
            let address = VirtualAllocEx(
                process,
                std::ptr::null(),
                bytes,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if address.is_null() {
                return Err(InjectError::last("VirtualAllocEx"));
            }
            let buffer = RemoteBuffer { process, address };
            if WriteProcessMemory(
                process,
                address,
                data.as_ptr().cast(),
                bytes,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(InjectError::last("WriteProcessMemory"));
            }
            Ok(buffer)
        }
    }
}

impl Drop for RemoteBuffer {
    fn drop(&mut self) {
        unsafe { VirtualFreeEx(self.process, self.address, 0, MEM_RELEASE) };
    }
}

/// Run `start(parameter)` in a new thread in `process` and return its exit code.
fn run_remote(
    process: HANDLE,
    start: usize,
    parameter: *const c_void,
    timeout_ms: u32,
    what: &'static str,
) -> Result<u32, InjectError> {
    unsafe {
        let routine =
            std::mem::transmute::<usize, unsafe extern "system" fn(*mut c_void) -> u32>(start);
        let thread = CreateRemoteThread(
            process,
            std::ptr::null(),
            0,
            Some(routine),
            parameter,
            0,
            std::ptr::null_mut(),
        );
        if thread.is_null() {
            return Err(InjectError::last("CreateRemoteThread"));
        }
        let waited = WaitForSingleObject(thread, timeout_ms);
        let mut code = 0;
        let got_code = GetExitCodeThread(thread, &mut code);
        CloseHandle(thread);
        if waited != WAIT_OBJECT_0 {
            return Err(InjectError::Timeout(what));
        }
        if got_code == 0 {
            return Err(InjectError::last("GetExitCodeThread"));
        }
        Ok(code)
    }
}

/// Base address of the module named `file_name` in process `pid`.
fn find_remote_module(pid: u32, file_name: &str) -> Result<usize, InjectError> {
    for _ in 0..50 {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
            if snapshot == INVALID_HANDLE_VALUE {
                // The module list can be mid-update; Windows says to retry.
                if GetLastError() == ERROR_BAD_LENGTH {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
                return Err(InjectError::last("CreateToolhelp32Snapshot"));
            }
            let mut entry: MODULEENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
            let mut found = None;
            let mut more = Module32FirstW(snapshot, &mut entry) != 0;
            while more {
                let len = entry
                    .szModule
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szModule.len());
                if String::from_utf16_lossy(&entry.szModule[..len]).eq_ignore_ascii_case(file_name)
                {
                    found = Some(entry.modBaseAddr as usize);
                    break;
                }
                more = Module32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            return found.ok_or(InjectError::ModuleNotFound);
        }
    }
    Err(InjectError::ModuleNotFound)
}

/// Offset of `fwm_init` from the DLL's base, found by mapping it here without running it.
fn init_export_offset(dll: &Path) -> Result<usize, InjectError> {
    let path = wide(dll.as_os_str());
    unsafe {
        let module = LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            DONT_RESOLVE_DLL_REFERENCES,
        );
        if module.is_null() {
            return Err(InjectError::last("LoadLibraryExW (reading the DLL)"));
        }
        let name = format!("{}\0", protocol::INIT_EXPORT);
        let export = GetProcAddress(module, name.as_ptr());
        let offset = export.map(|f| f as usize - module as usize);
        FreeLibrary(module);
        offset.ok_or(InjectError::ExportNotFound)
    }
}

/// Load `dll` into `process` and run its `fwm_init` with `config`.
pub fn inject(
    process: HANDLE,
    pid: u32,
    dll: &Path,
    config: &InitConfig,
) -> Result<(), InjectError> {
    let offset = init_export_offset(dll)?;
    let file_name = dll
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(protocol::DLL_NAME);

    let dll_path = RemoteBuffer::new(process, &wide(dll.as_os_str()))?;
    let load_library = unsafe {
        let kernel32 = GetModuleHandleW(wide(OsStr::new("kernel32.dll")).as_ptr());
        GetProcAddress(kernel32, c"LoadLibraryW".as_ptr().cast())
            .ok_or(InjectError::LoadLibraryFailed)? as usize
    };
    // kernel32 sits at the same address in every process for this boot.
    let loaded = run_remote_keeping(dll_path, |buffer| {
        run_remote(
            process,
            load_library,
            buffer,
            LOAD_TIMEOUT_MS,
            "the DLL to load",
        )
    })?;
    if loaded == 0 {
        return Err(InjectError::LoadLibraryFailed);
    }
    let base = find_remote_module(pid, file_name)?;

    let config = RemoteBuffer::new(process, &wide(OsStr::new(&config.encode())))?;
    match run_remote_keeping(config, |buffer| {
        run_remote(
            process,
            base + offset,
            buffer,
            INIT_TIMEOUT_MS,
            "the plugin to start",
        )
    })? {
        protocol::code::OK => Ok(()),
        code => Err(InjectError::Init(code)),
    }
}

/// If the remote thread timed out it may still read `buffer`, so leak it rather than free it.
fn run_remote_keeping(
    buffer: RemoteBuffer,
    run: impl FnOnce(*const c_void) -> Result<u32, InjectError>,
) -> Result<u32, InjectError> {
    let result = run(buffer.address);
    if matches!(result, Err(InjectError::Timeout(_))) {
        std::mem::forget(buffer);
    }
    result
}

/// The pid of the only running `exe_name`.
pub fn find_process(exe_name: &str) -> Result<u32, String> {
    let mut pids = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!(
                "couldn't list processes (Windows error {})",
                GetLastError()
            ));
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe_name) {
                pids.push(entry.th32ProcessID);
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    match pids.as_slice() {
        [pid] => Ok(*pid),
        [] => Err(format!("{exe_name} isn't running")),
        many => Err(format!(
            "several {exe_name} processes are running ({many:?}); pass --attach <pid>"
        )),
    }
}
