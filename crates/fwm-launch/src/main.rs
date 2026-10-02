//! Starts Factorio with fwm_hook.dll injected, injects it into a running game,
//! or, double-clicked, turns the plugin on or off by setting Factorio's Steam
//! launch option to `"<path>\fwm-launch.exe" %command%`.
#![windows_subsystem = "windows"]

mod inject;
mod steam_setup;

use fwm_core::args::{self, Command, LaunchOptions};
use fwm_core::protocol::{self, InitConfig};
use fwm_core::steam;
use inject::{Child, OpenedProcess};
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use steam_setup::Switch;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_YESNO,
};

const TITLE: &str = "Factorio window memory";
/// Whether printed text reaches anyone (a terminal or a pipe); if not, use dialogs.
static HAS_CONSOLE: AtomicBool = AtomicBool::new(false);

const USAGE: &str = "\
fwm-launch: open Factorio's windows where you last dragged them

usage:
  fwm-launch                  turn the plugin on or off (asks first)
  fwm-launch --install        turn it on: Factorio's Steam launch option
                              starts the game through fwm-launch
  fwm-launch --uninstall      turn it off: remove that launch option
  fwm-launch [options] [path\\to\\factorio.exe] [game arguments...]
  fwm-launch [options] --launch
  fwm-launch [options] --attach [pid]

Turning it on or off restarts Steam if it's open, since Steam only saves
launch options when it exits. The launch option it sets is
  \"<path>\\fwm-launch.exe\" %command%

options:
  --launch              start Factorio (found in your Steam library) with the plugin
  --attach [pid]        inject into an already running game
  --fwm-data-dir <dir>  where positions.json and fwm.log go
                        (default %APPDATA%\\Factorio\\window-memory)
  --fwm-dll <path>      plugin DLL (default fwm_hook.dll next to this exe)
  --fwm-verbose         log every window placement and drag
  --fwm-no-wait         exit after injecting instead of waiting for the game
  --fwm-late-inject     inject after the game has started
  --fwm-no-activate     start the game window without focusing it
";

fn main() {
    let has_console = unsafe {
        // A GUI-subsystem exe has no console; borrow the terminal's if run from one.
        let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
        if stdout.is_null() || stdout == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS) != 0
        } else {
            true
        }
    };
    HAS_CONSOLE.store(has_console, Ordering::Relaxed);
    let code = match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("fwm-launch: {message}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32, String> {
    match args::parse(std::env::args_os().skip(1))? {
        Command::Help => {
            show(&USAGE.replace('\n', "\r\n"), MB_ICONINFORMATION);
            Ok(0)
        }
        Command::Toggle => toggle(),
        Command::Install { opts } => switch(Switch::On, &opts),
        Command::Uninstall { opts } => switch(Switch::Off, &opts),
        Command::Launch { exe, args, opts } => launch(exe, args, opts),
        Command::Attach { pid, opts } => attach(pid, opts),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Print if someone can see it, otherwise pop up a dialog.
fn show(message: &str, icon: u32) {
    if HAS_CONSOLE.load(Ordering::Relaxed) {
        println!("{}", message.replace("\r\n", "\n"));
    } else {
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                wide(message).as_ptr(),
                wide(TITLE).as_ptr(),
                MB_OK | icon,
            )
        };
    }
}

fn ask(question: &str) -> bool {
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(question).as_ptr(),
            wide(TITLE).as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

/// Double-clicked: offer to turn the plugin on, or off if it's already on.
fn toggle() -> Result<i32, String> {
    let opts = LaunchOptions::default();
    let Some(root) = steam_setup::steam_root(None) else {
        show("Couldn't find Steam on this PC.", MB_ICONWARNING);
        return Ok(3);
    };
    let question = if steam_setup::is_on(&root) {
        "Factorio window memory is ON.\n\nTurn it off? Factorio will start normally from Steam again.\n\n(If Steam is open, it restarts to save the setting.)"
    } else {
        "Turn on Factorio window memory?\n\nFrom then on, Factorio started from Steam opens its windows where you last dragged them.\n\n(If Steam is open, it restarts to save the setting.)"
    };
    if !ask(question) {
        return Ok(0);
    }
    let switch_to = if steam_setup::is_on(&root) {
        Switch::Off
    } else {
        Switch::On
    };
    switch(switch_to, &opts)
}

/// `--install` / `--uninstall` (and the double-click switch).
fn switch(to: Switch, opts: &LaunchOptions) -> Result<i32, String> {
    let log = LaunchLog::new(opts);
    let Some(root) = steam_setup::steam_root(opts.steam_root.as_deref()) else {
        show("Couldn't find Steam on this PC.", MB_ICONWARNING);
        return Ok(3);
    };
    let launcher = std::env::current_exe().map_err(|e| e.to_string())?;
    if to == Switch::On {
        // The launch option points at this exe, so it needs its DLL beside it.
        dll_path(opts)?;
    }
    match steam_setup::apply(&root, to, &launcher) {
        Ok(lines) => {
            let summary = lines.join("\n");
            log.write(&summary);
            let next = match to {
                Switch::On => "Start Factorio from Steam as usual. Run this again to turn it off.",
                Switch::Off => "Factorio starts normally from Steam now.",
            };
            show(&format!("{summary}\n\n{next}"), MB_ICONINFORMATION);
            Ok(0)
        }
        Err(e) => {
            log.write(&e);
            show(&format!("Nothing was changed:\n\n{e}"), MB_ICONWARNING);
            Ok(3)
        }
    }
}

fn launch(
    exe: Option<PathBuf>,
    game_args: Vec<OsString>,
    opts: LaunchOptions,
) -> Result<i32, String> {
    let exe = match exe {
        Some(exe) => exe,
        None => steam::find_factorio_exe()
            .ok_or("couldn't find factorio.exe; pass its path as the first argument")?,
    };
    let dll = dll_path(&opts)?;
    let log = LaunchLog::new(&opts);
    log.say(&format!(
        "starting {} with {}",
        exe.display(),
        dll.display()
    ));

    // Launched by Steam these are already set; run directly, the game would exit
    // and get relaunched by Steam without the plugin. The child inherits them.
    for name in ["SteamAppId", "SteamGameId"] {
        if std::env::var_os(name).is_none() {
            std::env::set_var(name, steam::FACTORIO_APP_ID);
        }
    }

    let child = Child::spawn_suspended(&exe, &game_args, opts.no_activate)
        .map_err(|e| format!("couldn't start {}: {e}", exe.display()))?;
    let config = InitConfig {
        data_dir: opts.data_dir.clone(),
        attach: false,
        verbose: opts.verbose,
    };
    let late = InitConfig {
        attach: true,
        ..config.clone()
    };

    let outcome = if opts.late_inject {
        child.resume_and_settle();
        inject::inject(child.process(), child.pid(), &dll, &late)
    } else {
        match inject::inject(child.process(), child.pid(), &dll, &config) {
            Err(e) if e.worth_retrying_late() => {
                log.say(&format!(
                    "early injection failed ({e}); retrying once the game is up"
                ));
                child.resume_and_settle();
                inject::inject(child.process(), child.pid(), &dll, &late)
            }
            other => other,
        }
    };
    child.resume();

    let status = match &outcome {
        Ok(()) => {
            log.say(&format!("plugin loaded into pid {}", child.pid()));
            0
        }
        Err(e) => {
            log.say(&format!("plugin not loaded, the game runs unmodified: {e}"));
            e.exit_code()
        }
    };
    if opts.wait {
        Ok(child.wait() as i32)
    } else {
        Ok(status)
    }
}

fn attach(pid: Option<u32>, opts: LaunchOptions) -> Result<i32, String> {
    let pid = match pid {
        Some(pid) => pid,
        None => inject::find_process("factorio.exe")?,
    };
    let dll = dll_path(&opts)?;
    let log = LaunchLog::new(&opts);
    let process = OpenedProcess::open(pid).map_err(|e| e.to_string())?;
    let config = InitConfig {
        data_dir: opts.data_dir.clone(),
        attach: true,
        verbose: opts.verbose,
    };
    match inject::inject(process.handle(), pid, &dll, &config) {
        Ok(()) => {
            log.say(&format!("plugin loaded into running pid {pid}"));
            Ok(0)
        }
        Err(e) => {
            log.say(&format!("couldn't load the plugin into pid {pid}: {e}"));
            Ok(e.exit_code())
        }
    }
}

fn dll_path(opts: &LaunchOptions) -> Result<PathBuf, String> {
    let dll = match &opts.dll {
        Some(dll) => dll.clone(),
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name(protocol::DLL_NAME),
    };
    let dll = std::path::absolute(&dll).map_err(|e| e.to_string())?;
    if !dll.is_file() {
        return Err(format!("{} not found", dll.display()));
    }
    Ok(dll)
}

/// Prints status and appends it to fwm-launch.log, since Steam hides the console.
struct LaunchLog(Option<PathBuf>);

impl LaunchLog {
    fn new(opts: &LaunchOptions) -> LaunchLog {
        LaunchLog(
            opts.data_dir
                .clone()
                .or_else(protocol::default_data_dir)
                .map(|d| d.join("fwm-launch.log")),
        )
    }

    fn say(&self, message: &str) {
        println!("fwm-launch: {message}");
        self.write(message);
    }

    /// Log only; the caller tells the user some other way.
    fn write(&self, message: &str) {
        let Some(path) = &self.0 else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "[{now}] {message}");
        }
    }
}
