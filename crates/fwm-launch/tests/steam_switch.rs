//! `--install` / `--uninstall` against a fake Steam folder (no steam.exe runs
//! there, so nothing is closed or restarted): account 111 has played Factorio
//! with its own launch option, account 222 never has.

use std::path::{Path, PathBuf};
use std::process::Command;

const WITH_FACTORIO: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"427520\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1790000000\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"--disable-audio\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
const WITHOUT_FACTORIO: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"570\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

fn fake_steam(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("fwm-steam-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (account, text) in [("111", WITH_FACTORIO), ("222", WITHOUT_FACTORIO)] {
        let dir = root.join("userdata").join(account).join("config");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("localconfig.vdf"), text).unwrap();
    }
    root
}

fn config(root: &Path, account: &str) -> String {
    std::fs::read_to_string(
        root.join("userdata")
            .join(account)
            .join(r"config\localconfig.vdf"),
    )
    .unwrap()
}

fn run(root: &Path, flag: &str) -> (Option<i32>, String) {
    let data = root.join("fwm-data");
    let output = Command::new(env!("CARGO_BIN_EXE_fwm-launch"))
        .arg(flag)
        .arg("--fwm-steam-root")
        .arg(root)
        .arg("--fwm-data-dir")
        .arg(&data)
        // A stand-in Steam restarted by fwm-launch inherits this and stays up 30 s.
        .env("FWM_DUMMY_MS", "30000")
        .output()
        .expect("run fwm-launch");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn install_then_uninstall_round_trips() {
    let root = fake_steam("roundtrip");
    let launcher = std::path::absolute(env!("CARGO_BIN_EXE_fwm-launch")).unwrap();
    let expected = format!(
        "\"\\\"{}\\\" %command% --disable-audio\"",
        launcher.display().to_string().replace('\\', "\\\\")
    );

    let (code, out) = run(&root, "--install");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("Steam account 111: turned on"), "{out}");
    assert!(
        config(&root, "111").contains(&expected),
        "{}",
        config(&root, "111")
    );
    assert_eq!(
        config(&root, "222"),
        WITHOUT_FACTORIO,
        "an account without Factorio is left alone"
    );
    assert!(root
        .join(r"userdata\111\config\localconfig.vdf.fwm-backup")
        .is_file());
    assert!(!root
        .join(r"userdata\222\config\localconfig.vdf.fwm-backup")
        .exists());

    let (code, out) = run(&root, "--install");
    assert_eq!(code, Some(0));
    assert!(out.contains("Already on"), "{out}");

    let (code, out) = run(&root, "--uninstall");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("Steam account 111: turned off"), "{out}");
    assert_eq!(
        config(&root, "111"),
        WITH_FACTORIO,
        "turning off restores the original exactly"
    );

    let (_, out) = run(&root, "--uninstall");
    assert!(out.contains("Already off"), "{out}");
}

#[test]
fn install_with_no_accounts_changes_nothing() {
    let root = std::env::temp_dir().join(format!("fwm-steam-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("userdata")).unwrap();
    let (code, out) = run(&root, "--install");
    assert_eq!(code, Some(3), "{out}");
    assert!(out.contains("no Steam accounts found"), "{out}");
}

/// With "Steam" running (fwm-dummy copied in as steam.exe), turning on must
/// close it, edit, start it again, and return straight away: the restarted
/// Steam must not inherit our output pipe, or a caller reading it would wait
/// until Steam exits.
#[test]
fn restarting_steam_does_not_hold_our_output_open() {
    if Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq factorio.exe", "/NH"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("factorio.exe"))
    {
        eprintln!("skipping: Factorio is running, so the switch would refuse to restart Steam");
        return;
    }
    let root = fake_steam("restart");
    let steam = root.join("steam.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_fwm-dummy"), &steam).unwrap();
    let mut running = Command::new(&steam).arg("1000").spawn().unwrap();

    let (code, out) = run(&root, "--install");
    running.wait().unwrap();
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("Steam was restarted"), "{out}");

    // The restarted stand-in stays up 30 s; if our output had been held open,
    // run() would only have returned after it exited. Count it, then clean up.
    let restarted = Command::new("powershell")
        .args(["-NoProfile", "-Command"])
        .arg(format!(
            "$p = @(Get-CimInstance Win32_Process -Filter \"Name='steam.exe'\" | Where-Object {{ $_.ExecutablePath -eq '{}' }}); \
             $p.Count; $p | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}",
            steam.display()
        ))
        .output()
        .unwrap();
    let count: u32 = String::from_utf8_lossy(&restarted.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(
        count, 1,
        "the restarted stand-in Steam should still be running when we return"
    );
}
