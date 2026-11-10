# Build the offline OCR plugin (command, text result only).
# Output: ../.temp/plugin-dist/ocr/  (ocr.exe + manifest.json + engine/*)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$repo = Split-Path (Split-Path $here -Parent) -Parent
$workspace = Split-Path $repo -Parent
$target = Join-Path $workspace ".temp\plugin-target\ocr"
$out = Join-Path $workspace ".temp\plugin-dist\ocr"
$manifest = Join-Path $here "manifest.json"

if (-not (Test-Path -LiteralPath $manifest)) {
    Write-Error "manifest.json not found: $manifest"
    exit 1
}

# Engine + models are never stored in the source tree. The helper verifies
# every shipped binary against its pinned SHA256 before copying it into dist.
& python (Join-Path $here "fetch_assets.py")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo test --release --manifest-path (Join-Path $here "Cargo.toml") --target-dir $target -- --test-threads=1
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo build --release --manifest-path (Join-Path $here "Cargo.toml") --target-dir $target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item -LiteralPath (Join-Path $target "release\rupix-plugin-ocr.exe") -Destination (Join-Path $out "ocr.exe") -Force
Copy-Item -LiteralPath $manifest -Destination (Join-Path $out "manifest.json") -Force
# The generator overwrites THIRD-PARTY-NOTICES.md in every ZIP with a crate
# table, so the shipped engine notice needs its own filename.
Copy-Item -LiteralPath (Join-Path $here "ENGINE-NOTICE.md") -Destination (Join-Path $out "ENGINE-NOTICE.md") -Force
Write-Host "built ocr -> $out"
exit 0
