$lines = [System.IO.File]::ReadAllLines('E:\ai\Projects\Meridian\src\modules\backend-manager\pages\backend-manager.vue')
'Total lines: ' + $lines.Length
'---'
for ($i = 0; $i -lt $lines.Length; $i++) {
    if ($lines[$i] -match 'modelsDir') {
        '{0,4}: {1}' -f ($i + 1), $lines[$i]
    }
}
