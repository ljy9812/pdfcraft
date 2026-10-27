# build-hap.ps1 — build (and optionally install) the Release HAP, Windows toolchain.
# Usage: .\scripts\build-hap.ps1 [-NoInstall]
#
# Signing: the committed tree stays signing-free ("signingConfigs": [] in
# build-profile.json5). At build time a signing fragment from local\signing.json
# (gitignored, machine-specific) is injected and the pristine file restored
# afterwards. Without it you get an unsigned HAP (hdc install will reject it).
param(
    [switch]$NoInstall
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot   # packaging/ohos
$rawIndex = Join-Path $root 'entry\src\main\resources\rawfile\web\index.html'
if (-not (Test-Path $rawIndex)) { throw "rawfile\web\index.html missing (run scripts\fetch-web.ps1 first)" }

# Toolchain: DevEco Studio install root hosts tools\{ohpm,hvigor}; the SDK root is sdk\
# (ohpm/hvigor live under the install root's tools\, NOT under sdk\tools\).
$deveco = 'C:\Program Files\Huawei\DevEco Studio'
if (-not $env:DEVECO_SDK_HOME) { $env:DEVECO_SDK_HOME = "$deveco\sdk" }
$ohpm = "$deveco\tools\ohpm\bin\ohpm.bat"
if (-not (Test-Path $ohpm)) { throw "ohpm.bat not found at $ohpm (adjust `$deveco for your install)" }
try { Get-Command hvigorw -ErrorAction Stop | Out-Null } catch {
    $env:PATH = "$deveco\tools\hvigor\bin;$env:PATH"
}

# 1) Signing injection (pristine -> local signed form; restored after the build)
$bp = Join-Path $root 'build-profile.json5'
$frag = Join-Path $root 'local\signing.json'
$bpBak = "$bp.bak"
$signed = $false
if (Test-Path $frag) {
    Copy-Item $bp $bpBak -Force
    $fragText = [System.IO.File]::ReadAllText($frag).Trim()
    $bpText = [System.IO.File]::ReadAllText($bp)
    $injected = $bpText.Replace('"signingConfigs": []', '"signingConfigs": [' + "`n    " + $fragText + "`n  ]")
    if ($injected -eq $bpText) { Remove-Item $bpBak -Force; throw 'pristine anchor "signingConfigs": [] not found in build-profile.json5' }
    [System.IO.File]::WriteAllText($bp, $injected)
    $signed = $true
    Write-Output 'signing: injected from local\signing.json'
} else {
    Write-Warning 'local\signing.json not found -> building an UNSIGNED HAP'
}

try {
    # 2) ohpm deps + 3) hvigor Release HAP
    # (PS 5.1 + $ErrorActionPreference='Stop' wraps native stderr warnings as
    #  terminating NativeCommandError; success is judged by $LASTEXITCODE.)
    $ErrorActionPreference = 'Continue'
    Push-Location $root
    try {
        & $ohpm install --all
        if ($LASTEXITCODE -ne 0) { throw "ohpm install failed (exit $LASTEXITCODE)" }
        hvigorw assembleHap --mode module -p product=default -p buildMode=release
        if ($LASTEXITCODE -ne 0) { throw "hvigor assembleHap failed (exit $LASTEXITCODE)" }
    } finally { Pop-Location }
} finally {
    $ErrorActionPreference = 'Stop'
    if ($signed -and (Test-Path $bpBak)) {
        Move-Item $bpBak $bp -Force
        Write-Output 'signing: build-profile.json5 restored to pristine form'
    }
}

# 4) Locate the artifact
$hap = Join-Path $root 'entry\build\default\outputs\default\entry-default-signed.hap'
if (-not (Test-Path $hap)) {
    $hap = Join-Path $root 'entry\build\default\outputs\default\entry-default-unsigned.hap'
    if (Test-Path $hap) { Write-Warning 'no signing config: using UNSIGNED HAP' }
}
if (-not (Test-Path $hap)) { throw "HAP not found at expected path: $hap" }
Write-Output ("HAP: {0} ({1:N0} B)" -f $hap, (Get-Item $hap).Length)

# 5) Install to the connected device
if (-not $NoInstall) {
    hdc install -r $hap
    if ($LASTEXITCODE -ne 0) { throw "hdc install failed (exit $LASTEXITCODE)" }
    Write-Output 'installed to device'
}
