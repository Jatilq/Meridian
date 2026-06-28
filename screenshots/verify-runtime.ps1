$ErrorActionPreference = 'Continue'
$root = 'E:\ai\Projects\Meridian'
$shotDir = Join-Path $root 'screenshots'

# === 1) Kill the existing dev tree (conservative) ===
Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in 'sigma-file-manager','Meridian' } |
    ForEach-Object { Stop-Process -Id $_.Id -Force }

$threshold = (Get-Date).AddMinutes(-10)
Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in 'cargo','rustc','link','cl','gcc','cc1' -and $_.StartTime -lt $threshold } |
    ForEach-Object { Stop-Process -Id $_.Id -Force }

Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -eq 'npm.cmd' -and $_.CommandLine -match 'run tauri' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

Start-Sleep -Seconds 2

foreach ($name in 'dev-stdout.log','dev-stderr.log') {
    $p = Join-Path $shotDir $name
    if (Test-Path $p) {
        try { Remove-Item $p -Force -ErrorAction Stop; Write-Host ("removed {0}" -f $name) }
        catch { Write-Host ("could not remove {0}: {1}" -f $name, $_.Exception.Message) }
    }
}

# === 2) Launch fresh tauri:dev (detached) ===
$npm = 'E:\ai\apps\pinokio\bin\miniconda\pkgs\nodejs-22.21.1-he453025_2\npm.cmd'
$outLog = Join-Path $shotDir 'dev-stdout.log'
$errLog = Join-Path $shotDir 'dev-stderr.log'

$proc = Start-Process -FilePath $npm `
    -ArgumentList 'run tauri:dev' `
    -WorkingDirectory $root `
    -RedirectStandardOutput $outLog `
    -RedirectStandardError $errLog `
    -WindowStyle Hidden `
    -PassThru
Write-Host ('NPM_PID={0}' -f $proc.Id)

# === 3) Wait for the Tauri webview to launch, polling up to 4 minutes ===
$deadline = (Get-Date).AddMinutes(4)
$everUp = $false
while ((Get-Date) -lt $deadline) {
    $s = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -in 'sigma-file-manager','Meridian' }
    if ($s) {
        $everUp = $true
        Write-Host ('SIGMA_UP_AT={0:o}' -f (Get-Date))
        break
    }
    Start-Sleep -Seconds 5
}

# === 4) Give Vite an extra warm-up window to attempt any pre-transform ===
Start-Sleep -Seconds 8

# === 5) Report verdict on the stderr log ===
Write-Host '---'
Write-Host ('SIGMA_EVER_UP={0}' -f $everUp)
Write-Host ('STDERR_BYTES={0}' -f (Get-Item $errLog).Length)
Write-Host '---'
Write-Host 'VITE_ERROR_KEYWORDS_IN_STDERR:'
$bad = Select-String -Path $errLog -Pattern 'Element is missing end tag','Internal server error','Parse error','vue-?template.*Error' -SimpleMatch -CaseSensitive:$false
if ($bad) {
    Write-Host 'FOUND_BAD_LINES:'
    $bad | Select-Object -First 30 | ForEach-Object { Write-Host ('  L{0}: {1}' -f $_.LineNumber, $_.Line) }
    Write-Host '---'
    Write-Host 'TYPECHECK_VERDICT_AND_RUNTIME_DISAGREE'
} else {
    Write-Host 'NO_TEMPLATE_OR_VITE_ERROR_KEYWORDS_PRESENT'
}

# === 6) List visible windows for the user-facing report ===
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public class Win32 {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lp, IntPtr l);
    public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
}
"@
$ws = New-Object System.Collections.ArrayList
$cb = [Win32+EnumWindowsProc]{ param($h,$l)
    if ([Win32]::IsWindowVisible($h)) {
        $len = [Win32]::GetWindowTextLength($h)
        if ($len -gt 0) {
            $sb = New-Object System.Text.StringBuilder ($len+1)
            [Win32]::GetWindowText($h, $sb, $sb.Capacity) | Out-Null
            [void]$ws.Add([PSCustomObject]@{ Title = $sb.ToString() })
        }
    }
    return $true
}
[Win32]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
Write-Host '---'
Write-Host 'VISIBLE_MERIDIAN_WINDOWS:'
$ws | Where-Object { $_.Title -match 'Meridian|sigma|Cluster|Backend|Tauri|RPC' } |
    Sort-Object Title | Format-Table Title -AutoSize | Out-String | Write-Host
