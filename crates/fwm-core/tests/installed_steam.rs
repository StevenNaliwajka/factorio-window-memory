//! Read-only checks against this machine's real Steam config files: the editor
//! must reproduce each `localconfig.vdf` byte for byte, and turning the plugin
//! on then off must give back the original text. Nothing is written to disk.
//! Skips when Steam isn't installed.

use fwm_core::{launch_option, steam, vdf};
use std::path::{Path, PathBuf};

#[test]
fn real_localconfig_files_round_trip() {
    let files: Vec<(String, PathBuf)> = steam::STEAM_ROOTS
        .iter()
        .flat_map(|root| steam::localconfig_files(Path::new(root)))
        .collect();
    if files.is_empty() {
        eprintln!("skipping: no Steam accounts found");
        return;
    }
    let launcher = PathBuf::from(r"C:\Some Folder\fwm-launch.exe");
    for (account, path) in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed = vdf::parse(&text).unwrap_or_else(|e| panic!("account {account}: {e}"));
        assert!(
            vdf::serialize(&parsed) == text,
            "account {account}: not reproduced byte for byte"
        );

        let before = launch_option::read(&text).unwrap();
        let Some(on) =
            launch_option::edit(&text, |c| launch_option::with_launcher(c, &launcher)).unwrap()
        else {
            panic!("account {account}: turning on changed nothing");
        };
        assert!(launch_option::has_launcher(
            &launch_option::read(&on).unwrap().launch_options
        ));
        let off = launch_option::edit(&on, launch_option::without_launcher)
            .unwrap()
            .unwrap_or(on.clone());
        if !launch_option::has_launcher(&before.launch_options) {
            assert!(
                off == text || !before.known,
                "account {account}: on then off didn't restore the original"
            );
        }
    }
}
