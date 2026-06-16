param(
    [switch]$SkipBuild,
    [switch]$KeepArtifacts,
    [Nullable[int]]$EnterX = $null,
    [Nullable[int]]$EnterY = $null,
    [double]$EnterXRatio = 0.20,
    [double]$EnterYRatio = 0.45
)

$ErrorActionPreference = 'Stop'

if (-not $SkipBuild) {
    cargo build --features gui-gpui --bin bili-opinion-gui
}

Add-Type -AssemblyName System.Drawing
if (-not ('GuiSmokeWin' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class GuiSmokeWin {
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
    [GuiSmokeWin]::SetProcessDpiAwareness(2) | Out-Null
} catch {
    # Already set by the host process or unavailable on older Windows.
}

$tmp = Resolve-Path 'tmp' -ErrorAction SilentlyContinue
if (-not $tmp) {
    New-Item -ItemType Directory -Force tmp | Out-Null
    $tmp = Resolve-Path 'tmp'
}

$stdout = Join-Path $tmp 'gui-smoke-stdout.log'
$stderr = Join-Path $tmp 'gui-smoke-stderr.log'
$entryPath = Join-Path $tmp 'gui-smoke-entry.png'
$workbenchPath = Join-Path $tmp 'gui-smoke-workbench.png'
Remove-Item -LiteralPath $stdout, $stderr, $entryPath, $workbenchPath -Force -ErrorAction SilentlyContinue

$exe = Resolve-Path 'target\debug\bili-opinion-gui.exe'
$proc = Start-Process -FilePath $exe -WorkingDirectory (Get-Location) -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru

function Get-MainWindowHandle {
    param([System.Diagnostics.Process]$Process)

    for ($i = 0; $i -lt 30; $i++) {
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

function Capture-Window {
    param(
        [IntPtr]$Hwnd,
        [string]$Path
    )

    $rect = New-Object GuiSmokeWin+RECT
    $result = [GuiSmokeWin]::DwmGetWindowAttribute(
        $Hwnd,
        9,
        [ref]$rect,
        [System.Runtime.InteropServices.Marshal]::SizeOf([type][GuiSmokeWin+RECT])
    )
    if ($result -ne 0) {
        throw "DwmGetWindowAttribute failed: $result"
    }

    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -lt 300 -or $height -lt 300) {
        throw "Window bounds too small: $width x $height"
    }

    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, [System.Drawing.Size]::new($width, $height))
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }

    return [pscustomobject]@{
        path = $Path
        width = $width
        height = $height
        left = $rect.Left
        top = $rect.Top
    }
}

function Measure-ImageDifference {
    param(
        [string]$BeforePath,
        [string]$AfterPath
    )

    $before = [System.Drawing.Bitmap]::FromFile($BeforePath)
    $after = [System.Drawing.Bitmap]::FromFile($AfterPath)
    try {
        $width = [Math]::Min($before.Width, $after.Width)
        $height = [Math]::Min($before.Height, $after.Height)
        $stepX = [Math]::Max(1, [int]($width / 96))
        $stepY = [Math]::Max(1, [int]($height / 64))
        $total = 0.0
        $samples = 0

        for ($y = 0; $y -lt $height; $y += $stepY) {
            for ($x = 0; $x -lt $width; $x += $stepX) {
                $a = $before.GetPixel($x, $y)
                $b = $after.GetPixel($x, $y)
                $total += (
                    [Math]::Abs($a.R - $b.R) +
                    [Math]::Abs($a.G - $b.G) +
                    [Math]::Abs($a.B - $b.B)
                ) / 3.0
                $samples += 1
            }
        }

        if ($samples -eq 0) {
            return 0.0
        }
        return $total / $samples
    } finally {
        $before.Dispose()
        $after.Dispose()
    }
}

try {
    $hwnd = Get-MainWindowHandle -Process $proc
    [GuiSmokeWin]::ShowWindow($hwnd, 3) | Out-Null
    [GuiSmokeWin]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 800

    $entry = Capture-Window -Hwnd $hwnd -Path $entryPath

    $clickX = if ($EnterX.HasValue) {
        $EnterX.Value
    } else {
        $entry.left + [int][Math]::Round($entry.width * $EnterXRatio)
    }
    $clickY = if ($EnterY.HasValue) {
        $EnterY.Value
    } else {
        $entry.top + [int][Math]::Round($entry.height * $EnterYRatio)
    }

    [GuiSmokeWin]::SetCursorPos($clickX, $clickY) | Out-Null
    Start-Sleep -Milliseconds 120
    [GuiSmokeWin]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [GuiSmokeWin]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Seconds 2

    $workbench = Capture-Window -Hwnd $hwnd -Path $workbenchPath
    $imageDiff = Measure-ImageDifference -BeforePath $entryPath -AfterPath $workbenchPath
    if ($imageDiff -lt 4.0) {
        throw "Workbench capture is too similar to entry capture (mean RGB diff $([Math]::Round($imageDiff, 2))). The Enter Workbench click may have missed."
    }

    [pscustomobject]@{
        pid = $proc.Id
        hwnd = $hwnd.ToInt64()
        clickX = $clickX
        clickY = $clickY
        clickMode = if ($EnterX.HasValue -or $EnterY.HasValue) { 'absolute' } else { 'window-ratio' }
        imageDiff = [Math]::Round($imageDiff, 2)
        entry = $entry.path
        workbench = $workbench.path
        width = $workbench.width
        height = $workbench.height
    } | ConvertTo-Json -Compress
} finally {
    if (-not $proc.HasExited) {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }

    if (-not $KeepArtifacts) {
        Start-Sleep -Milliseconds 200
        Remove-Item -LiteralPath $entryPath, $workbenchPath, $stdout, $stderr -Force -ErrorAction SilentlyContinue
    }
}
