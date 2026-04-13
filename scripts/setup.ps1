<#
.SYNOPSIS
  Maintafox Desktop -- one-command development environment setup for Windows.
  Run this script once after cloning the repository.
  It is safe to run multiple times.
.EXAMPLE
  .\scripts\setup.ps1
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step([string]$message) {
    Write-Host "`n[SETUP] $message" -ForegroundColor Cyan
}

function Write-Pass([string]$message) {
    Write-Host "  [PASS] $message" -ForegroundColor Green
}

function Write-Fail([string]$message) {
    Write-Host "  [FAIL] $message" -ForegroundColor Red
}

function Write-Info([string]$message) {
    Write-Host "  [INFO] $message" -ForegroundColor Yellow
}

$Failed = $false

# -- 1. Check Rust --------------------------------------------------------------
Write-Step "Checking Rust toolchain..."
try {
    $rustVersion = (rustup show active-toolchain 2>&1) -join " "
    Write-Pass "Rust found: $rustVersion"
} catch {
    Write-Fail "Rust not found."
    Write-Info "Install Rust from: https://rustup.rs"
    Write-Info "Run the installer, then restart PowerShell and re-run this script."
    $Failed = $true
}

# -- 2. Check Node.js -----------------------------------------------------------
Write-Step "Checking Node.js..."
try {
    $nodeVersion = node --version 2>&1
    $nodeMajor = [int]($nodeVersion -replace "v(\d+).*", '$1')
    if ($nodeMajor -ge 20) {
        Write-Pass "Node.js found: $nodeVersion"
    } else {
        Write-Fail "Node.js $nodeVersion is too old. Version 20 or higher is required."
        Write-Info "Download Node.js 20 LTS from: https://nodejs.org"
        $Failed = $true
    }
} catch {
    Write-Fail "Node.js not found."
    Write-Info "Download Node.js 20 LTS from: https://nodejs.org"
    $Failed = $true
}

# -- 3. Check or install pnpm ---------------------------------------------------
Write-Step "Checking pnpm..."
try {
    $pnpmVersion = pnpm --version 2>&1
    $pnpmMajor = [int]($pnpmVersion -replace "(\d+).*", '$1')
    if ($pnpmMajor -ge 9) {
        Write-Pass "pnpm found: $pnpmVersion"
    } else {
        Write-Info "pnpm $pnpmVersion is too old -- upgrading to latest..."
        npm install -g pnpm@latest
        Write-Pass "pnpm upgraded."
    }
} catch {
    Write-Info "pnpm not found -- installing..."
    npm install -g pnpm@latest
    Write-Pass "pnpm installed."
}

# -- 4. Check WebView2 Runtime (required by Tauri on Windows) -----------------
Write-Step "Checking Microsoft Edge WebView2 Runtime..."
$webview2Key = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
$webview2Key2 = "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
if ((Test-Path $webview2Key) -or (Test-Path $webview2Key2)) {
    Write-Pass "WebView2 Runtime found."
} else {
    Write-Fail "WebView2 Runtime not found."
    Write-Info "Download and install from:"
    Write-Info "  https://developer.microsoft.com/en-us/microsoft-edge/webview2/"
    Write-Info "Choose 'Evergreen Bootstrapper' and install it, then re-run this script."
    $Failed = $true
}

# -- 5. Check MSVC Build Tools -------------------------------------------------
Write-Step "Checking Visual C++ Build Tools..."
$clPaths = @(
    "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC",
    "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC",
    "C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\MSVC",
    "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\MSVC",
    "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Tools\MSVC",
    "C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\VC\Tools\MSVC"
)
$msvcFound = $false
foreach ($path in $clPaths) {
    if (Test-Path $path) { $msvcFound = $true; break }
}
if (-not $msvcFound) {
    # Try vswhere as fallback
    $vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vsWhere) {
        $vsPath = & $vsWhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
        if ($vsPath) { $msvcFound = $true }
    }
}
if (-not $msvcFound) {
    # Last resort: verify rustc can link (proves MSVC linker is usable)
    try {
        $testFile = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.rs'
        'fn main() {}' | Set-Content $testFile -Encoding ascii
        $testExe = $testFile -replace '\.rs$', '.exe'
        rustc $testFile -o $testExe 2>&1 | Out-Null
        if (Test-Path $testExe) {
            $msvcFound = $true
            Remove-Item $testFile, $testExe -ErrorAction SilentlyContinue
        } else {
            Remove-Item $testFile -ErrorAction SilentlyContinue
        }
    } catch { }
}
if ($msvcFound) {
    Write-Pass "Visual C++ Build Tools found."
} else {
    try {
        $clCheck = Get-Command cl.exe -ErrorAction Stop
        Write-Pass "cl.exe found in PATH: $($clCheck.Source)"
    } catch {
        Write-Fail "Visual C++ Build Tools not found."
        Write-Info "Install Visual Studio Build Tools from:"
        Write-Info "  https://visualstudio.microsoft.com/visual-cpp-build-tools/"
        Write-Info "Select: 'Desktop development with C++'"
        $Failed = $true
    }
}

# -- Abort if prerequisites are missing ----------------------------------------
if ($Failed) {
    Write-Host ""
    Write-Host "One or more prerequisites are missing." -ForegroundColor Red
    Write-Host "Install the items marked [FAIL] above, then re-run this script." -ForegroundColor Red
    exit 1
}

# -- 6. Install Node.js dependencies -------------------------------------------
Write-Step "Installing Node.js dependencies (pnpm install)..."
pnpm install
Write-Pass "Node.js dependencies installed."

# -- 7. Pre-fetch Rust dependencies --------------------------------------------
Write-Step "Pre-fetching Rust dependencies (cargo fetch)..."
Push-Location src-tauri
cargo fetch
Pop-Location
Write-Pass "Rust dependencies fetched."

# -- 8. Create .env.local if it does not exist ---------------------------------
Write-Step "Checking environment configuration..."
if (-not (Test-Path ".env.local")) {
    Copy-Item ".env.example" ".env.local"
    Write-Pass ".env.local created from .env.example."
    Write-Info "Review .env.local and adjust MAINTAFOX_ENV if needed."
} else {
    Write-Pass ".env.local already exists -- not overwriting."
}

# -- 9. Run environment preflight checker --------------------------------------
Write-Step "Running environment preflight check..."
pnpm tsx scripts/check-env.ts

Write-Host ""
Write-Host "========================================" -ForegroundColor Green
Write-Host "  Maintafox dev environment is ready.  " -ForegroundColor Green
Write-Host "  Run: pnpm run dev                    " -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Green
