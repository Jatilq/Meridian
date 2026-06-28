$lines = [System.IO.File]::ReadAllLines('E:\ai\Projects\Meridian\src\modules\ai-panel\ai-panel.vue')
for ($i = 789; $i -lt 840 -and $i -lt $lines.Length; $i++) {
    '{0,4}: {1}' -f ($i + 1), $lines[$i]
}
