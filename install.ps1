<#
.SYNOPSIS
  Builds a release from source and copies fwm-launch.exe and fwm_hook.dll to -Destination.
#>
param([string]$Destination = (Join-Path $env:LOCALAPPDATA 'Programs\factorio-window-memory'))

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --release -p fwm-launch -p fwm-hook
if ($LASTEXITCODE) { throw 'cargo build failed' }

New-Item -ItemType Directory -Force $Destination | Out-Null
Copy-Item 'target\release\fwm-launch.exe', 'target\release\fwm_hook.dll' $Destination -Force

Write-Host "installed to $Destination"
Write-Host "Steam launch option:  `"$Destination\fwm-launch.exe`" %command%"
