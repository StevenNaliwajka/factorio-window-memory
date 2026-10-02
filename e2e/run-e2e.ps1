<#
.SYNOPSIS
  End-to-end test: runs the real game with the plugin and checks windows reopen
  where they were dragged.

.DESCRIPTION
  Uses an isolated Factorio data folder under target\e2e (its own config, saves
  and mods), with audio disabled and the window opened without focus. It never
  touches your normal Factorio data, and only stops a Factorio process that its
  own launcher started.

  Run 1  No saved positions. The fwm-e2e test mod opens several built-in windows,
         then holds the character window open while this script drags it by its
         title bar with posted mouse messages. Checks the drag is recorded and the
         window reopens at the dragged spot.
  Run 2  Seeds positions.json with a spot for every window class seen in run 1,
         restarts the game (so persistence across launches is covered) and checks
         every window opens at its seeded spot.

  Screenshots of every step land in target\e2e\run1 and target\e2e\run2.
#>
param(
    [switch]$SkipBuild,
    [int]$TimeoutSeconds = 300
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Launcher = Join-Path $Root 'target\release\fwm-launch.exe'
$Work = Join-Path $Root 'target\e2e'
$Data = Join-Path $Work 'data'
$FwmDir = Join-Path $Work 'fwm'
$Config = Join-Path $Data 'config\config.ini'
$Save = Join-Path $Data 'saves\fwm-e2e.zip'
$Out = Join-Path $Data 'script-output\fwm-e2e'
$Log = Join-Path $FwmDir 'fwm.log'
$Positions = Join-Path $FwmDir 'positions.json'
$DragTarget = @{ X = 20; Y = 20 }

$failures = New-Object System.Collections.Generic.List[string]
$warnings = New-Object System.Collections.Generic.List[string]
function Fail($message) { $failures.Add($message); Write-Host "FAIL: $message" -ForegroundColor Red }
function Warn($message) { $warnings.Add($message); Write-Host "WARN: $message" -ForegroundColor Yellow }
function Pass($message) { Write-Host "PASS: $message" -ForegroundColor Green }
function Step($message) { Write-Host "`n== $message" -ForegroundColor Cyan }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class FwmInput {
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    const uint WM_MOUSEMOVE = 0x200, WM_LBUTTONDOWN = 0x201, WM_LBUTTONUP = 0x202;
    static IntPtr At(int x, int y) { return (IntPtr)((y << 16) | (x & 0xFFFF)); }
    public static void Move(IntPtr w, int x, int y, bool down) { PostMessage(w, WM_MOUSEMOVE, (IntPtr)(down ? 1 : 0), At(x, y)); }
    public static void Down(IntPtr w, int x, int y) { PostMessage(w, WM_LBUTTONDOWN, (IntPtr)1, At(x, y)); }
    public static void Up(IntPtr w, int x, int y) { PostMessage(w, WM_LBUTTONUP, IntPtr.Zero, At(x, y)); }
}
'@

function Find-Factorio {
    if ($env:FWM_FACTORIO_EXE) { return $env:FWM_FACTORIO_EXE }
    $libraries = @()
    foreach ($steam in @('C:\Program Files (x86)\Steam', 'C:\Program Files\Steam')) {
        $libraries += $steam
        $vdf = Join-Path $steam 'steamapps\libraryfolders.vdf'
        if (Test-Path $vdf) {
            foreach ($match in (Select-String -Path $vdf -Pattern '"path"\s+"([^"]+)"')) {
                $libraries += ($match.Matches[0].Groups[1].Value -replace '\\\\', '\')
            }
        }
    }
    foreach ($library in $libraries) {
        $exe = Join-Path $library 'steamapps\common\Factorio\bin\x64\factorio.exe'
        if (Test-Path $exe) { return $exe }
    }
    throw 'factorio.exe not found; set FWM_FACTORIO_EXE'
}

function Read-Events {
    $events = @()
    if (-not (Test-Path $Log)) { return $events }
    foreach ($line in Get-Content $Log) {
        if ($line -match 'seen class=(\S+) centered=(-?\d+),(-?\d+) size=(\d+)x(\d+) bounds=(\d+)x(\d+) saved=(\S+)') {
            $events += [pscustomobject]@{ Kind = 'seen'; Class = $Matches[1]; X = [int]$Matches[2]; Y = [int]$Matches[3]
                W = [int]$Matches[4]; H = [int]$Matches[5]; BoundsW = [int]$Matches[6]; BoundsH = [int]$Matches[7]; Saved = $Matches[8] }
        } elseif ($line -match 'placed class=(\S+) target=(-?\d+),(-?\d+) now=(-?\d+),(-?\d+)') {
            $events += [pscustomobject]@{ Kind = 'placed'; Class = $Matches[1]; TargetX = [int]$Matches[2]; TargetY = [int]$Matches[3]
                NowX = [int]$Matches[4]; NowY = [int]$Matches[5] }
        } elseif ($line -match 'moved class=(\S+) to=(-?\d+),(-?\d+) via=(\S+)') {
            $events += [pscustomobject]@{ Kind = 'moved'; Class = $Matches[1]; X = [int]$Matches[2]; Y = [int]$Matches[3]; Via = $Matches[4] }
        }
    }
    return $events
}

function Start-Game($name) {
    Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
    $arguments = @('--fwm-data-dir', "`"$FwmDir`"", '--fwm-verbose', '--fwm-no-activate',
        "`"$Factorio`"", '-c', "`"$Config`"", '--load-game', "`"$Save`"", '--window-size', '1280x720', '--disable-audio')
    $launcherProcess = Start-Process -FilePath $Launcher -ArgumentList $arguments -PassThru
    $deadline = (Get-Date).AddSeconds(60)
    do {
        Start-Sleep -Milliseconds 300
        $game = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($launcherProcess.Id) AND Name='factorio.exe'" | Select-Object -First 1
    } while (-not $game -and -not $launcherProcess.HasExited -and (Get-Date) -lt $deadline)
    if (-not $game) { throw "${name}: the game didn't start (launcher exit code $($launcherProcess.ExitCode))" }
    if ($script:PreexistingPids -contains $game.ProcessId) { throw "${name}: pid $($game.ProcessId) predates the test; refusing to touch it" }
    Write-Host "${name}: game pid $($game.ProcessId) (launcher pid $($launcherProcess.Id))"
    return [pscustomobject]@{ Launcher = $launcherProcess; Game = (Get-Process -Id $game.ProcessId) }
}

function Wait-ForFile($path, $run) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while (-not (Test-Path $path)) {
        if ($run.Game.HasExited -or (Get-Date) -gt $deadline) { return $false }
        Start-Sleep -Milliseconds 250
    }
    return $true
}

function Stop-Game($run, $name) {
    # Only ever the process our launcher started (checked in Start-Game).
    if (-not $run.Game.HasExited) { Stop-Process -Id $run.Game.Id -Force }
    $run.Launcher.WaitForExit(15000) | Out-Null
    $shots = Join-Path $Work $name
    Remove-Item -Recurse -Force $shots -ErrorAction SilentlyContinue
    if (Test-Path $Out) { Copy-Item -Recurse $Out $shots }
    Copy-Item $Log (Join-Path $Work "$name-fwm.log") -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $Data 'factorio-current.log') (Join-Path $Work "$name-factorio.log") -ErrorAction SilentlyContinue
}

function Assert-Hooked($name) {
    $text = Get-Content $Log -Raw
    if ($text -match 'hooks installed: (.+)') { Pass "${name}: hooks installed ($($Matches[1].Trim()))" }
    else { Fail "${name}: hooks not installed:`n$text"; return $false }
    return $true
}

# Posted mouse messages reach an unfocused window, but SDL asks Windows to report
# when the mouse leaves, and since the real cursor is elsewhere that WM_MOUSELEAVE
# is queued right behind the first move. Burst mode queues the whole drag ahead
# of it; paced mode spreads it over frames in case the game needs that.
function Invoke-Drag($hwnd, $fromX, $fromY, $toX, $toY, [switch]$Burst) {
    $pause = if ($Burst) { 0 } else { 40 }
    [FwmInput]::Move($hwnd, $fromX, $fromY, $false)
    [FwmInput]::Down($hwnd, $fromX, $fromY)
    if (-not $Burst) { Start-Sleep -Milliseconds 150 }
    $steps = 20
    for ($i = 1; $i -le $steps; $i++) {
        $x = [int]($fromX + ($toX - $fromX) * $i / $steps)
        $y = [int]($fromY + ($toY - $fromY) * $i / $steps)
        [FwmInput]::Move($hwnd, $x, $y, $true)
        if ($pause) { Start-Sleep -Milliseconds $pause }
    }
    [FwmInput]::Up($hwnd, $toX, $toY)
    [FwmInput]::Move($hwnd, $toX, $toY, $false)
}

# ---------------------------------------------------------------------------

$Factorio = Find-Factorio
$script:PreexistingPids = @(Get-Process -Name factorio -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
Write-Host "factorio: $Factorio"
if ($PreexistingPids.Count) { Write-Host "already running (left alone): $($PreexistingPids -join ', ')" }

if (-not $SkipBuild) {
    Step 'build'
    Push-Location $Root
    cargo build --release --workspace
    $built = $LASTEXITCODE
    Pop-Location
    if ($built) { throw 'cargo build failed' }
}

Step 'isolated Factorio data folder'
Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Split-Path $Config), (Join-Path $Data 'mods'), (Join-Path $Data 'saves'), $FwmDir | Out-Null
Set-Content -Path $Config -Encoding ASCII -Value @(
    '[path]', 'read-data=__PATH__executable__/../../data', "write-data=$($Data -replace '\\', '/')",
    '[graphics]', 'full-screen=false')
$modList = @{ mods = @(
        @{ name = 'base'; enabled = $true }, @{ name = 'fwm-e2e'; enabled = $true },
        @{ name = 'space-age'; enabled = $false }, @{ name = 'quality'; enabled = $false }, @{ name = 'elevated-rails'; enabled = $false }) }
$modList | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $Data 'mods\mod-list.json') -Encoding ASCII
Copy-Item -Recurse (Join-Path $PSScriptRoot 'mod\fwm-e2e') (Join-Path $Data 'mods\fwm-e2e_0.1.0')
Write-Host $Data

Step 'create a test map'
$create = Start-Process -FilePath $Factorio -ArgumentList @('-c', "`"$Config`"", '--create', "`"$Save`"", '--disable-audio') -PassThru -Wait -WindowStyle Hidden
if (-not (Test-Path $Save)) { throw "map creation failed (exit $($create.ExitCode)); see $Data\factorio-current.log" }
Pass "created $Save"

# --- run 1 ------------------------------------------------------------------
Step 'run 1: open windows, drag the character window, reopen it'
$run = Start-Game 'run 1'
if (-not (Wait-ForFile (Join-Path $Out 'hold.txt') $run)) {
    Stop-Game $run 'run1'
    throw "run 1: the test mod never reached the drag step; see $Work\run1-factorio.log and run1-fwm.log"
}
Start-Sleep -Seconds 1
$hooked1 = Assert-Hooked 'run 1'
$focusStolen = ([FwmInput]::GetForegroundWindow() -eq $run.Game.MainWindowHandle)
if ($focusStolen) { Warn 'the test window took keyboard focus' }

$events = Read-Events
$character = $events | Where-Object Kind -eq 'seen' | Select-Object -Last 1
$draggedClass = $null
if (-not $character) {
    Fail 'run 1: no window was centred, so Window::center never ran through the hook'
} else {
    Write-Host "character window: class=$($character.Class) at $($character.X),$($character.Y) size $($character.W)x$($character.H), game client $($character.BoundsW)x$($character.BoundsH)"
    $expectedX = [int](($character.BoundsW - $character.W) / 2)
    if ([math]::Abs($character.X - $expectedX) -le 2) { Pass 'GUI coordinates match the game window''s client pixels' }
    else { Warn "window centred at x=$($character.X) but client-area centre is x=$expectedX; GUI and client coordinates may differ" }

    $moves = @()
    foreach ($attempt in 1..3) {
        # Start from wherever the window is now, in case an earlier attempt half-worked.
        $here = @{ X = $character.X; Y = $character.Y }
        $latest = Read-Events | Where-Object { $_.Kind -eq 'moved' -and $_.Class -eq $character.Class } | Select-Object -Last 1
        if ($latest) { $here = @{ X = $latest.X; Y = $latest.Y } }
        $grabX = $here.X + 60; $grabY = $here.Y + 14
        $dropX = $grabX - ($here.X - $DragTarget.X); $dropY = $grabY - ($here.Y - $DragTarget.Y)
        $mode = if ($attempt -eq 2) { 'paced' } else { 'burst' }
        Write-Host "drag attempt ${attempt} (${mode}): title bar $grabX,$grabY -> $dropX,$dropY"
        Invoke-Drag $run.Game.MainWindowHandle $grabX $grabY $dropX $dropY -Burst:($mode -eq 'burst')
        Start-Sleep -Seconds 2
        $moves = @(Read-Events | Where-Object { $_.Kind -eq 'moved' -and $_.Class -eq $character.Class })
        if ($moves.Count -and $moves[-1].X -eq $DragTarget.X -and $moves[-1].Y -eq $DragTarget.Y) { break }
    }
    if ($moves.Count -eq 0) {
        Warn 'run 1: the synthetic drag never reached the game (posted mouse messages are ignored by unfocused windows); drag recording needs a manual check'
    } else {
        $last = $moves[-1]
        $vias = ($moves | Select-Object -ExpandProperty Via -Unique) -join ', '
        Pass "run 1: drag recorded via $vias, window ended at $($last.X),$($last.Y)"
        $saved = (Get-Content $Positions -Raw | ConvertFrom-Json).windows.($character.Class)
        if ($saved -and $saved.x -eq $last.X -and $saved.y -eq $last.Y) {
            Pass "run 1: positions.json has $($character.Class) at $($saved.x),$($saved.y)"
            $draggedClass = $character.Class
        } else {
            Fail "run 1: positions.json doesn't have the dragged position (has: $($saved | ConvertTo-Json -Compress))"
        }
    }
}

if (-not (Wait-ForFile (Join-Path $Out 'done.json') $run)) { Fail 'run 1: the test mod never finished' }
Start-Sleep -Seconds 1
$events = Read-Events
$done = Get-Content (Join-Path $Out 'done.json') -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json
Stop-Game $run 'run1'

if ($done) {
    foreach ($property in $done.opened.PSObject.Properties) {
        if ($property.Value) { Pass "run 1: the $($property.Name) window opened" } else { Fail "run 1: the $($property.Name) window didn't open" }
    }
}
if ($draggedClass) {
    $reopen = $events | Where-Object { $_.Kind -eq 'placed' -and $_.Class -eq $draggedClass } | Select-Object -Last 1
    $saved = (Get-Content $Positions -Raw | ConvertFrom-Json).windows.$draggedClass
    if ($reopen -and $reopen.NowX -eq $saved.x -and $reopen.NowY -eq $saved.y) {
        Pass "run 1: after closing and reopening, $draggedClass came back at $($reopen.NowX),$($reopen.NowY)"
    } else {
        Fail "run 1: reopened $draggedClass wasn't placed at the dragged spot ($($reopen | ConvertTo-Json -Compress))"
    }
}
$classes = @($events | Where-Object Kind -eq 'seen' | Select-Object -ExpandProperty Class -Unique)
Write-Host "window classes seen: $($classes -join ', ')"

# --- run 2 ------------------------------------------------------------------
Step 'run 2: restart with seeded positions for every window class'
$windows = [ordered]@{}
$i = 0
foreach ($class in $classes) {
    if ($class -eq $draggedClass) {
        $saved = (Get-Content $Positions -Raw | ConvertFrom-Json).windows.$class
        $windows[$class] = [ordered]@{ x = [int]$saved.x; y = [int]$saved.y }
    } else {
        $windows[$class] = [ordered]@{ x = 30 + 25 * $i; y = 40 + 15 * $i }
    }
    $i++
}
[ordered]@{ version = 1; windows = $windows; ignore = @() } | ConvertTo-Json -Depth 5 | Set-Content -Path $Positions -Encoding ASCII
Write-Host (Get-Content $Positions -Raw)

$run = Start-Game 'run 2'
$finished = Wait-ForFile (Join-Path $Out 'done.json') $run
Start-Sleep -Seconds 1
$hooked2 = Assert-Hooked 'run 2'
$events = Read-Events
Stop-Game $run 'run2'
if (-not $finished) { Fail 'run 2: the test mod never finished' }

# Walk the log in order: a window should open at its seed, or wherever it was
# last dragged during this run (a real mouse passing over the unfocused test
# window can drag it), clamped to the screen the same way the plugin does.
$expected = @{}
foreach ($class in $windows.Keys) { $expected[$class] = @{ x = $windows[$class].x; y = $windows[$class].y } }
$lastSeen = @{}
$results = @{}
foreach ($class in $windows.Keys) { $results[$class] = [pscustomobject]@{ Ok = 0; Clamped = 0; Dragged = 0; Bad = @() } }
foreach ($event in $events) {
    if (-not $results.ContainsKey($event.Class)) { continue }
    $result = $results[$event.Class]
    switch ($event.Kind) {
        'seen' { $lastSeen[$event.Class] = $event }
        'moved' { $expected[$event.Class] = @{ x = $event.X; y = $event.Y }; $result.Dragged++ }
        'placed' {
            $seen = $lastSeen[$event.Class]
            $want = $expected[$event.Class]
            $x = [math]::Min([math]::Max($want.x, 0), [math]::Max($seen.BoundsW - $seen.W, 0))
            $y = [math]::Min([math]::Max($want.y, 0), [math]::Max($seen.BoundsH - $seen.H, 0))
            if ($event.TargetX -ne $x -or $event.TargetY -ne $y -or $event.NowX -ne $x -or $event.NowY -ne $y) {
                $result.Bad += "wanted $x,$y; placed $($event.TargetX),$($event.TargetY); ended $($event.NowX),$($event.NowY)"
            } elseif ($x -ne $want.x -or $y -ne $want.y) { $result.Clamped++ } else { $result.Ok++ }
        }
    }
}
foreach ($class in $windows.Keys) {
    $result = $results[$class]
    $seed = $windows[$class]
    $count = $result.Ok + $result.Clamped + $result.Bad.Count
    $notes = @()
    if ($result.Clamped) { $notes += "$($result.Clamped) clamped to stay on screen" }
    if ($result.Dragged) { $notes += "followed a real drag during the run" }
    $suffix = if ($notes.Count) { " ($($notes -join '; '))" } else { '' }
    if ($class -eq 'agui::Window') {
        if ($count -eq 0) { Pass "run 2: generic $class windows were left where Factorio put them, despite a saved spot" }
        else { Fail "run 2: generic $class windows were moved" }
    } elseif ($result.Bad.Count) { Fail "run 2: $class misplaced: $($result.Bad[0])" }
    elseif ($count -eq 0) { Fail "run 2: $class was never placed" }
    else { Pass "run 2: $class opened at its saved spot $($seed.x),$($seed.y) ${count}x$suffix" }
}

# --- summary -----------------------------------------------------------------
Step 'summary'
Write-Host "screenshots: $Work\run1, $Work\run2"
Write-Host "logs:        $Work\run1-fwm.log, $Work\run2-fwm.log"
foreach ($w in $warnings) { Write-Host "warning: $w" -ForegroundColor Yellow }
if ($failures.Count) {
    Write-Host "$($failures.Count) check(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host 'all end-to-end checks passed' -ForegroundColor Green
exit 0
