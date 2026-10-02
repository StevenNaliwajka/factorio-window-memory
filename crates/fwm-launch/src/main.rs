//! Starts Factorio with fwm_hook.dll injected, or injects it into a running game.
//!
//! Steam launch option: `"D:\path\to\fwm-launch.exe" %command%`
#![windows_subsystem = "windows"]

mod inject;

use fwm_core::args::{self, Command, LaunchOptions};
use fwm_core::protocol::{self, InitConfig};
use fwm_core::steam;
use inject::{Child, OpenedProcess};
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};

const USAGE: &str = "\
fwm-launch: open Factorio's windows where you last dragged them

usage:
  fwm-launch [options] [path\\to\\factorio.exe] [game arguments...]
  fwm-launch [options] --attach [pid]

With no factorio.exe it is found in your Steam library. As a Steam launch
option use:  \"<path>\\fwm-launch.exe\" %command%

options:
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
    unsafe {
        // A GUI-subsystem exe has no console; borrow the terminal's if run from one.
        let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
        if stdout.is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
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
            println!("{USAGE}");
            Ok(0)
        }
        Command::Launch { exe, args, opts } => launch(exe, args, opts),
        Command::Attach { pid, opts } => attach(pid, opts),
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
