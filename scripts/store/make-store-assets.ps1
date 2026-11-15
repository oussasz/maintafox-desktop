# Makes the Microsoft Store package images and the Store listing logos from the app logo
# (src-tauri/icons/app-icon.png, 2160 x 2160, transparent). Run it again only when the logo changes;
# the images are committed so the Store build does not depend on an image tool.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/store/make-store-assets.ps1
#
# Package images (src-tauri/windows/msix/Assets): every scale Windows asks for (100-400 %), and the
# taskbar / Start icon sizes (targetsize, also "unplated" so Windows 11 shows them without a plate).
# Listing logos (docs/store/listing): 1:1 logos for Partner Center (300, 1080 and 2160 px).
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$sourcePath = Join-Path $root "src-tauri\icons\app-icon.png"
$assets = Join-Path $root "src-tauri\windows\msix\Assets"
$listing = Join-Path $root "docs\store\listing"
New-Item -ItemType Directory -Force -Path $assets, $listing | Out-Null

$source = [System.Drawing.Image]::FromFile($sourcePath)
if ($source.Width -ne $source.Height -or $source.Width -lt 1024) {
  throw "app-icon.png must be square and at least 1024 px (it is $($source.Width) x $($source.Height))."
}

# Draws the logo centred on a transparent width x height canvas, the logo filling `fill` of the
# shorter side (tiles keep a margin; small icons use almost all of it).
function Save-Logo([string]$path, [int]$width, [int]$height, [double]$fill) {
  $bitmap = New-Object System.Drawing.Bitmap($width, $height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $side = [int][Math]::Round([Math]::Min($width, $height) * $fill)
    $x = [int][Math]::Floor(($width - $side) / 2)
    $y = [int][Math]::Floor(($height - $side) / 2)
    $g.DrawImage($source, (New-Object System.Drawing.Rectangle($x, $y, $side, $side)))
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  } finally {
    $g.Dispose()
    $bitmap.Dispose()
  }
}

$scales = 100, 125, 150, 200, 400
# Name, base width, base height, logo fill.
$images = @(
  @("Square44x44Logo", 44, 44, 1.0),
  @("StoreLogo", 50, 50, 1.0),
  @("SmallTile", 71, 71, 0.8),
  @("Square150x150Logo", 150, 150, 0.66),
  @("Wide310x150Logo", 310, 150, 0.66),
  @("LargeTile", 310, 310, 0.6)
)
foreach ($image in $images) {
  foreach ($scale in $scales) {
    $w = [int][Math]::Round($image[1] * $scale / 100)
    $h = [int][Math]::Round($image[2] * $scale / 100)
    Save-Logo (Join-Path $assets "$($image[0]).scale-$scale.png") $w $h $image[3]
  }
}
foreach ($size in 16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256) {
  Save-Logo (Join-Path $assets "Square44x44Logo.targetsize-$size.png") $size $size 1.0
  Save-Logo (Join-Path $assets "Square44x44Logo.targetsize-${size}_altform-unplated.png") $size $size 1.0
  Save-Logo (Join-Path $assets "Square44x44Logo.targetsize-${size}_altform-lightunplated.png") $size $size 1.0
}
foreach ($size in 300, 1080, 2160) {
  Save-Logo (Join-Path $listing "maintafox-store-logo-$size.png") $size $size 0.9
}
$source.Dispose()
Write-Host "Store images written to $assets and $listing."
