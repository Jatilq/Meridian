$ErrorActionPreference = 'SilentlyContinue'
$root = 'E:\ai\Projects\Meridian'
$shotDir = Join-Path $root 'screenshots'
$ts = Get-Date -Format 'HH:mm:ss'

Write-Host ("=== STATUS @ {0} ===" -f $ts)

Write-Host '--- log files ---'
foreach ($name in 'dev-stdout.log','dev-stderr.log','launch.ps1') {
    $p = Join-Path $shotDir $name
    if (Test-Path $p) {
        $f = Get-Item $p
        Write-Host ("{0}  size={1}  mtime={2}" -f $name, $f.Length, $f.LastWriteTime)
    } else {
        Write-Host ("{0}  (missing)" -f $name)
    }
}

Write-Host '--- dev-stdout tail (50) ---'
$out = Join-Path $shotDir 'dev-stdout.log'
if (Test-Path $out) { Get-Content $out -Tail 50 }
else { Write-Host '(none)' }

Write-Host '--- dev-stderr tail (50) ---'
$err = Join-Path $shotDir 'dev-stderr.log'
if (Test-Path $err) { Get-Content $err -Tail 50 }
else { Write-Host '(none)' }

Write-Host '--- processes of interest ---'
$targets = @('node','rustc','cargo','link','sigma-file-manager','Meridian','WebView2','cmd')
Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in $targets } |
    Select-Object Id,ProcessName,StartTime |
    Sort-Object ProcessName,StartTime |
    Format-Table -AutoSize

Write-Host '--- parent-child chain (top 30) ---'
$procs = Get-CimInstance Win32_Process -Filter "Name IN ('node.exe','rustc.exe','cargo.exe','link.exe','sigma-file-manager.exe','Meridian.exe','cmd.exe','powershell.exe')"
$procs | Select-Object ProcessId,ParentProcessId,Name,CreationDate | Sort-Object CreationDate | Select-Object -First 30 | Format-Table -AutoSize
