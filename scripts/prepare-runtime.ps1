param(
    [string]$Version = "latest",
    [string]$PluginRepo = "SpookySandwich/dsh-plugin-smooth-stream",
    [string]$PluginRef = "main",
    [string]$OutDir = (Join-Path $PSScriptRoot "..\shell\src-tauri\resources\runtime")
)

# Prepare the sidecar runtime: install @deepseek-ai/dsh, then copy the node
# binary and the production node_modules closure into the Tauri resources tree.
# Also fetches the dsh-smooth-stream plugin payload (offered as an optional
# pre-install component in the NSIS installer; the shell merges it into the
# user's web profile at first run when the marker is present).
$ErrorActionPreference = "Stop"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$stage = Join-Path $root ".staging\runtime"
$outDir = [System.IO.Path]::GetFullPath((Join-Path $root "shell\src-tauri\resources\runtime"))

if (-not $Version -or $Version -eq "latest") {
    $spec = "@deepseek-ai/dsh@latest"
} else {
    $spec = "@deepseek-ai/dsh@$Version"
}

Write-Host "==> Installing $spec into staging..."
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
Set-Content -Path (Join-Path $stage "package.json") -Value '{"name":"dsh-runtime","private":true}' -Encoding utf8
Push-Location $stage
npm install $spec --omit=dev --no-audit --no-fund
Pop-Location

Write-Host "==> Copying node.exe and node_modules to $outDir"
New-Item -ItemType Directory -Force $outDir | Out-Null
$node = (Get-Command node).Source
Copy-Item $node (Join-Path $outDir "node.exe") -Force
if (Test-Path (Join-Path $outDir "node_modules")) { Remove-Item (Join-Path $outDir "node_modules") -Recurse -Force }
Copy-Item (Join-Path $stage "node_modules") (Join-Path $outDir "node_modules") -Recurse -Force

# Copy the notification bridge (subscribes to dsh turn/end events)
Copy-Item (Join-Path $PSScriptRoot "bridge.mjs") (Join-Path $outDir "bridge.mjs") -Force

# Record the resolved dsh version (CI stamps it into the installer version)
$dshVersion = (Get-Content (Join-Path $outDir "node_modules\@deepseek-ai\dsh\package.json") -Raw | ConvertFrom-Json).version
Set-Content -Path (Join-Path $outDir "dsh-version.txt") -Value $dshVersion -Encoding utf8

# Fetch the dsh-smooth-stream plugin payload (GitHub zip; Expand-Archive is
# used instead of tar so the script works even when a Unix tar shadows the
# Windows one on PATH).
Write-Host "==> Fetching plugin payload $PluginRepo@$PluginRef"
$pluginStage = Join-Path $root ".staging\plugin"
if (Test-Path $pluginStage) { Remove-Item $pluginStage -Recurse -Force }
New-Item -ItemType Directory -Force $pluginStage | Out-Null
$zip = Join-Path $pluginStage "plugin.zip"
Invoke-WebRequest -Uri "https://codeload.github.com/$PluginRepo/zip/refs/heads/$PluginRef" -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $pluginStage
# Take the archive's own root directory rather than assuming "<repo>-<ref>":
# GitHub redirects a renamed repository's codeload URL but names the root after
# the *current* repository, so a hardcoded name silently breaks on a rename.
$pluginSrc = (Get-ChildItem -Path $pluginStage -Directory | Select-Object -First 1).FullName
if (-not $pluginSrc) { throw "plugin archive from $PluginRepo@$PluginRef contained no directory" }
# The payload directory name must match the package name the shell installs
# into the profile (see PLUGIN_PACKAGE in shell/src-tauri/src/lib.rs).
$pluginName = (Get-Content (Join-Path $pluginSrc "package.json") -Raw | ConvertFrom-Json).name
$pluginOut = Join-Path $outDir "plugins\$pluginName"
if (Test-Path $pluginOut) { Remove-Item $pluginOut -Recurse -Force }
New-Item -ItemType Directory -Force $pluginOut | Out-Null
foreach ($item in @("package.json", "cordis.patch.yml", "plugin.client.js", "lib", "LICENSE")) {
    Copy-Item (Join-Path $pluginSrc $item) (Join-Path $pluginOut $item) -Recurse -Force
}

Write-Host "==> Runtime prepared:"
Write-Host "    node.exe    -> $(Join-Path $outDir 'node.exe')"
Write-Host "    bin.js      -> $(Join-Path $outDir 'node_modules\@deepseek-ai\dsh\lib\bin.js')"
Write-Host "    dsh version -> $dshVersion"
Write-Host "    plugin      -> $pluginOut ($pluginName)"
