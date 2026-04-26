# Drive rust2xml-gui through its main flows and capture store screenshots.
#
# - Launches the release binary.
# - Waits for the window, resizes it to 1366x768.
# - Captures the empty initial state.
# - Clicks "Run -e (Extended)" via simulated mouse input.
# - Waits for the SQLite file to appear under ~/rust2xml/sqlite/.
# - Captures the populated tab view.
# - Switches tabs by clicking tab strip cells, captures each.
# - Types into the search box, captures filtered view.
#
# Output: 1366x768 PNGs into the same directory as this script.

param(
    [string]$Exe = "C:\Users\zdava\Documents\software\rust2xml\target\release\rust2xml-gui.exe",
    [int]$Width = 1366,
    [int]$Height = 768,
    [int]$RunTimeoutSec = 600
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

$signature = @'
using System;
using System.Runtime.InteropServices;
public static class Win32 {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, uint dwData, IntPtr dwExtraInfo);
    [DllImport("user32.dll")] public static extern short VkKeyScan(char ch);
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, IntPtr dwExtraInfo);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
}
'@
if (-not ('Win32' -as [type])) { Add-Type -TypeDefinition $signature }

function Get-GuiWindow {
    for ($i = 0; $i -lt 60; $i++) {
        $p = Get-Process -Name rust2xml-gui -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if ($p) { return $p }
        Start-Sleep -Milliseconds 500
    }
    throw "rust2xml-gui window did not appear within 30 seconds"
}

function Resize-Window([IntPtr]$hwnd, [int]$w, [int]$h) {
    [Win32]::ShowWindow($hwnd, 9) | Out-Null
    [Win32]::SetForegroundWindow($hwnd) | Out-Null
    # Move to (40, 40) so the whole window stays on screen even at 4K scale.
    # SWP_NOZORDER=0x0004, SWP_SHOWWINDOW=0x0040
    [Win32]::SetWindowPos($hwnd, [IntPtr]::Zero, 40, 40, $w, $h, 0x0044) | Out-Null
    Start-Sleep -Milliseconds 600
}

function Get-WindowRect([IntPtr]$hwnd) {
    $r = New-Object Win32+RECT
    [Win32]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    return $r
}

function Get-ClientOrigin([IntPtr]$hwnd) {
    $p = New-Object Win32+POINT
    [Win32]::ClientToScreen($hwnd, [ref]$p) | Out-Null
    return $p
}

function Capture([IntPtr]$hwnd, [string]$name) {
    $r = Get-WindowRect $hwnd
    $w = $r.Right - $r.Left
    $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, [System.Drawing.Size]::new($w, $h))
    $g.Dispose()
    $out = Join-Path $ScriptDir ("$name.png")
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "Saved $out  ($w x $h)"
}

function Click-At([int]$screenX, [int]$screenY) {
    [Win32]::SetCursorPos($screenX, $screenY) | Out-Null
    Start-Sleep -Milliseconds 80
    # MOUSEEVENTF_LEFTDOWN = 0x02, MOUSEEVENTF_LEFTUP = 0x04
    [Win32]::mouse_event(0x02, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [Win32]::mouse_event(0x04, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 200
}

function Type-Text([string]$text) {
    [System.Windows.Forms.SendKeys]::SendWait($text)
    Start-Sleep -Milliseconds 200
}

# --- launch ---
$dataRoot = Join-Path $env:USERPROFILE "rust2xml\sqlite"
$beforeFiles = @()
if (Test-Path $dataRoot) {
    $beforeFiles = Get-ChildItem $dataRoot -Filter "rust2xml_e_*.sqlite" -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName }
}

Write-Host "Launching $Exe"
$gui = Start-Process -FilePath $Exe -PassThru
$proc = Get-GuiWindow
$hwnd = $proc.MainWindowHandle
Resize-Window $hwnd $Width $Height

# Initial empty state.
Capture $hwnd "01-empty"

# Click "Run -e (Extended)".  Button is the first item in the top-bar
# horizontal row, min_size 220x36, with 6px top padding inside the panel.
$client = Get-ClientOrigin $hwnd
$btnX = $client.X + 110           # half of 220
$btnY = $client.Y + 6 + 18 + 8    # top padding + half of 36 + window inset
Click-At $btnX $btnY

# Capture an early "in progress" frame — log + spinner visible.
Start-Sleep -Seconds 3
Capture $hwnd "02-running"

# Wait for a fresh SQLite file to appear (the GUI writes it on Done).
$sqlite = $null
$deadline = (Get-Date).AddSeconds($RunTimeoutSec)
while ((Get-Date) -lt $deadline) {
    if (Test-Path $dataRoot) {
        $candidate = Get-ChildItem $dataRoot -Filter "rust2xml_e_*.sqlite" -ErrorAction SilentlyContinue |
            Where-Object { $beforeFiles -notcontains $_.FullName } |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if ($candidate -and ((Get-Date) - $candidate.LastWriteTime).TotalSeconds -gt 2) {
            # File hasn't been written to in 2s → GUI Done.
            $sqlite = $candidate.FullName
            break
        }
    }
    Start-Sleep -Seconds 2
}
if (-not $sqlite) { throw "Run did not finish within $RunTimeoutSec seconds." }
Write-Host "DB ready: $sqlite"
# Let the GUI render the populated tabs.
Start-Sleep -Seconds 4
Capture $hwnd "03-tabs-loaded"

# Tab strip y is right under the bottom of the controls panel.  In the
# default layout the controls panel is ~110 px tall (buttons + DB row +
# padding), so the tab strip lands at client-y ≈ 122.  Click the second
# tab if there is one — it shifts the active selection visibly so the
# screenshot doesn't look identical to 03.
$tabsY = $client.Y + 130
Click-At ($client.X + 220) $tabsY     # roughly second tab
Start-Sleep -Seconds 2
Capture $hwnd "04-tab-second"

Click-At ($client.X + 360) $tabsY     # third tab area
Start-Sleep -Seconds 2
Capture $hwnd "05-tab-third"

Click-At ($client.X + 500) $tabsY     # fourth tab area
Start-Sleep -Seconds 2
Capture $hwnd "06-tab-fourth"

# Search box demonstration.  Search row sits below the tab strip and the
# separator (~28 px combined).
$searchY = $client.Y + 168
Click-At ($client.X + 400) $searchY
Start-Sleep -Milliseconds 300
Type-Text "PONSTAN"
Start-Sleep -Seconds 2
Capture $hwnd "07-search-filtered"

Write-Host "Done.  Screenshots in $ScriptDir"
# Always close the GUI we launched — leaving it running is intrusive.
Stop-Process -Id $gui.Id -Force -ErrorAction SilentlyContinue
Get-Process -Name rust2xml-gui -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
