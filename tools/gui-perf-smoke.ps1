param(
    [switch]$SkipBuild,
    [switch]$Release,
    [switch]$KeepArtifacts,
    [string]$Bvid = 'BV1quVd6DEA2',
    [int]$DurationSeconds = 12,
    [double]$EnterXRatio = 0.20,
    [double]$EnterYRatio = 0.45,
    [double]$StartXRatio = 0.955,
    [double]$StartYRatio = 0.064,
    [int]$PostEnterDelayMs = 2500,
    [int]$PostStartDelayMs = 1200
)

$ErrorActionPreference = 'Stop'

if (-not $SkipBuild) {
    if ($Release) {
        cargo build --release --features gui-gpui --bin bili-opinion-gui
    } else {
        cargo build --features gui-gpui --bin bili-opinion-gui
    }
}

Add-Type -AssemblyName System.Drawing
if (-not ('GuiPerfSmokeWin' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class GuiPerfSmokeWin {
    [DllImport("shcore.dll")] public static extern int SetProcessDpiAwareness(int value);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int dwAttribute, out RECT pvAttribute, int cbAttribute);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
'@
}

try {
    [GuiPerfSmokeWin]::SetProcessDpiAwareness(2) | Out-Null
} catch {
    # Already set by this PowerShell host or unavailable.
}

$tmp = Resolve-Path 'tmp' -ErrorAction SilentlyContinue
if (-not $tmp) {
    New-Item -ItemType Directory -Force tmp | Out-Null
    $tmp = Resolve-Path 'tmp'
}

$stdout = Join-Path $tmp 'gui-perf-smoke-stdout.log'
$stderr = Join-Path $tmp 'gui-perf-smoke-stderr.log'
$entryPath = Join-Path $tmp 'gui-perf-smoke-entry.png'
$workbenchPath = Join-Path $tmp 'gui-perf-smoke-workbench.png'
$runPath = Join-Path $tmp 'gui-perf-smoke-run.png'
$frameLog = Join-Path $tmp 'gui-frame-stats.jsonl'
$outputRoot = Join-Path $tmp 'gui-perf-output'
Remove-Item -LiteralPath $stdout, $stderr, $entryPath, $workbenchPath, $runPath, $frameLog -Force -ErrorAction SilentlyContinue

function Get-MainWindowHandle {
    param([System.Diagnostics.Process]$Process)

    for ($i = 0; $i -lt 40; $i++) {
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "GUI exited before exposing a window. stderr: $(Get-Content -Raw $stderr -ErrorAction SilentlyContinue)"
        }
        if ($Process.MainWindowHandle -ne 0) {
            return [IntPtr]$Process.MainWindowHandle
        }
        Start-Sleep -Milliseconds 250
    }

    throw "Timed out waiting for GUI window for pid $($Process.Id)."
}

function Get-WindowBounds {
    param([IntPtr]$Hwnd)

    $rect = New-Object GuiPerfSmokeWin+RECT
    $result = [GuiPerfSmokeWin]::DwmGetWindowAttribute(
        $Hwnd,
        9,
        [ref]$rect,
        [System.Runtime.InteropServices.Marshal]::SizeOf([type][GuiPerfSmokeWin+RECT])
    )
    if ($result -ne 0) {
        throw "DwmGetWindowAttribute failed: $result"
    }

    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -lt 300 -or $height -lt 300) {
        throw "Window bounds too small: $width x $height"
    }

    [pscustomobject]@{
        left = $rect.Left
        top = $rect.Top
        width = $width
        height = $height
    }
}

function Capture-Window {
    param(
        [IntPtr]$Hwnd,
        [string]$Path
    )

    $bounds = Get-WindowBounds -Hwnd $Hwnd
    $bitmap = New-Object System.Drawing.Bitmap $bounds.width, $bounds.height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.left, $bounds.top, 0, 0, [System.Drawing.Size]::new($bounds.width, $bounds.height))
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }

    [pscustomobject]@{
        path = $Path
        width = $bounds.width
        height = $bounds.height
        left = $bounds.left
        top = $bounds.top
    }
}

function Click-WindowRatio {
    param(
        [pscustomobject]$Bounds,
        [double]$XRatio,
        [double]$YRatio
    )

    $x = $Bounds.left + [int][Math]::Round($Bounds.width * $XRatio)
    $y = $Bounds.top + [int][Math]::Round($Bounds.height * $YRatio)
    [GuiPerfSmokeWin]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 90
    [GuiPerfSmokeWin]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 70
    [GuiPerfSmokeWin]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)

    [pscustomobject]@{
        x = $x
        y = $y
        windowX = $x - $Bounds.left
        windowY = $y - $Bounds.top
        xRatio = [Math]::Round($XRatio, 4)
        yRatio = [Math]::Round($YRatio, 4)
    }
}

function Get-ProcessGpuPercent {
    param([int]$ProcessId)

    try {
        $pattern = "pid_$ProcessId"
        $samples = (Get-Counter '\GPU Engine(*)\Utilization Percentage' -ErrorAction Stop).CounterSamples |
            Where-Object { $_.InstanceName -like "*$pattern*" }
        if (-not $samples) {
            return $null
        }
        return ($samples | Measure-Object -Property CookedValue -Sum).Sum
    } catch {
        return $null
    }
}

function Read-FrameSamples {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        return @()
    }

    $samples = @()
    foreach ($line in Get-Content -LiteralPath $Path) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        try {
            $samples += ($line | ConvertFrom-Json)
        } catch {
            # Ignore a partially written line.
        }
    }
    return $samples
}

$oldFrameStatsPath = $env:BILI_GUI_FRAME_STATS_PATH
$oldSmokeBvids = $env:BILI_GUI_SMOKE_BVIDS
$oldSmokeOutput = $env:BILI_GUI_SMOKE_OUTPUT
$env:BILI_GUI_FRAME_STATS_PATH = $frameLog
$env:BILI_GUI_SMOKE_BVIDS = $Bvid
$env:BILI_GUI_SMOKE_OUTPUT = $outputRoot
$proc = $null

try {
    $exePath = if ($Release) {
        'target\release\bili-opinion-gui.exe'
    } else {
        'target\debug\bili-opinion-gui.exe'
    }
    $exe = Resolve-Path $exePath
    $proc = Start-Process -FilePath $exe -WorkingDirectory (Get-Location) -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    $hwnd = Get-MainWindowHandle -Process $proc
    [GuiPerfSmokeWin]::ShowWindow($hwnd, 3) | Out-Null
    [GuiPerfSmokeWin]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 900

    $entry = Capture-Window -Hwnd $hwnd -Path $entryPath
    $enterClick = Click-WindowRatio -Bounds $entry -XRatio $EnterXRatio -YRatio $EnterYRatio
    Start-Sleep -Milliseconds $PostEnterDelayMs

    $workbench = Capture-Window -Hwnd $hwnd -Path $workbenchPath
    [GuiPerfSmokeWin]::SetForegroundWindow($hwnd) | Out-Null
    $startClick = Click-WindowRatio -Bounds $workbench -XRatio $StartXRatio -YRatio $StartYRatio
    Start-Sleep -Milliseconds $PostStartDelayMs

    $cpuSamples = @()
    $gpuSamples = @()
    $lastCpuMs = $proc.TotalProcessorTime.TotalMilliseconds
    $lastCpuAt = Get-Date
    $deadline = (Get-Date).AddSeconds($DurationSeconds)

    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $proc.Refresh()
        if ($proc.HasExited) {
            throw "GUI exited during perf smoke. stderr: $(Get-Content -Raw $stderr -ErrorAction SilentlyContinue)"
        }

        $now = Get-Date
        $cpuMs = $proc.TotalProcessorTime.TotalMilliseconds
        $wallMs = ($now - $lastCpuAt).TotalMilliseconds
        if ($wallMs -gt 0) {
            $cpuSamples += (($cpuMs - $lastCpuMs) / $wallMs / [Environment]::ProcessorCount * 100.0)
        }
        $lastCpuMs = $cpuMs
        $lastCpuAt = $now

        $gpu = Get-ProcessGpuPercent -ProcessId $proc.Id
        if ($null -ne $gpu) {
            $gpuSamples += $gpu
        }
    }

    $run = Capture-Window -Hwnd $hwnd -Path $runPath
    $frameSamples = Read-FrameSamples -Path $frameLog
    $latestFrame = if ($frameSamples.Count -gt 0) { $frameSamples[-1] } else { $null }
    $runningSamples = @($frameSamples | Where-Object { $_.taskPhase -eq 'running' -or $_.taskPhase -eq 'validating' })
    $cpuStats = $cpuSamples | Measure-Object -Average -Maximum
    $gpuStats = $gpuSamples | Measure-Object -Average -Maximum
    $maxDtStats = $frameSamples | Measure-Object -Property maxDtMs -Maximum
    $slowDeltaStats = $frameSamples | Measure-Object -Property slowFrameRate -Average -Maximum

    [pscustomobject]@{
        pid = $proc.Id
        profile = if ($Release) { 'release' } else { 'debug' }
        bvid = $Bvid
        durationSeconds = $DurationSeconds
        frameSamples = $frameSamples.Count
        activeRunFrameSamples = $runningSamples.Count
        latestEmaFps = if ($latestFrame) { [Math]::Round([double]$latestFrame.emaFps, 2) } else { $null }
        maxDtMs = if ($maxDtStats.Maximum) { [Math]::Round([double]$maxDtStats.Maximum, 3) } else { $null }
        averageSlowFrameRate = if ($slowDeltaStats.Average) { [Math]::Round([double]$slowDeltaStats.Average, 4) } else { $null }
        maxSlowFrameRate = if ($slowDeltaStats.Maximum) { [Math]::Round([double]$slowDeltaStats.Maximum, 4) } else { $null }
        averageCpuPercent = if ($cpuStats.Average) { [Math]::Round([double]$cpuStats.Average, 2) } else { $null }
        maxCpuPercent = if ($cpuStats.Maximum) { [Math]::Round([double]$cpuStats.Maximum, 2) } else { $null }
        averageGpuPercent = if ($gpuStats.Average) { [Math]::Round([double]$gpuStats.Average, 2) } else { $null }
        maxGpuPercent = if ($gpuStats.Maximum) { [Math]::Round([double]$gpuStats.Maximum, 2) } else { $null }
        enterClick = $enterClick
        startClick = $startClick
        entry = $entry.path
        workbench = $workbench.path
        run = $run.path
        frameLog = $frameLog
        outputRoot = $outputRoot
        stdout = $stdout
        stderr = $stderr
        width = $run.width
        height = $run.height
    } | ConvertTo-Json -Compress -Depth 5
} finally {
    if ($null -ne $proc -and -not $proc.HasExited) {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }

    if ($null -eq $oldFrameStatsPath) {
        Remove-Item Env:\BILI_GUI_FRAME_STATS_PATH -ErrorAction SilentlyContinue
    } else {
        $env:BILI_GUI_FRAME_STATS_PATH = $oldFrameStatsPath
    }

    if ($null -eq $oldSmokeBvids) {
        Remove-Item Env:\BILI_GUI_SMOKE_BVIDS -ErrorAction SilentlyContinue
    } else {
        $env:BILI_GUI_SMOKE_BVIDS = $oldSmokeBvids
    }

    if ($null -eq $oldSmokeOutput) {
        Remove-Item Env:\BILI_GUI_SMOKE_OUTPUT -ErrorAction SilentlyContinue
    } else {
        $env:BILI_GUI_SMOKE_OUTPUT = $oldSmokeOutput
    }

    if (-not $KeepArtifacts) {
        Start-Sleep -Milliseconds 200
        Remove-Item -LiteralPath $entryPath, $workbenchPath, $runPath, $stdout, $stderr, $frameLog -Force -ErrorAction SilentlyContinue
    }
}
