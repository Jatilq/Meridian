# Find all tag boundaries, then print a window around the suspect classes
$path = 'E:\ai\Projects\Meridian\src\modules\ai-panel\ai-panel.vue'
$lines = [System.IO.File]::ReadAllLines($path)
'Total lines: ' + $lines.Length
'---'
# Boundaries
for ($i = 0; $i -lt $lines.Length; $i++) {
    if ($lines[$i] -match '^\s*<(template|script|style)\b' -or
        $lines[$i] -match '^\s*</(template|script|style)>') {
        '{0,4}: {1}' -f ($i + 1), $lines[$i]
    }
}
'---'
# find exact lines that open or close an ai-panel confirm / onboarding-skip div
for ($i = 0; $i -lt $lines.Length; $i++) {
    $l = $lines[$i]
    if ($l -match 'class="[^"]*ai-panel__(confirm|onboarding-skip)') {
        '{0,4}: {1}' -f ($i + 1), $l
    }
    elseif ($l -match 'class="[^"]*ai-panel__(confirm|onboarding-skip)') {
        '{0,4}: {1}' -f ($i + 1), $l
    }
}
'---'
# also show divs around any v-if toolConfirmDetails
for ($i = 0; $i -lt $lines.Length; $i++) {
    if ($lines[$i] -match 'toolConfirmDetails') {
        '{0,4}: {1}' -f ($i + 1), $lines[$i]
    }
}
