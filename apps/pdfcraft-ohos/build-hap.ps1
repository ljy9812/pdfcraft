# pdfcraft-ohos release HAP delivery script (ohos-craft-p2 task 7.1 / design
# D12; ported from ohos-spike/frame-spike/build-hap.ps1): the reproducible
# delivery chain for the CI-unreachable environment. Chain: cargo release
# cross-build -> stage the .so where hvigor packs it -> hvigor release
# assembleHap -> hdc install. Every stage is incremental, so consecutive runs
# are idempotent.
#
# Signing: the committed app/build-profile.json5 is the sanitized no-signing
# form; the local DevEco auto-sign copy lives as an uncommitted working-tree
# modification. Without it this script still builds the HAP but the SignHap
# step produces an unsigned artifact (hdc install will then fail with a
# signature error — expected on a fresh checkout).
param(
    # Skip the device install (build-only run).
    [switch]$NoInstall,
    # Rust cross target (the default is the only ohos target this app ships).
    [string]$Target = "aarch64-unknown-linux-ohos"
)
$ErrorActionPreference = 'Stop'

# Resolve repo paths relative to this script (apps/pdfcraft-ohos/build-hap.ps1):
# $PSScriptRoot = apps/pdfcraft-ohos/, two levels up = the pdfcraft cargo
# workspace root.
$crateDir = $PSScriptRoot
$repoDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$appDir = Join-Path $crateDir 'app'
$libSrc = Join-Path $repoDir "target\$Target\release\libpdfcraft_ohos.so"
$libDst = Join-Path $appDir 'entry\libs\arm64-v8a\libpdfcraft_ohos.so'
$abilityHar = Join-Path $repoDir 'vendor\openharmony-ability\ability.har'

# aws-lc-sys (pdfcraft-sign chain) needs the Ninja cmake generator on the
# ohos face: cmake-rs defaults to a VS generator on Windows, whose TryCompile
# clags out on the ohos toolchain flags (P-2 finding, 2026-10-21). Pinned
# here so no machine-level CMAKE_GENERATOR can leak into the build.
$env:CMAKE_GENERATOR = 'Ninja'

# 0. Build input guard: the ability bridge .har is an untracked local build
#    input (same handling as the staged .so — never committed). Provenance:
#    byte-copy of egui-repo openharmony-ability/ability.har (ohdev @ 01440bb
#    working-tree state, built 2026-10-14 in ohos-winit p1), which is the
#    exact source state this repo's vendor/openharmony-ability holds.
if (-not (Test-Path $abilityHar)) {
    throw "ability.har not found: $abilityHar (copy it from the egui repo's openharmony-ability/ability.har — see P2_NOTES 7.x)"
}

# 1. Rust release cross-build. The stable msvc host toolchain carries the
#    ohos std targets on this machine (the gnu toolchains do not), so spell
#    it out.
Push-Location $repoDir
try {
    cargo +stable-x86_64-pc-windows-msvc build --release -p pdfcraft-ohos --target $Target
} finally {
    Pop-Location
}
if ($LASTEXITCODE -ne 0) { throw "cargo release build failed (exit $LASTEXITCODE)" }

# 2. Stage the .so for hvigor's automatic arm64-v8a packing. The libs dir is
#    gitignored (entry/.gitignore /libs), so a fresh checkout reaches this
#    point without it — create it before the copy.
if (-not (Test-Path $libSrc)) { throw "release .so not found: $libSrc" }
New-Item -ItemType Directory -Force (Split-Path -Parent $libDst) | Out-Null
Copy-Item $libSrc $libDst -Force
Write-Output "staged: $libDst"

# 3. ohpm dependency install (the step DevEco's project Sync silently did
#    for frame-spike; a fresh checkout has no oh_modules and hvigor's ArkTS
#    compile fails to resolve @ohos-rs/ability). Incremental — a no-op in
#    under a second when up to date. ohpm rides the DevEco tools install;
#    default to this machine's path when the env lacks an override.
#    DEVECO_SDK_HOME is required below anyway; default it to the DevEco
#    Studio install on this machine when the environment lacks it.
if (-not $env:DEVECO_SDK_HOME) {
    $env:DEVECO_SDK_HOME = 'C:\Program Files\Huawei\DevEco Studio\sdk'
}
$ohpm = Join-Path $env:DEVECO_SDK_HOME 'tools\ohpm\bin\ohpm.bat'
if (-not (Test-Path $ohpm)) { $ohpm = 'C:\Program Files\Huawei\DevEco Studio\tools\ohpm\bin\ohpm.bat' }
if (-not (Test-Path $ohpm)) { throw "ohpm.bat not found (set DEVECO_SDK_HOME or install DevEco Studio tools)" }
Push-Location $appDir
try {
    & $ohpm install --all
} finally {
    Pop-Location
}
if ($LASTEXITCODE -ne 0) { throw "ohpm install failed (exit $LASTEXITCODE)" }

# 4. hvigor release HAP.
Push-Location $appDir
try {
    hvigorw assembleHap --mode module -p product=default -p buildMode=release
} finally {
    Pop-Location
}
if ($LASTEXITCODE -ne 0) { throw "hvigor assembleHap failed (exit $LASTEXITCODE)" }

# Prefer the signed artifact; without a local signingConfig hvigor emits the
# unsigned one (see the header note — install then fails with a signature
# error, expected on a fresh checkout).
$hap = Join-Path $appDir 'entry\build\default\outputs\default\entry-default-signed.hap'
if (-not (Test-Path $hap)) {
    $hap = Join-Path $appDir 'entry\build\default\outputs\default\entry-default-unsigned.hap'
    if (Test-Path $hap) {
        Write-Warning "no signingConfig: using the UNSIGNED hap - hdc install will fail until DevEco auto-sign writes the local build-profile.json5 copy"
    }
}
if (-not (Test-Path $hap)) { throw "HAP not found: $hap" }
Write-Output "HAP: $hap"

# 5. Device install (replace in place).
if (-not $NoInstall) {
    hdc install -r $hap
    if ($LASTEXITCODE -ne 0) { throw "hdc install failed (exit $LASTEXITCODE)" }
    Write-Output "installed: com.egui.pdfcraft_spike"
}
