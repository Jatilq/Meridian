$shotDir = 'E:\ai\Projects\Meridian\screenshots'
'=== backend-manager.vue modelsDir sites ==='
$bm = 'E:\ai\Projects\Meridian\src\modules\backend-manager\pages\backend-manager.vue'
$lines = [System.IO.File]::ReadAllLines($bm)
for ($i = 0; $i -lt $lines.Length; $i++) {
    if ($lines[$i] -match 'modelsDir') {
        '{0,4}: {1}' -f ($i + 1), $lines[$i]
    }
}
'=== schemas/user-settings.ts schema version ==='
$sch = (Get-Content 'E:\ai\Projects\Meridian\src\stores\schemas\user-settings.ts' -Raw) -split "`n"
for ($i = 0; $i -lt $sch.Length; $i++) {
    if ($sch[$i] -match 'USER_SETTINGS_SCHEMA_VERSION\b') {
        '{0,4}: {1}' -f ($i + 1), $sch[$i]
    }
}
