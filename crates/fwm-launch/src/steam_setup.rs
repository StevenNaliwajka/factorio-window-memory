//! Turning the plugin on or off for good: Factorio's Steam launch option, in
//! every Steam account on this PC that has Factorio. Steam keeps its config in
//! memory and writes `localconfig.vdf` when it exits, so it is closed before the
//! edit and reopened after.

use fwm_core::launch_option;
use fwm_core::steam;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, FALSE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, OpenProcess, QueryFullProcessImageNameW, CREATE_NEW_PROCESS_GROUP,
    DETACHED_PROCESS, PROCESS_INFORMATION, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    STARTUPINFOW,
};

/// Steam's own processes; anything else Steam started is a game.
const STEAM_HELPERS: &[&str] = &[
    "steamwebhelper.exe",
    "steamservice.exe",
    "steamerrorreporter.exe",
    "steamerrorreporter64.exe",
    "gameoverlayui.exe",
    "gameoverlayui64.exe",
    "steamsysinfo.exe",
];
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
}

/// `--fwm-steam-root`, else Steam's registered install folder, else the default ones.
pub fn steam_root(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = explicit {
        return Some(root.to_path_buf());
    }
    registered_steam_path()
        .into_iter()
        .chain(steam::STEAM_ROOTS.iter().map(PathBuf::from))
        .find(|root| root.join("userdata").is_dir())
}

fn registered_steam_path() -> Option<PathBuf> {
    let key: Vec<u16> = OsString::from(r"Software\Valve\Steam")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = OsString::from("SteamPath")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut buffer = vec![0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return None;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let path = OsString::from_wide(&buffer[..len])
        .to_string_lossy()
        .replace('/', "\\");
    Some(PathBuf::from(path))
}

/// Whether any account's Factorio launch option goes through fwm-launch.
pub fn is_on(root: &Path) -> bool {
    steam::localconfig_files(root).iter().any(|(_, path)| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| launch_option::read(&text).ok())
            .is_some_and(|o| launch_option::has_launcher(&o.launch_options))
    })
}

/// Accounts to change: for On, those that have Factorio (or every account if
/// none has run it yet); for Off, those whose launch option uses fwm-launch.
fn targets(root: &Path, switch: Switch) -> Vec<(String, PathBuf)> {
    let files = steam::localconfig_files(root);
    let read = |path: &PathBuf| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| launch_option::read(&t).ok())
    };
    let matching: Vec<(String, PathBuf)> = files
        .iter()
        .filter(|(_, path)| {
            read(path).is_some_and(|o| match switch {
                Switch::On => o.known,
                Switch::Off => launch_option::has_launcher(&o.launch_options),
            })
        })
        .cloned()
        .collect();
    if matching.is_empty() && switch == Switch::On {
        files
    } else {
        matching
    }
}

fn change(text: &str, switch: Switch, launcher: &Path) -> Result<Option<String>, String> {
    match switch {
        Switch::On => launch_option::edit(text, |current| {
            launch_option::with_launcher(current, launcher)
        }),
        Switch::Off => launch_option::edit(text, launch_option::without_launcher),
    }
}

/// Set the switch. Returns one line per account for the user.
pub fn apply(root: &Path, switch: Switch, launcher: &Path) -> Result<Vec<String>, String> {
    if steam::localconfig_files(root).is_empty() {
        return Err(format!(
            "no Steam accounts found under {}; sign in to Steam once first",
            root.display()
        ));
    }
    let accounts = targets(root, switch);
    // Dry run first, so nothing restarts Steam when there's nothing to do.
    let pending: Vec<&(String, PathBuf)> = accounts
        .iter()
        .filter(|(_, path)| {
            std::fs::read_to_string(path)
                .ok()
                .is_some_and(|t| !matches!(change(&t, switch, launcher), Ok(None)))
        })
        .collect();
    let word = if switch == Switch::On { "on" } else { "off" };
    if pending.is_empty() {
        return Ok(vec![format!("Already {word}; nothing to change.")]);
    }

    let steam_was_running = match steam_process(root) {
        Some(pid) => {
            ensure_no_game_running(pid)?;
            shut_down_steam(root)?;
            true
        }
        None => false,
    };

    // Re-read now: Steam rewrites these files as it exits.
    let mut lines = Vec::new();
    let mut failures = 0;
    for (account, path) in &pending {
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| change(&text, switch, launcher))
            .and_then(|new| match new {
                Some(new) => write_with_backup(path, &new).map(|_| true),
                None => Ok(false),
            });
        match result {
            Ok(true) => lines.push(format!("Steam account {account}: turned {word}")),
            Ok(false) => lines.push(format!("Steam account {account}: already {word}")),
            Err(e) => {
                failures += 1;
                lines.push(format!("Steam account {account}: not changed ({e})"));
            }
        }
    }

    if steam_was_running {
        start_steam(root);
        lines.push("Steam was restarted.".to_owned());
    }
    if failures == pending.len() {
        return Err(lines.join("\n"));
    }
    Ok(lines)
}

/// Keep the first original as `localconfig.vdf.fwm-backup`, then replace atomically.
fn write_with_backup(path: &Path, text: &str) -> Result<(), String> {
    let backup = path.with_extension("vdf.fwm-backup");
    if !backup.exists() {
        std::fs::copy(path, &backup).map_err(|e| format!("couldn't back up: {e}"))?;
    }
    let temp = path.with_extension("vdf.fwm-tmp");
    std::fs::write(&temp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}

struct ProcessInfo {
    pid: u32,
    parent: u32,
    name: String,
}

fn processes() -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return out;
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
            out.push(ProcessInfo {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase(),
            });
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    out
}

fn image_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        (ok != 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

fn same_path(a: &str, b: &Path) -> bool {
    let normalize = |s: &str| {
        s.replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    normalize(a) == normalize(&b.to_string_lossy())
}

/// The running steam.exe that belongs to the Steam install at `root`, if any.
fn steam_process(root: &Path) -> Option<u32> {
    let exe = root.join("steam.exe");
    processes()
        .into_iter()
        .filter(|p| p.name == "steam.exe")
        .find(|p| image_path(p.pid).is_some_and(|path| same_path(&path, &exe)))
        .map(|p| p.pid)
}

fn ensure_no_game_running(steam_pid: u32) -> Result<(), String> {
    let all = processes();
    if all.iter().any(|p| p.name == "factorio.exe") {
        return Err("Factorio is running. Close it first, then try again.".into());
    }
    if let Some(game) = all
        .iter()
        .find(|p| p.parent == steam_pid && !STEAM_HELPERS.contains(&p.name.as_str()))
    {
        return Err(format!(
            "A game is running from Steam ({}). Close it first, since Steam has to restart to save the setting.",
            game.name
        ));
    }
    Ok(())
}

fn shut_down_steam(root: &Path) -> Result<(), String> {
    detached(&root.join("steam.exe"), &["-shutdown"])
        .map_err(|e| format!("couldn't ask Steam to close: {e}"))?;
    let started = Instant::now();
    while steam_process(root).is_some() {
        if started.elapsed() > SHUTDOWN_TIMEOUT {
            return Err(
                "Steam didn't close within a minute; close it yourself and try again.".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // Let it finish writing its files.
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn start_steam(root: &Path) {
    let _ = detached(&root.join("steam.exe"), &[]);
}

/// Start a process that outlives us. It must not inherit any handles:
/// `std::process::Command` always inherits, so a restarted Steam would keep our
/// stdout pipe open and whoever reads it (install.ps1) would wait until Steam exits.
fn detached(exe: &Path, args: &[&str]) -> std::io::Result<()> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let mut command_line = fwm_core::args::build_command_line(exe.as_os_str(), &args);
    command_line.push(0);
    let application: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let directory: Option<Vec<u16>> = exe
        .parent()
        .map(|d| d.as_os_str().encode_wide().chain(Some(0)).collect());
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            FALSE,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(),
            directory.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            &startup,
            &mut info,
        );
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    Ok(())
}
