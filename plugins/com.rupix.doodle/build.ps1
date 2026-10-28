# Build the doodle editor plugin.
# Output: <workspace>/.temp/plugin-dist/com.rupix.doodle/
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$repo = Split-Path (Split-Path $here -Parent) -Parent
$workspace = Split-Path $repo -Parent
$target = Join-Path $workspace ".temp\plugin-target\doodle"
$out = Join-Path $workspace ".temp\plugin-dist\com.rupix.doodle"
$manifest = Join-Path $here "manifest.json"

if (-not (Test-Path -LiteralPath $manifest)) {
    Write-Error "manifest.json not found: $manifest"
    exit 1
}

# Gate: clippy must be warning-free (release profile, same as the host repo).
& cargo clippy --release --manifest-path (Join-Path $here "Cargo.toml") --target-dir $target -- -D warnings
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
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
Copy-Item -LiteralPath (Join-Path $target "release\rupix-plugin-doodle.exe") -Destination (Join-Path $out "doodle.exe") -Force
Copy-Item -LiteralPath $manifest -Destination (Join-Path $out "manifest.json") -Force
Write-Host "built com.rupix.doodle -> $out"
exit 0
