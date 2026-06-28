$lines = [System.IO.File]::ReadAllLines('E:\ai\Projects\Meridian\src\modules\cluster\pages\cluster.vue')
'Total lines: ' + $lines.Length
'---'
for ($i = 0; $i -lt 340 -and $i -lt $lines.Length; $i++) {
    '{0,4}: {1}' -f ($i + 1), $lines[$i]
}
