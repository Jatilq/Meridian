$ErrorActionPreference = 'Continue'
$root = 'E:\ai\Projects\Meridian'
$shotDir = Join-Path $root 'screenshots'
New-Item -ItemType Directory -Force -Path $shotDir | Out-Null

$npm = 'E:\ai\apps\pinokio\bin\miniconda\pkgs\nodejs-22.21.1-he453025_2\npm.cmd'
$outLog = Join-Path $shotDir 'dev-stdout.log'
$errLog = Join-Path $shotDir 'dev-stderr.log'

if (Test-Path $outLog) { Remove-Item $outLog -Force }
if (Test-Path $errLog) { Remove-Item $errLog -Force }

$proc = Start-Process -FilePath $npm `
    -ArgumentList 'run tauri:dev' `
    -WorkingDirectory $root `
    -RedirectStandardOutput $outLog `
    -RedirectStandardError $errLog `
    -WindowStyle Hidden `
    -PassThru

Write-Host ("NPM_PID={0}" -f $proc.Id)
Write-Host ("STARTED={0:o}" -f (Get-Date))
Write-Host ("OUT_LOG={0}" -f $outLog)
Write-Host ("ERR_LOG={0}" -f $errLog)
