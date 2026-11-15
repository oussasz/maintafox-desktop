# Packs a built Maintafox (Store flavour: MAINTAFOX_DISTRIBUTION=store, no updater) into an unsigned
# MSIX and the .msixupload file Partner Center takes. The Microsoft Store signs the package itself
# (owner decision 2026-10-24: no code-signing certificate). Used by .github/workflows/store.yml.
#
#   scripts/store/build-msix.ps1 -Executable src-tauri/target/release/maintafox.exe `
#     -IdentityName <Package/Identity/Name> -Publisher "CN=..." -PublisherDisplayName <name> `
#     -Version 0.9.3 -OutDir dist-store
#
# The three identity values come from Partner Center (docs/store/MICROSOFT_STORE.md); there is no
# default: a package with made-up values is refused by the Store.
param(
  [Parameter(Mandatory)] [string]$Executable,
  [Parameter(Mandatory)] [string]$IdentityName,
  [Parameter(Mandatory)] [string]$Publisher,
  [Parameter(Mandatory)] [string]$PublisherDisplayName,
  [Parameter(Mandatory)] [string]$Version,
  [Parameter(Mandatory)] [string]$OutDir
)
$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$msixDir = Join-Path $root "src-tauri\windows\msix"

# ── Inputs ──────────────────────────────────────────────────────────────────────────────────
if (-not (Test-Path -LiteralPath $Executable)) { throw "The app binary $Executable does not exist (build it first)." }
# Package identity name: letters, digits, dots and dashes, 3 to 50 characters (appx schema).
if ($IdentityName -cnotmatch '^[A-Za-z0-9.\-]{3,50}$') { throw "IdentityName '$IdentityName' is not a package identity name (Partner Center > Product identity > Package/Identity/Name)." }
if ($Publisher -cnotmatch '^CN=.+') { throw "Publisher must be the 'CN=...' value of Partner Center > Product identity > Package/Identity/Publisher." }
if (-not $PublisherDisplayName.Trim()) { throw "PublisherDisplayName is empty (Partner Center > Product identity > Package/Properties/PublisherDisplayName)." }
# The Store takes MAJOR.MINOR.PATCH.0 only (the fourth part is reserved for the Store).
if ($Version -cnotmatch '^(\d+)\.(\d+)\.(\d+)$') { throw "Version '$Version' is not MAJOR.MINOR.PATCH." }
foreach ($part in 1..3) {
  if ([int]$Matches[$part] -gt 65535) { throw "Version part $($Matches[$part]) is above 65535 (MSIX limit)." }
}
$packageVersion = "$Version.0"

# ── Windows SDK tools (installed on GitHub's windows-latest runners) ────────────────────────
function Find-SdkTool([string]$name) {
  $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
  $tool = Get-ChildItem -Path $kits -Filter $name -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.DirectoryName -match '\\10\.[\d.]+\\x64$' } |
    Sort-Object { [version](Split-Path (Split-Path $_.DirectoryName -Parent) -Leaf) } -Descending |
    Select-Object -First 1
  if (-not $tool) { throw "$name was not found in the Windows 10/11 SDK ($kits)." }
  return $tool.FullName
}
$makeappx = Find-SdkTool "makeappx.exe"
$makepri = Find-SdkTool "makepri.exe"
Write-Host "makeappx: $makeappx"
Write-Host "makepri:  $makepri"

# ── Package layout ──────────────────────────────────────────────────────────────────────────
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("maintafox-msix-" + [guid]::NewGuid().ToString("N"))
$layout = Join-Path $work "layout"
New-Item -ItemType Directory -Force -Path $layout | Out-Null
$exeName = "Maintafox.exe"
Copy-Item -LiteralPath $Executable -Destination (Join-Path $layout $exeName)
Copy-Item -Recurse -Path (Join-Path $msixDir "Assets") -Destination (Join-Path $layout "Assets")

function Escape-Xml([string]$value) { return [System.Security.SecurityElement]::Escape($value) }
$manifest = Get-Content -Raw -Encoding UTF8 (Join-Path $msixDir "AppxManifest.template.xml")
$values = @{
  "IDENTITY_NAME" = $IdentityName
  "PUBLISHER" = $Publisher
  "PUBLISHER_DISPLAY_NAME" = $PublisherDisplayName
  "VERSION" = $packageVersion
  "EXECUTABLE" = $exeName
}
foreach ($key in $values.Keys) { $manifest = $manifest.Replace("{{$key}}", (Escape-Xml $values[$key])) }
if ($manifest -match '\{\{[A-Z_]+\}\}') { throw "The manifest still has an unfilled value: $($Matches[0])." }
[xml]$manifest | Out-Null  # well-formed XML, or stop here
[System.IO.File]::WriteAllText((Join-Path $layout "AppxManifest.xml"), $manifest, (New-Object System.Text.UTF8Encoding($false)))

# Resource index: lets Windows pick the image for each display scale and taskbar size.
$priConfig = Join-Path $work "priconfig.xml"
& $makepri createconfig /cf $priConfig /dq fr-FR_en-US /pv 10.0.0 /o
if ($LASTEXITCODE -ne 0) { throw "makepri createconfig failed." }
# One package, not a bundle: keep every image in the one resources.pri (the default config splits
# them into per-scale resource packs, which only a bundle installs).
[xml]$config = Get-Content -Raw $priConfig
foreach ($node in @($config.SelectNodes("//packaging"))) { [void]$node.ParentNode.RemoveChild($node) }
$config.Save($priConfig)
& $makepri new /pr $layout /cf $priConfig /mn (Join-Path $layout "AppxManifest.xml") /of (Join-Path $layout "resources.pri") /o
if ($LASTEXITCODE -ne 0) { throw "makepri new failed." }

# ── Pack ────────────────────────────────────────────────────────────────────────────────────
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$baseName = "Maintafox_${Version}_x64"
$msix = Join-Path (Resolve-Path $OutDir) "$baseName.msix"
& $makeappx pack /d $layout /p $msix /o
if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed (the manifest or the layout was refused)." }

# .msixupload: a zip holding the .msix (what Partner Center's Packages page takes).
$zip = Join-Path $work "$baseName.zip"
Compress-Archive -LiteralPath $msix -DestinationPath $zip -Force
$upload = Join-Path (Resolve-Path $OutDir) "$baseName.msixupload"
Move-Item -LiteralPath $zip -Destination $upload -Force
Remove-Item -Recurse -Force $work

foreach ($file in $msix, $upload) {
  $sha = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
  Write-Host "$(Split-Path $file -Leaf)  $sha"
}
