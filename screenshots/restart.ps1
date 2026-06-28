$ErrorActionPreference = 'Continue'

# Conservative kill. Never target powershell.exe by commandline match (risk of killing our own parent).
# Only target: (a) the Tauri webview, (b) stale cargo/rustc/link, (c) the SPECIFIC npm.cmd that ran tauri:dev.

# (a) Kill the Tauri webview executable
Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in 'sigma-file-manager','Meridian' } |
    ForEach-Object { Write-Host ("KILL pid={0} name={1}" -f $_.Id, $_.ProcessName); Stop-Process -Id $_.Id -Force }

# (b) Kill cargo/rustc/link older than 10 minutes (these were running from earlier today)
$threshold = (Get-Date).AddMinutes(-10)
Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in 'cargo','rustc','link','cl','gcc','cc1' -and $_.StartTime -lt $threshold } |
    ForEach-Object { Write-Host ("KILL pid={0} name={1} start={2}" -f $_.Id, $_.ProcessName, $_.StartTime); Stop-Process -Id $_.Id -Force }

# (c) Kill npm.cmd processes whose commandLine contains 'run tauri:dev'
Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -eq 'npm.cmd' -and $_.CommandLine -match 'run tauri' } |
    ForEach-Object {
        $cl = $_.CommandLine
        if ($cl.Length -gt 80) { $cl = $cl.Substring(0,80) + '...' }
        Write-Host ("KILL pid={0} name={1} cmd={2}" -f $_.ProcessId, $_.Name, $cl)
        Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
    }

Start-Sleep -Seconds 2

# Force-close any leftover log file handles
$shotDir = 'E:\ai\Projects\Meridian\screenshots'
foreach ($name in 'dev-stdout.log','dev-stderr.log') {
    $p = Join-Path $shotDir $name
    if (Test-Path $p) {
        try { Remove-Item $p -Force -ErrorAction Stop; Write-Host ("removed {0}" -f $name) }
        catch { Write-Host ("could not remove {0}: {1}" -f $name, $_.Exception.Message) }
    }
}

Write-Host '--- surviving processes (Meridian-family) ---'
$survivors = Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in 'sigma-file-manager','Meridian','cargo','rustc' }
if ($survivors) {
    $survivors | Select-Object Id,ProcessName,StartTime | Format-Table -AutoSize
} else {
    Write-Host '(none)'
}
Write-Host 'KILL_DONE'
