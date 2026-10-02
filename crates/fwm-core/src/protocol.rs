//! What the launcher and the hook DLL agree on.

use std::path::PathBuf;

pub const DLL_NAME: &str = "fwm_hook.dll";
/// Export the launcher calls in a remote thread once the DLL is loaded.
pub const INIT_EXPORT: &str = "fwm_init";

/// Return values of `fwm_init`.
pub mod code {
    pub const OK: u32 = 0;
    pub const PANIC: u32 = 1;
    pub const DATA_DIR: u32 = 2;
    pub const PDB_READ: u32 = 3;
    pub const PDB_MISMATCH: u32 = 4;
    pub const SYMBOLS_MISSING: u32 = 5;
    pub const HOOK_FAILED: u32 = 6;
    pub const ALREADY_LOADED: u32 = 7;
}

pub fn describe(c: u32) -> &'static str {
    match c {
        code::OK => "ok",
        code::PANIC => "the plugin crashed while starting",
        code::DATA_DIR => "couldn't create the data folder",
        code::PDB_READ => "couldn't read factorio.pdb",
        code::PDB_MISMATCH => "factorio.pdb doesn't match factorio.exe",
        code::SYMBOLS_MISSING => "this Factorio version lacks a function the plugin needs",
        code::HOOK_FAILED => "couldn't patch a game function",
        code::ALREADY_LOADED => "the plugin is already loaded in this process",
        _ => "unknown error",
    }
}

/// Passed to `fwm_init` as NUL-terminated UTF-16 `key=value` lines.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InitConfig {
    pub data_dir: Option<PathBuf>,
    /// Injected into an already-running game rather than at startup.
    pub attach: bool,
    /// Log every window placement and drag, not just the first per window class.
    pub verbose: bool,
}

impl InitConfig {
    pub fn encode(&self) -> String {
        let mut out = String::new();
        if let Some(dir) = &self.data_dir {
            out.push_str(&format!("data_dir={}\n", dir.display()));
        }
        out.push_str(&format!("attach={}\n", self.attach as u8));
        out.push_str(&format!("verbose={}\n", self.verbose as u8));
        out
    }

    pub fn decode(text: &str) -> InitConfig {
        let mut config = InitConfig::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "data_dir" if !value.is_empty() => config.data_dir = Some(PathBuf::from(value)),
                "attach" => config.attach = value.trim() == "1",
                "verbose" => config.verbose = value.trim() == "1",
                _ => {}
            }
        }
        config
    }
}

/// `%FWM_DATA_DIR%`, else `%APPDATA%\Factorio\window-memory`.
pub fn default_data_dir() -> Option<PathBuf> {
    std::env::var_os("FWM_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("APPDATA")
                .map(|a| PathBuf::from(a).join("Factorio").join("window-memory"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips() {
        let config = InitConfig {
            data_dir: Some(PathBuf::from(
                r"C:\Users\x\AppData\Roaming\Factorio\window-memory",
            )),
            attach: true,
            verbose: false,
        };
        assert_eq!(InitConfig::decode(&config.encode()), config);
    }

    #[test]
    fn empty_or_unknown_input_gives_defaults() {
        assert_eq!(InitConfig::decode(""), InitConfig::default());
        assert_eq!(
            InitConfig::decode("future_key=1\nnonsense"),
            InitConfig::default()
        );
    }

    #[test]
    fn paths_containing_equals_signs_survive() {
        let config = InitConfig {
            data_dir: Some(PathBuf::from(r"D:\a=b")),
            ..Default::default()
        };
        assert_eq!(
            InitConfig::decode(&config.encode()).data_dir,
            config.data_dir
        );
    }
}
