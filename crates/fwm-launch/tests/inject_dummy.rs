//! Injects the real fwm_hook.dll into fwm-dummy.exe. The dummy has a valid PDB but
//! none of Factorio's functions, so a working pipeline ends with `fwm_init`
//! reporting SYMBOLS_MISSING (launcher exit code 10 + 5). That proves the DLL got
//! loaded, its export was found and called, the exe/PDB match check passed, and
//! the result made it back.
//!
//! Needs the DLL built first: `cargo build --workspace` (scripts\test.ps1 does this).

use fwm_core::protocol::code;
use std::path::{Path, PathBuf};
use std::process::Command;

fn hook_dll() -> PathBuf {
    let dll = Path::new(env!("CARGO_BIN_EXE_fwm-launch")).with_file_name("fwm_hook.dll");
    assert!(
        dll.is_file(),
        "{} missing; run `cargo build --workspace` first",
        dll.display()
    );
    dll
}

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fwm-inject-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn run_launcher(extra: &[&str], data: &Path) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_fwm-launch"))
        .args(["--fwm-no-wait", "--fwm-dll"])
        .arg(hook_dll())
        .arg("--fwm-data-dir")
        .arg(data)
        .args(extra)
        .arg(env!("CARGO_BIN_EXE_fwm-dummy"))
        .arg("4000")
        .output()
        .expect("run fwm-launch");
    let log = std::fs::read_to_string(data.join("fwm.log")).unwrap_or_default();
    (output.status.code(), log)
}

fn assert_reached_symbol_lookup(code: Option<i32>, log: &str) {
    assert_eq!(
        code,
        Some(10 + code::SYMBOLS_MISSING as i32),
        "launcher exit code; fwm.log:\n{log}"
    );
    assert!(
        log.contains("loaded at game start") || log.contains("attached to a running game"),
        "{log}"
    );
    assert!(
        log.contains("fwm_dummy.pdb"),
        "should have found the dummy's PDB:\n{log}"
    );
    assert!(
        log.contains("missing symbols: ?center@Window@agui@@AEAAXXZ"),
        "{log}"
    );
}

#[test]
fn injects_at_startup() {
    let data = data_dir("early");
    let (code, log) = run_launcher(&[], &data);
    assert_reached_symbol_lookup(code, &log);
    assert!(log.contains("loaded at game start"), "{log}");
    let launch_log = std::fs::read_to_string(data.join("fwm-launch.log")).unwrap();
    assert!(
        !launch_log.contains("retrying"),
        "early injection should work without the fallback:\n{launch_log}"
    );
}

#[test]
fn injects_after_startup() {
    let data = data_dir("late");
    let (code, log) = run_launcher(&["--fwm-late-inject"], &data);
    assert_reached_symbol_lookup(code, &log);
    assert!(log.contains("attached to a running game"), "{log}");
}

#[test]
fn missing_dll_is_reported() {
    let output = Command::new(env!("CARGO_BIN_EXE_fwm-launch"))
        .args(["--fwm-no-wait", "--fwm-dll", r"C:\nope\fwm_hook.dll"])
        .arg(env!("CARGO_BIN_EXE_fwm-dummy"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not found"));
}
