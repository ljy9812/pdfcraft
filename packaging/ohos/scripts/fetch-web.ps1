# fetch-web.ps1 — deploy the official pdfcraft web build into the HAP's rawfile.
# Downloads pdfcraft-web-<version>.zip from this repository's GitHub release
# (latest by default) and unpacks it into entry/src/main/resources/rawfile/web/.
# Usage: .\scripts\fetch-web.ps1 [-Tag v0.12.0]
param(
    [string]$Tag = ""
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot   # packaging/ohos
$repo = "storytold/pdfcraft"

if (-not $Tag) {
    $Tag = (gh release view --repo $repo --json tagName --jq .tagName)
    if ($LASTEXITCODE -ne 0) { throw "gh release view failed for $repo" }
}

$dl = Join-Path $root 'downloads'
New-Item -ItemType Directory -Force $dl | Out-Null
gh release download $Tag --repo $repo --pattern "pdfcraft-web-*.zip" --dir $dl --clobber
if ($LASTEXITCODE -ne 0) { throw "gh release download failed" }

$zip = Get-ChildItem $dl -Filter "pdfcraft-web-*.zip" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Write-Output ("zip: {0} ({1:N1} MB)" -f $zip.Name, ($zip.Length / 1MB))

$dest = Join-Path $dl $zip.BaseName
if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
Expand-Archive $zip.FullName -DestinationPath $dest
# release zips carry a same-named inner directory; locate the layer holding index.html
$inner = Get-ChildItem $dest -Directory | Where-Object { Test-Path (Join-Path $_.FullName 'index.html') } | Select-Object -First 1
$webDir = if ($inner) { $inner.FullName } else { $dest }
if (-not (Test-Path (Join-Path $webDir 'index.html'))) { throw "index.html not found under $dest - unexpected zip layout" }

# Deploy: wipe rawfile/web (keep the README placeholder), copy the web build in.
$rawWeb = Join-Path $root 'entry\src\main\resources\rawfile\web'
Get-ChildItem $rawWeb -Exclude 'README.md' | Remove-Item -Recurse -Force
Copy-Item (Join-Path $webDir '*') $rawWeb -Recurse -Force
Write-Output "deployed: $webDir -> $rawWeb"
