<#
.SYNOPSIS
  Builds everything and runs every test.

.DESCRIPTION
  1. cargo build (debug + release)
  2. cargo test: unit tests, the check that the installed factorio.pdb still has
     every function the hook needs, and injection into a stand-in process
  3. e2e\run-e2e.ps1: the real game, twice, in an isolated data folder
     (skip with -NoGame)
#>
param([switch]$NoGame)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --workspace
if ($LASTEXITCODE) { exit 1 }
cargo build --release --workspace
if ($LASTEXITCODE) { exit 1 }
cargo test --workspace
if ($LASTEXITCODE) { exit 1 }

if (-not $NoGame) {
    & (Join-Path $PSScriptRoot 'e2e\run-e2e.ps1') -SkipBuild
    exit $LASTEXITCODE
}
