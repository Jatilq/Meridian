$ErrorActionPreference = 'Continue'

# 1) Clear vue-tsc incremental cache so we get a fresh build verdict
$tscfg = 'E:\ai\Projects\Meridian\tsconfig.json'
$root = 'E:\ai\Projects\Meridian'

Get-ChildItem -Path $root -Recurse -Filter '*.tsbuildinfo' -Force -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host ("removed tsbuildinfo: {0}" -f $_.FullName); Remove-Item -Force $_.FullName }

# Also clear any .vite cache (Vite's transform cache)
Get-ChildItem -Path (Join-Path $root 'node_modules') -Recurse -Directory -Filter '.vite' -Force -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host ("vite cache: {0}" -f $_.FullName) }

# 2) Run fresh type-check
Write-Host '---'
Write-Host 'Running: npm run type-check'
Write-Host '---'

& 'E:\ai\apps\pinokio\bin\miniconda\pkgs\nodejs-22.21.1-he453025_2\npm.cmd' run type-check 2>&1 | Tee-Object -Variable typecheckOut | Out-Host
$ec = $LASTEXITCODE

Write-Host '---'
Write-Host ('TYPECHECK_EXIT={0}' -f $ec)

$bad = ($typecheckOut | Select-String -Pattern 'Element is missing end tag|vue-?template.*Error|Parse error' -SimpleMatch -CaseSensitive:$false)
if ($bad) {
    Write-Host '---'
    Write-Host 'BAD_LINES_FOUND:'
    $bad | ForEach-Object { Write-Host $_.ToString() }
} else {
    Write-Host 'NO_TEMPLATE_ERRORS_DETECTED'
}
