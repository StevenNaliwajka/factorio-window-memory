//! fwm-launch's command line. Steam runs it as `fwm-launch.exe %command%`, so the
//! game's own exe and arguments arrive after any `--fwm-*` options and are passed
//! through untouched. Run with no arguments at all (double-clicked), it's the
//! on/off switch.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
    pub dll: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    /// Stay alive until the game exits, so Steam sees the game as running.
    pub wait: bool,
    /// Skip early injection and inject once the game has started.
    pub late_inject: bool,
    /// Start the game window without taking focus (used by the end-to-end test).
    pub no_activate: bool,
    pub verbose: bool,
    /// Steam install to configure instead of the registered one (used by tests).
    pub steam_root: Option<PathBuf>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            dll: None,
            data_dir: None,
            wait: true,
            late_inject: false,
            no_activate: false,
            verbose: false,
            steam_root: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    /// No arguments: ask whether to turn the plugin on (or off, if it's on).
    Toggle,
    /// Set Factorio's Steam launch option to start through fwm-launch.
    Install {
        opts: LaunchOptions,
    },
    /// Remove fwm-launch from Factorio's Steam launch option.
    Uninstall {
        opts: LaunchOptions,
    },
    /// `exe` is `None` when it should be found in the Steam library.
    Launch {
        exe: Option<PathBuf>,
        args: Vec<OsString>,
        opts: LaunchOptions,
    },
    /// Inject into a running game; `pid` is `None` to find the only factorio.exe.
    Attach {
        pid: Option<u32>,
        opts: LaunchOptions,
    },
}

enum Mode {
    Attach(Option<u32>),
    Install,
    Uninstall,
}

/// `args` excludes the launcher's own path.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut args = args.into_iter().peekable();
    if args.peek().is_none() {
        return Ok(Command::Toggle);
    }
    let mut opts = LaunchOptions::default();
    let mut mode: Option<Mode> = None;

    while let Some(arg) = args.peek() {
        let Some(flag) = arg.to_str().map(str::to_owned) else {
            break;
        };
        match flag.as_str() {
            "-h" | "--help" | "/?" => return Ok(Command::Help),
            "--attach" => {
                args.next();
                let pid = args
                    .peek()
                    .and_then(|a| a.to_str())
                    .and_then(|s| s.parse::<u32>().ok());
                if pid.is_some() {
                    args.next();
                }
                mode = Some(Mode::Attach(pid));
            }
            "--install" => {
                args.next();
                mode = Some(Mode::Install);
            }
            "--uninstall" => {
                args.next();
                mode = Some(Mode::Uninstall);
            }
            "--launch" => {
                args.next();
            }
            "--fwm-dll" => {
                args.next();
                opts.dll = Some(value(&mut args, &flag)?.into());
            }
            "--fwm-data-dir" => {
                args.next();
                opts.data_dir = Some(value(&mut args, &flag)?.into());
            }
            "--fwm-steam-root" => {
                args.next();
                opts.steam_root = Some(value(&mut args, &flag)?.into());
            }
            "--fwm-no-wait" => {
                args.next();
                opts.wait = false;
            }
            "--fwm-late-inject" => {
                args.next();
                opts.late_inject = true;
            }
            "--fwm-no-activate" => {
                args.next();
                opts.no_activate = true;
            }
            "--fwm-verbose" => {
                args.next();
                opts.verbose = true;
            }
            other if other.starts_with("--fwm-") => return Err(format!("unknown option {other}")),
            _ => break,
        }
    }

    let rest: Vec<OsString> = args.collect();
    match mode {
        Some(_) if !rest.is_empty() => {
            Err("--attach, --install and --uninstall don't take a game command".into())
        }
        Some(Mode::Attach(pid)) => Ok(Command::Attach { pid, opts }),
        Some(Mode::Install) => Ok(Command::Install { opts }),
        Some(Mode::Uninstall) => Ok(Command::Uninstall { opts }),
        None => {
            let (exe, args) = match rest.split_first() {
                Some((first, tail)) if looks_like_exe(first) => {
                    (Some(PathBuf::from(first)), tail.to_vec())
                }
                _ => (None, rest),
            };
            Ok(Command::Launch { exe, args, opts })
        }
    }
}

fn value(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn looks_like_exe(arg: &OsStr) -> bool {
    Path::new(arg)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// A `CreateProcessW` command line (without the trailing NUL) that the child's
/// CRT will split back into exactly `exe` followed by `args`.
pub fn build_command_line(exe: &OsStr, args: &[OsString]) -> Vec<u16> {
    let mut line = Vec::new();
    append_quoted(&mut line, exe, true);
    for arg in args {
        line.push(b' ' as u16);
        append_quoted(&mut line, arg, false);
    }
    line
}

/// The MSVC CRT's argv rules: backslashes are literal unless they precede a quote.
fn append_quoted(line: &mut Vec<u16>, arg: &OsStr, force: bool) {
    let arg: Vec<u16> = arg.encode_wide().collect();
    let needs_quotes = arg.is_empty()
        || arg
            .iter()
            .any(|&c| matches!(c, 0x20 | 0x09 | 0x0a | 0x0b | 0x22));
    if !force && !needs_quotes {
        line.extend(arg);
        return;
    }
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    line.push(QUOTE);
    let mut i = 0;
    loop {
        let mut backslashes = 0;
        while i < arg.len() && arg[i] == BACKSLASH {
            backslashes += 1;
            i += 1;
        }
        if i == arg.len() {
            line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
            break;
        }
        if arg[i] == QUOTE {
            line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2 + 1));
        } else {
            line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        }
        line.push(arg[i]);
        i += 1;
    }
    line.push(QUOTE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn line(exe: &str, args: &[&str]) -> String {
        String::from_utf16(&build_command_line(OsStr::new(exe), &os(args))).unwrap()
    }

    #[test]
    fn steam_style_command_is_passed_through() {
        let cmd = parse(os(&[
            r"D:\Steam\Factorio\bin\x64\factorio.exe",
            "--load-game",
            "my save.zip",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            Command::Launch {
                exe: Some(PathBuf::from(r"D:\Steam\Factorio\bin\x64\factorio.exe")),
                args: os(&["--load-game", "my save.zip"]),
                opts: LaunchOptions::default(),
            }
        );
    }

    #[test]
    fn launcher_options_come_before_the_game() {
        let cmd = parse(os(&[
            "--fwm-data-dir",
            r"C:\fwm",
            "--fwm-no-wait",
            "--fwm-verbose",
            "factorio.exe",
            "--fwm-dll",
        ]))
        .unwrap();
        let Command::Launch { exe, args, opts } = cmd else {
            panic!()
        };
        assert_eq!(exe, Some(PathBuf::from("factorio.exe")));
        // After the exe everything belongs to the game, even if it looks like ours.
        assert_eq!(args, os(&["--fwm-dll"]));
        assert_eq!(opts.data_dir, Some(PathBuf::from(r"C:\fwm")));
        assert!(!opts.wait);
        assert!(opts.verbose);
    }

    #[test]
    fn no_arguments_means_the_on_off_switch() {
        assert_eq!(parse(os(&[])).unwrap(), Command::Toggle);
    }

    #[test]
    fn launch_finds_the_game() {
        assert_eq!(
            parse(os(&["--launch"])).unwrap(),
            Command::Launch {
                exe: None,
                args: vec![],
                opts: LaunchOptions::default()
            }
        );
    }

    #[test]
    fn install_and_uninstall() {
        let Command::Install { opts } =
            parse(os(&["--install", "--fwm-steam-root", r"C:\S"])).unwrap()
        else {
            panic!()
        };
        assert_eq!(opts.steam_root, Some(PathBuf::from(r"C:\S")));
        assert!(matches!(
            parse(os(&["--uninstall"])).unwrap(),
            Command::Uninstall { .. }
        ));
        assert!(parse(os(&["--install", "factorio.exe"])).is_err());
    }

    #[test]
    fn game_arguments_without_an_exe() {
        let Command::Launch { exe, args, .. } = parse(os(&["--load-game", "x.zip"])).unwrap()
        else {
            panic!()
        };
        assert_eq!(exe, None);
        assert_eq!(args, os(&["--load-game", "x.zip"]));
    }

    #[test]
    fn attach_with_and_without_a_pid() {
        assert!(matches!(
            parse(os(&["--attach", "1234"])).unwrap(),
            Command::Attach {
                pid: Some(1234),
                ..
            }
        ));
        assert!(matches!(
            parse(os(&["--attach"])).unwrap(),
            Command::Attach { pid: None, .. }
        ));
        assert!(parse(os(&["--attach", "factorio.exe"])).is_err());
    }

    #[test]
    fn bad_options_are_errors() {
        assert!(parse(os(&["--fwm-bogus"])).is_err());
        assert!(parse(os(&["--fwm-dll"])).is_err());
        assert_eq!(parse(os(&["--help"])).unwrap(), Command::Help);
    }

    #[test]
    fn command_line_quoting() {
        assert_eq!(line(r"C:\a\f.exe", &[]), r#""C:\a\f.exe""#);
        assert_eq!(
            line(r"C:\Program Files\f.exe", &["--x", "a b"]),
            r#""C:\Program Files\f.exe" --x "a b""#
        );
        assert_eq!(line("f.exe", &[r#"say "hi""#]), r#""f.exe" "say \"hi\"""#);
        assert_eq!(
            line("f.exe", &[r"C:\dir with space\"]),
            r#""f.exe" "C:\dir with space\\""#
        );
        assert_eq!(line("f.exe", &[""]), r#""f.exe" """#);
        assert_eq!(
            line("f.exe", &[r"C:\no\spaces\"]),
            r#""f.exe" C:\no\spaces\"#
        );
    }
}
