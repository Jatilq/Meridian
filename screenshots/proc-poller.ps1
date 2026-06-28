$ErrorActionPreference = 'Continue'
$shotDir = 'E:\ai\Projects\Meridian\screenshots'
$log = Join-Path $shotDir 'proc-events.csv'

# Baseline: snapshot all pids at start
$baseline = Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CreationDate
"timestamp,event,pid,ppid,name,cmd,windowtitle" | Out-File -FilePath $log -Encoding utf8

$baselineFile = Join-Path $shotDir 'proc-baseline.json'
$baseline | ConvertTo-Json -Depth 2 | Out-File -FilePath $baselineFile -Encoding utf8

# Anchor root pids: tauri dev started chain (every node + every rustc/cargo we see now)
$rootPids = @()
$rootPids += ($baseline | Where-Object { $_.Name -in 'node.exe','rustc.exe','cargo.exe','link.exe','cmd.exe','sigma-file-manager.exe','Meridian.exe' } | ForEach-Object { $_.ProcessId })

Write-Host ("Monitoring {0} root pids for {1}s" -f $rootPids.Count, 60)
$deadline = (Get-Date).AddSeconds(60)
$flashHits = @()

while ((Get-Date) -lt $deadline) {
    $now = Get-CimInstance Win32_Process | Where-Object {
        $_.Name -in 'cmd.exe','powershell.exe','pwsh.exe','conhost.exe','wt.exe','powershell_ise.exe' -or
        ($_.Name -notin @('System','Idle','smss.exe','csrss.exe','wininit.exe','services.exe','lsass.exe','svchost.exe','dwm.exe','fontdrvhost.exe','WmiPrvSe.exe','taskmgr.exe','explorer.exe','dllhost.exe','SearchHost.exe','SearchIndexer.exe','ShellExperienceHost.exe','SecurityHealthService.exe','spoolsv.exe','MsMpEng.exe'))
    }

    foreach ($p in $now) {
        # Only interesting if it's a child of our root chain or unattributed
        if ($p.ProcessId -in $rootPids) { continue }
        $owningRoot = $false
        $pp = $p.ParentProcessId
        $depth = 0
        while ($pp -gt 0 -and $depth -lt 10) {
            if ($pp -in $rootPids) { $owningRoot = $true; break }
            $parent = $now | Where-Object { $_.ProcessId -eq $pp } | Select-Object -First 1
            if (-not $parent) { break }
            $pp = $parent.ParentProcessId
            $depth++
        }
        if ($owningRoot) {
            $line = ('{0:o},FOUND,{1},{2},{3},"{4}",""' -f (Get-Date), $p.ProcessId, $p.ParentProcessId, $p.Name, ($p.CommandLine -replace '"','""'))
            Add-Content -Path $log -Value $line -Encoding utf8
            if ($p.Name -in 'cmd.exe','powershell.exe','pwsh.exe','wt.exe','powershell_ise.exe','conhost.exe') {
                $flashHits += [PSCustomObject]@{ Time=(Get-Date); Pid=$p.ProcessId; Ppid=$p.ParentProcessId; Name=$p.Name; Cmd=$p.CommandLine }
            }
        }
    }
    Start-Sleep -Milliseconds 500
}

"timestamp,event,summary" | Out-File -FilePath (Join-Path $shotDir 'proc-summary.txt') -Encoding utf8
$count = $flashHits.Count
Add-Content -Path (Join-Path $shotDir 'proc-summary.txt') -Value ("{0:o},CMD_FLASH_COUNT,{1}" -f (Get-Date), $count) -Encoding utf8
foreach ($h in $flashHits) {
    Add-Content -Path (Join-Path $shotDir 'proc-summary.txt') -Value ("{0:o},CMD_FLASH,{1} pid={2} ppid={3} cmd={4}" -f $h.Time, $h.Name, $h.Pid, $h.Ppid, $h.Cmd) -Encoding utf8
}
Write-Host ("CMD_FLASH_COUNT={0}" -f $count)
foreach ($h in $flashHits) { Write-Host ("FLASH: {0} pid={1} ppid={2} cmd={3}" -f $h.Name, $h.Pid, $h.Ppid, $h.Cmd) }
