$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$shotDir = 'E:\ai\Projects\Meridian\screenshots'
New-Item -ItemType Directory -Force -Path $shotDir | Out-Null

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class Win32 {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
"@

# Enumerate top-level windows
$windows = New-Object System.Collections.ArrayList
$callback = [Win32+EnumWindowsProc]{
    param($hWnd, $lParam)
    if ([Win32]::IsWindowVisible($hWnd)) {
        $len = [Win32]::GetWindowTextLength($hWnd)
        if ($len -gt 0) {
            $sb = New-Object System.Text.StringBuilder ($len + 1)
            [Win32]::GetWindowText($hWnd, $sb, $sb.Capacity) | Out-Null
            $title = $sb.ToString()
            $pid = 0
            [Win32]::GetWindowThreadProcessId($hWnd, [ref]$pid) | Out-Null
            $rect = New-Object Win32+RECT
            [Win32]::GetWindowRect($hWnd, [ref]$rect) | Out-Null
            [void]$windows.Add([PSCustomObject]@{
                Hwnd    = $hWnd
                Title   = $title
                Pid     = $pid
                X       = $rect.Left
                Y       = $rect.Top
                W       = $rect.Right - $rect.Left
                H       = $rect.Bottom - $rect.Top
            })
        }
    }
    return $true
}
[Win32]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null

Write-Host "=== VISIBLE WINDOWS ==="
$windows | Where-Object { $_.Title -match 'Meridian|sigma|Cluster|Backend|Tauri|RPC|Vite' } |
    Sort-Object Title | Format-Table Title,Pid,X,Y,W,H -AutoSize

Write-Host "=== FOREGROUND ==="
$fg = [Win32]::GetForegroundWindow()
$sb = New-Object System.Text.StringBuilder 256
[Win32]::GetWindowText($fg, $sb, $sb.Capacity) | Out-Null
$fgpid = 0
[Win32]::GetWindowThreadProcessId($fg, [ref]$fgpid) | Out-Null
Write-Host ("FG_HWND={0} FG_TITLE='{1}' FG_PID={2}" -f $fg, $sb.ToString(), $fgpid)

# Capture full primary screen
$ts = Get-Date -Format 'yyyyMMdd-HHmmss'
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$path = Join-Path $shotDir ("screen-{0}.png" -f $ts)
$bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()
Write-Host ("SCREENSHOT={0}" -f $path)

# capture last 30 lines of vite log
$err = Join-Path $shotDir 'dev-stderr.log'
if (Test-Path $err) {
    Write-Host "=== VITE STDERR TAIL (30 unique) ==="
    Get-Content $err -Tail 200 | Select-Object -Unique | Select-Object -Last 30
}
