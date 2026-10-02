<#
.SYNOPSIS
  Builds a release from source, copies it to -Destination, and turns the plugin
  on (sets Factorio's Steam launch option; Steam restarts if it's open).

.PARAMETER SkipSteam
  Only copy the files; leave Steam's launch option alone.
#>
param(
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'Programs\factorio-window-memory'),
    [switch]$SkipSteam
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --release -p fwm-launch -p fwm-hook
if ($LASTEXITCODE) { throw 'cargo build failed' }

New-Item -ItemType Directory -Force $Destination | Out-Null
Copy-Item 'target\release\fwm-launch.exe', 'target\release\fwm_hook.dll' $Destination -Force
Write-Host "installed to $Destination"

if ($SkipSteam) {
    Write-Host "Steam launch option:  `"$Destination\fwm-launch.exe`" %command%"
} else {
    # Piping makes PowerShell wait for the GUI-subsystem exe and shows its output.
    & (Join-Path $Destination 'fwm-launch.exe') --install | ForEach-Object { $_ }
    if ($LASTEXITCODE) { throw "turning the plugin on failed (exit $LASTEXITCODE)" }
}
