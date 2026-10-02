# Security policy

## Supported versions

Only the latest release gets fixes.

## What this software does

`fwm-launch.exe` starts `factorio.exe` (or opens a running one) and loads
`fwm_hook.dll` into it with `CreateRemoteThread`/`LoadLibraryW`. The DLL patches
a few of the game's GUI functions in memory. It reads `factorio.pdb` and
`positions.json`, and writes `positions.json` and its logs. It makes no network
connections and doesn't modify any file on disk outside its data folder.

Only download releases from this repository's Releases page. Each release lists
SHA-256 checksums, and the binaries are built by GitHub Actions from the tagged
source.

## Reporting a vulnerability

Please report security problems privately rather than in a public issue. Use
**Security → Report a vulnerability** on this repository, or contact
[@StevenNaliwajka](https://github.com/StevenNaliwajka) on GitHub. Include the
steps to reproduce and the release version. You'll get a reply within a week.
