# Analyze storm segment logs: per-second line activity + forward/counter lines
# (full text, reassembling hilog's wrapped lines).
param([string]$Seg = "1")

$dir = "D:\xuqiu\egui\ohos-spike\frame-spike"
$log = "$dir\p2g74-storm-seg$Seg.log"

# Reassemble wrapped hilog lines: continuation lines start with spaces.
$raw = Get-Content $log
$joined = New-Object System.Collections.Generic.List[string]
foreach ($l in $raw) {
    if ($l -match '^\s+\S' -and $joined.Count -gt 0) { $joined[$joined.Count-1] = ($joined[$joined.Count-1] + $l.Trim()) }
    else { $joined.Add($l) }
}

# Per-second activity buckets from timestamped lines
$secs = @{}
foreach ($l in $joined) {
    if ($l -match '^10-21 (\d\d:\d\d:\d\d)') {
        $k = $Matches[1]
        if (-not $secs.ContainsKey($k)) { $secs[$k] = 0 }
        $secs[$k]++
    }
}
"segment $Seg : $($joined.Count) lines, $($secs.Count) distinct seconds with lines"
$sorted = $secs.Keys | Sort-Object
$sb = New-Object System.Text.StringBuilder
foreach ($k in $sorted) { [void]$sb.Append("$k=$($secs[$k]) ") }
"activity: $($sb.ToString())"
""
"forward lines (full):"
$joined | Select-String "forward" | ForEach-Object { $_.Line }
""
"pump counters (full):"
$joined | Select-String "pump counters" | ForEach-Object { $_.Line }
