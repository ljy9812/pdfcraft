# p2 G7.4 sustained-interaction storm v2: alternating taps + touch-scroll drags
# with per-segment hilog capture (app tag + system input-delivery tag),
# mid-segment screenshot (panel-open = click processed) and end screenshot.
# PS 5.1 discipline: no 2>&1, no &&, explicit -Encoding utf8.
param([int]$From = 5, [int]$To = 10)

$dir = "D:\xuqiu\egui\ohos-spike\frame-spike"

for ($n = $From; $n -le $To; $n++) {
    $t0 = Get-Date
    hdc shell "hilog -r" | Out-Null

    # I1: scroll down (finger up)
    hdc shell "uitest uiInput drag 1460 1400 1460 750" | Out-Null
    Start-Sleep -Milliseconds 3000
    # I2: rail Pages button -> right panel opens
    hdc shell "uitest uiInput click 3080 285" | Out-Null
    Start-Sleep -Milliseconds 2500
    # mid-segment frame: panel open proves the click was processed
    hdc shell "snapshot_display -f /data/local/tmp/p2g74-storm-mid.jpeg" | Out-Null
    hdc file recv /data/local/tmp/p2g74-storm-mid.jpeg "$dir\p2g74-storm$n-mid.jpeg" | Out-Null
    # I3: panel close button
    hdc shell "uitest uiInput click 3002 130" | Out-Null
    Start-Sleep -Milliseconds 2500
    # I4: scroll up (finger down)
    hdc shell "uitest uiInput drag 1460 750 1460 1400" | Out-Null
    Start-Sleep -Milliseconds 3000
    # I5: page tap (press-on-page, no chrome change)
    hdc shell "uitest uiInput click 1460 1000" | Out-Null
    Start-Sleep -Milliseconds 2500
    # I6: scroll down again
    hdc shell "uitest uiInput drag 1460 1400 1460 750" | Out-Null
    Start-Sleep -Milliseconds 3000
    $t1 = Get-Date

    # end-of-segment time-lapse frame
    hdc shell "snapshot_display -f /data/local/tmp/p2g74-storm.jpeg" | Out-Null
    hdc file recv /data/local/tmp/p2g74-storm.jpeg "$dir\p2g74-storm$n.jpeg" | Out-Null

    # dump: app tag + system input delivery tag, drop wgpu trace flood
    $log = "$dir\p2g74-storm-seg$n.log"
    hdc shell "hilog -x -T OHOS_SPIKE,InputKeyFlow" | Where-Object { $_ -notmatch "wgpu" } | Set-Content -Encoding utf8 $log
    $f = @(Get-Content $log)
    $counters = @($f | Select-String "pump counters")
    $fw = @($f | Select-String "\[index\] forward")
    $mmi = @($f | Select-String "InputKeyFlow")
    "seg $n : lines=$($f.Count) counters=$($counters.Count) forward=$($fw.Count) mmi=$($mmi.Count) | interaction $([int]($t1-$t0).TotalSeconds)s"
}
