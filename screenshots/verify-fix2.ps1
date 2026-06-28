$lines = [System.IO.File]::ReadAllLines('E:\ai\Projects\Meridian\src\modules\ai-panel\ai-panel.vue')
'Total lines: ' + $lines.Length
'---'
# Confirm both fixes are in place
for ($i = 794; $i -lt 836 -and $i -lt $lines.Length; $i++) {
    '{0,4}: {1}' -f ($i + 1), $lines[$i]
}
