# Contributing

Thanks for helping. Bug reports, fixes for new Factorio versions, and support for
more windows are all welcome.

## Reporting a problem

Open an issue using the bug report form and attach `fwm.log` and
`fwm-launch.log` from `%APPDATA%\Factorio\window-memory`. They record which
Factorio build was running, whether the PDB matched, and which hooks went in.

If the game itself misbehaves, first check whether it still happens when started
without the plugin. Please don't report problems to Wube while the plugin is
loaded.

## Building

You need Windows, Rust 1.85 or newer with the MSVC toolchain
(`stable-x86_64-pc-windows-msvc`), and the Visual Studio C++ build tools.

```powershell
cargo build --workspace            # debug
cargo build --release --workspace  # what gets shipped
```

## Testing

```powershell
.\test.ps1           # everything, including the in-game test
.\test.ps1 -NoGame   # without launching Factorio
```

- `cargo test --workspace` covers the unit tests and injection into a stand-in
  process. It also checks that your installed `factorio.pdb` still has every
  function the hook needs; that check skips itself when Factorio isn't installed.
- The in-game test, `e2e\run-e2e.ps1`, needs the Steam copy of Factorio. It runs
  the game in an isolated data folder under `target\e2e`, so your saves and
  settings aren't touched. It saves screenshots of each step for a visual check.

CI runs format, clippy, build and `cargo test` on every push and pull request.
It can't run the game, so run `.\test.ps1` yourself for changes to the hook or
launcher.

## When a Factorio update breaks it

1. Run `cargo test -p fwm-core --test installed_factorio`. It names any function
   that's missing or no longer safe to patch.
2. Find the new name with any PDB symbol dumper and update
   `crates/fwm-core/src/targets.rs`.
3. Run `.\test.ps1`, and note the Factorio version you tested in `CHANGELOG.md`.

## Pull requests

- Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`.
- Keep game-facing code fail-safe: hooks must always call the original function,
  and must never let a panic or a bad pointer reach the game.
- Add an entry under `Unreleased` in `CHANGELOG.md` for user-visible changes.

By contributing you agree your work is released under the [MIT License](LICENSE)
and that you'll follow the [Code of Conduct](CODE_OF_CONDUCT.md).
