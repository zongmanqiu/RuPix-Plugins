# RuPix plugin repository build entry point.
# Contract: each plugin may provide plugins/<id>/build.ps1 and write dist/<id>/.
# Root builds each plugin into ../.temp/plugin-dist/<id>, then packages a local
# catalogue at ../.build/docs/plugins.json with ZIPs alongside. Installed plugins
# are copied by the host into <exe>/plugins/.
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$workspace = Split-Path $root -Parent
$plugins = Join-Path $root "plugins"
$distRoot = Join-Path $workspace ".temp\plugin-dist"

if (-not (Test-Path $plugins)) {
    Write-Error "plugins/ not found: $plugins"
    exit 1
}

$check = Join-Path $root "tools\check_manifests.py"
& python $check
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

New-Item -ItemType Directory -Force -Path $distRoot | Out-Null

$built = 0
$skipped = 0
Get-ChildItem -LiteralPath $plugins -Directory | ForEach-Object {
    $id = $_.Name
    $build = Join-Path $_.FullName "build.ps1"
    if (Test-Path -LiteralPath $build) {
        Write-Host "== build $id =="
        & $build
        if ($LASTEXITCODE -ne 0) {
            Write-Error "plugin build failed: $id"
            exit $LASTEXITCODE
        }
    }

    $dist = Join-Path $distRoot $id
    if (-not (Test-Path -LiteralPath $dist)) {
        if (Test-Path -LiteralPath $build) {
            Write-Error "plugin build produced no output: $dist"
            exit 1
        }
        Write-Host "skip $id (no build.ps1 and no $dist)"
        $skipped++
        return
    }
    $manifest = Join-Path $dist "manifest.json"
    if (-not (Test-Path -LiteralPath $manifest)) {
        Write-Error "output without manifest: $dist"
        exit 1
    }

    $built++
}

& python (Join-Path $root "tools\make_catalog.py")
if ($LASTEXITCODE -ne 0) {
    Write-Error "plugin catalogue build failed"
    exit $LASTEXITCODE
}

Write-Host "plugin build done: $built built, $skipped skipped"
exit 0
