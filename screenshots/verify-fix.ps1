$lines = [System.IO.File]::ReadAllLines('E:\ai\Projects\Meridian\src\modules\ai-panel\ai-panel.vue')
'Total lines: ' + $lines.Length
'---'
# Show lines 813-826 to confirm whether the fix landed
for ($i = 812; $i -lt 826 -and $i -lt $lines.Length; $i++) {
    '{0,4}: {1}' -f ($i + 1), $lines[$i]
}
'---'
# Quick balance check: count <div vs </div in the template block (lines 691-962)
$open = 0
$close = 0
$self = 0
for ($i = 690; $i -lt 962; $i++) {
    $l = $lines[$i]
    # crude: opening <div not />; closing </div>; self-closing <div.../>
    $opens = ([regex]::Matches($l, '<div(?:\s|>)')).Count
    $closes = ([regex]::Matches($l, '</div\s*>')).Count
    $selfClosing = ([regex]::Matches($l, '<div[^>]*/>')).Count
    $open += $opens
    $close += $closes
    $self += $selfClosing
}
'Template div balance check (template block only):'
"  raw <div> opens = $open"
"  raw </div> closes = $close"
"  self-closing <div.../> = $self"
"  net unclosed = " + ($open - $close - $self)
