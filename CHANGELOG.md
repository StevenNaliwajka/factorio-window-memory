# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-02

### Added

- `fwm-launch.exe`: starts Factorio with the plugin injected (works as a Steam
  launch option), or loads it into a running game with `--attach`.
- Set-and-forget switch: double-click `fwm-launch.exe` (or use `--install` /
  `--uninstall`) to add or remove it from Factorio's Steam launch option in every
  Steam account that has Factorio. Your other launch options are kept, Steam is
  restarted safely, and the original config is backed up.
- `fwm_hook.dll`: built-in windows open where that kind of window was last
  dragged. Positions are saved to `%APPDATA%\Factorio\window-memory\positions.json`
  and clamped to stay on screen.
- Functions are found by name in the shipped `factorio.pdb`, after checking it
  matches the running `factorio.exe`; on any mismatch nothing is patched.
- An `ignore` list in `positions.json` for window types that should stay centred.
- Release binaries link the C runtime statically, so they don't need the Visual
  C++ Redistributable.
- Unit, injection and installed-game tests, plus an in-game end-to-end test.

Tested with Factorio 2.0.77 (Steam, Space Age).

[Unreleased]: https://github.com/StevenNaliwajka/factorio-window-memory/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/StevenNaliwajka/factorio-window-memory/releases/tag/v0.1.0
