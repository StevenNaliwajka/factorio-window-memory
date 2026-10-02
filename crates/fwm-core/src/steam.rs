//! Finding Steam, its accounts' config files, and factorio.exe.

use std::path::{Path, PathBuf};

/// Factorio's Steam app id. Steam puts it in `SteamAppId`/`SteamGameId` when it
/// launches the game; without them the Steam build exits at once and asks Steam
/// to relaunch it (which would drop the plugin).
pub const FACTORIO_APP_ID: &str = "427520";

pub const STEAM_ROOTS: &[&str] = &[r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"];
const FACTORIO_EXE: &str = r"steamapps\common\Factorio\bin\x64\factorio.exe";

/// `(account id, path)` of every account's `localconfig.vdf` under a Steam install.
pub fn localconfig_files(steam_root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(steam_root.join("userdata")) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path().join(r"config\localconfig.vdf");
            path.is_file()
                .then(|| (e.file_name().to_string_lossy().into_owned(), path))
        })
        .collect();
    files.sort();
    files
}

/// The `"path"` entries of a `libraryfolders.vdf`.
pub fn library_paths_from_vdf(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let mut tokens = line.split('"').filter(|t| !t.trim().is_empty());
            if tokens.next()? != "path" {
                return None;
            }
            Some(PathBuf::from(tokens.next()?.replace(r"\\", r"\")))
        })
        .collect()
}

/// `%FWM_FACTORIO_EXE%`, else the first Steam library that has Factorio installed.
pub fn find_factorio_exe() -> Option<PathBuf> {
    if let Some(exe) = std::env::var_os("FWM_FACTORIO_EXE") {
        return Some(exe.into());
    }
    let mut libraries = Vec::new();
    for root in STEAM_ROOTS.iter().map(Path::new) {
        libraries.push(root.to_path_buf());
        if let Ok(text) = std::fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf")) {
            libraries.extend(library_paths_from_vdf(&text));
        }
    }
    libraries
        .into_iter()
        .map(|l| l.join(FACTORIO_EXE))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_library_paths() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"529473995"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        assert_eq!(
            library_paths_from_vdf(vdf),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary")
            ]
        );
    }
}
