//! Factorio's Steam launch option is the on/off switch. Turning the plugin on
//! puts `"<path>\fwm-launch.exe" %command%` in front of whatever the user already
//! had there; turning it off takes exactly that back out.

use crate::steam::FACTORIO_APP_ID;
use crate::vdf::{self, Pairs, Value};
use std::path::Path;

const LAUNCHER_FILE: &str = "fwm-launch.exe";
const COMMAND: &str = "%command%";
const KEY: &str = "LaunchOptions";

/// Whitespace-separated tokens, with double quotes grouping (as Steam splits them).
fn tokens(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        let mut quoted = false;
        while i < bytes.len() && (quoted || !bytes[i].is_ascii_whitespace()) {
            if bytes[i] == b'"' {
                quoted = !quoted;
            }
            i += 1;
        }
        if i > start {
            out.push(&s[start..i]);
        }
    }
    out
}

fn is_launcher(token: &str) -> bool {
    token
        .trim_matches('"')
        .to_ascii_lowercase()
        .ends_with(LAUNCHER_FILE)
}

pub fn has_launcher(options: &str) -> bool {
    tokens(options).into_iter().any(is_launcher)
}

/// `options` with any fwm-launch.exe removed. `%command% -x` is written back as
/// `-x`, which Steam treats the same, so an untouched setup round-trips.
pub fn without_launcher(options: &str) -> String {
    let kept: Vec<&str> = tokens(options)
        .into_iter()
        .filter(|t| !is_launcher(t))
        .collect();
    let joined = kept.join(" ");
    if joined == COMMAND {
        return String::new();
    }
    match joined.strip_prefix("%command% ") {
        Some(rest) if !rest.contains(COMMAND) => rest.to_owned(),
        _ => joined,
    }
}

/// `options` with `launcher` wrapping the game: replaces an older fwm-launch.exe,
/// keeps other wrappers and the user's own arguments.
pub fn with_launcher(options: &str, launcher: &Path) -> String {
    let base = without_launcher(options);
    let ours = format!("\"{}\" {COMMAND}", launcher.display());
    if base.contains(COMMAND) {
        base.replacen(COMMAND, &ours, 1)
    } else if base.is_empty() {
        ours
    } else {
        format!("{ours} {base}")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FactorioOptions {
    /// The account has an entry for Factorio (it has been played or configured).
    pub known: bool,
    pub launch_options: String,
}

fn app(pairs: &Pairs) -> Option<&Pairs> {
    let (_, Value::Obj(store)) = pairs.first()? else {
        return None;
    };
    ["Software", "Valve", "Steam", "apps", FACTORIO_APP_ID]
        .iter()
        .try_fold(store, |node, key| match vdf::get(node, key) {
            Some(Value::Obj(children)) => Some(children),
            _ => None,
        })
}

fn app_mut(pairs: &mut Pairs) -> Result<&mut Pairs, String> {
    let Some((_, Value::Obj(store))) = pairs.first_mut() else {
        return Err("no UserLocalConfigStore block".into());
    };
    let mut node = store;
    for key in ["Software", "Valve", "Steam", "apps", FACTORIO_APP_ID] {
        node = vdf::obj_mut(node, key).ok_or_else(|| format!("\"{key}\" isn't a block"))?;
    }
    Ok(node)
}

fn parse_exact(text: &str) -> Result<Pairs, String> {
    let pairs = vdf::parse(text).map_err(|e| e.to_string())?;
    if vdf::serialize(&pairs) != text {
        return Err("its layout isn't the one Steam writes, so it won't be rewritten".into());
    }
    Ok(pairs)
}

/// Factorio's launch options in a `localconfig.vdf`.
pub fn read(text: &str) -> Result<FactorioOptions, String> {
    let pairs = parse_exact(text)?;
    Ok(match app(&pairs) {
        None => FactorioOptions {
            known: false,
            launch_options: String::new(),
        },
        Some(app) => FactorioOptions {
            known: true,
            launch_options: match vdf::get(app, KEY) {
                Some(Value::Str(raw)) => vdf::unescape(raw),
                _ => String::new(),
            },
        },
    })
}

/// The `localconfig.vdf` text with Factorio's launch options set to
/// `change(current)`, or `None` if that changes nothing. Refuses files that
/// wouldn't serialize back byte for byte, so nothing else in them can change.
pub fn edit(text: &str, change: impl FnOnce(&str) -> String) -> Result<Option<String>, String> {
    let mut pairs = parse_exact(text)?;
    let app = app_mut(&mut pairs)?;
    let index = app.iter().position(|(k, _)| k.eq_ignore_ascii_case(KEY));
    let current = match index.map(|i| &app[i].1) {
        Some(Value::Str(raw)) => vdf::unescape(raw),
        Some(Value::Obj(_)) => return Err("LaunchOptions is a block, not a string".into()),
        None => String::new(),
    };
    let wanted = change(&current);
    if wanted == current {
        return Ok(None);
    }
    match (index, wanted.is_empty()) {
        (Some(i), true) => {
            app.remove(i);
        }
        (Some(i), false) => app[i].1 = Value::Str(vdf::escape(&wanted)),
        (None, _) => app.push((KEY.to_owned(), Value::Str(vdf::escape(&wanted)))),
    }
    Ok(Some(vdf::serialize(&pairs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn launcher() -> PathBuf {
        PathBuf::from(r"C:\Tools\fwm\fwm-launch.exe")
    }

    #[test]
    fn turning_on_with_no_options() {
        assert_eq!(
            with_launcher("", &launcher()),
            r#""C:\Tools\fwm\fwm-launch.exe" %command%"#
        );
    }

    #[test]
    fn turning_on_keeps_plain_arguments() {
        assert_eq!(
            with_launcher("--disable-audio -x", &launcher()),
            r#""C:\Tools\fwm\fwm-launch.exe" %command% --disable-audio -x"#
        );
    }

    #[test]
    fn turning_on_keeps_other_wrappers() {
        assert_eq!(
            with_launcher("gamemoderun %command% -x", &launcher()),
            r#"gamemoderun "C:\Tools\fwm\fwm-launch.exe" %command% -x"#
        );
    }

    #[test]
    fn turning_on_again_replaces_an_old_path() {
        let old = r#""D:\Old Place\fwm-launch.exe" %command% -x"#;
        assert_eq!(
            with_launcher(old, &launcher()),
            r#""C:\Tools\fwm\fwm-launch.exe" %command% -x"#
        );
    }

    #[test]
    fn turning_off_restores_what_was_there() {
        for original in [
            "",
            "--disable-audio -x",
            "gamemoderun %command% -x",
            "%command% --a %command%",
        ] {
            let on = with_launcher(original, &launcher());
            assert!(has_launcher(&on));
            assert_eq!(without_launcher(&on), original, "from {on:?}");
        }
    }

    #[test]
    fn off_leaves_unrelated_options_alone() {
        assert!(!has_launcher("-x --y"));
        assert_eq!(without_launcher("-x --y"), "-x --y");
    }

    const CONFIG: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"427520\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1790000000\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

    #[test]
    fn edit_adds_and_removes_the_option_in_a_config_file() {
        let on = edit(CONFIG, |current| with_launcher(current, &launcher()))
            .unwrap()
            .unwrap();
        assert!(on.contains("\t\t\t\t\t\t\"LaunchOptions\"\t\t\"\\\"C:\\\\Tools\\\\fwm\\\\fwm-launch.exe\\\" %command%\"\n"));
        assert_eq!(
            read(&on).unwrap().launch_options,
            r#""C:\Tools\fwm\fwm-launch.exe" %command%"#
        );
        assert_eq!(
            edit(&on, |c| with_launcher(c, &launcher())).unwrap(),
            None,
            "already on"
        );

        let off = edit(&on, without_launcher).unwrap().unwrap();
        assert_eq!(off, CONFIG, "turning off restores the file exactly");
    }

    #[test]
    fn edit_creates_the_factorio_entry_when_missing() {
        let bare = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t}\n\t\t}\n\t}\n}\n";
        assert!(!read(bare).unwrap().known);
        let on = edit(bare, |c| with_launcher(c, &launcher()))
            .unwrap()
            .unwrap();
        assert!(read(&on).unwrap().known);
    }

    #[test]
    fn edit_refuses_files_it_cannot_reproduce() {
        let crlf = CONFIG.replace('\n', "\r\n");
        assert!(edit(&crlf, |c| with_launcher(c, &launcher())).is_err());
        assert!(edit("not vdf", |c| c.to_owned()).is_err());
    }
}
