# Build the barcode scan plugin (report only, no window).
# Output: ../.temp/plugin-dist/scan/
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$repo = Split-Path (Split-Path $here -Parent) -Parent
$workspace = Split-Path $repo -Parent
$target = Join-Path $workspace ".temp\plugin-target\scan"
$out = Join-Path $workspace ".temp\plugin-dist\scan"
$manifest = Join-Path $here "manifest.json"

if (-not (Test-Path -LiteralPath $manifest)) {
    Write-Error "manifest.json not found: $manifest"
    exit 1
}

& cargo test --release --manifest-path (Join-Path $here "Cargo.toml") --target-dir $target -- --test-threads=1
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

& cargo build --release --manifest-path (Join-Path $here "Cargo.toml") --target-dir $target
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item -LiteralPath (Join-Path $target "release\rupix-plugin-scan.exe") -Destination (Join-Path $out "scan.exe") -Force
Copy-Item -LiteralPath $manifest -Destination (Join-Path $out "manifest.json") -Force
Write-Host "built scan -> $out"
exit 0
