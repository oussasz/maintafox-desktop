# Maintafox Desktop — Windows setup script
# Run from the project root: .\scripts\setup.ps1

Write-Host "Checking environment..." -ForegroundColor Cyan

# Check Node.js
$nodeVersion = node --version 2>$null
if (-not $nodeVersion) {
    Write-Error "Node.js is not installed. Please install Node.js 20 LTS."
    exit 1
}
Write-Host "Node.js: $nodeVersion" -ForegroundColor Green

# Check pnpm
$pnpmVersion = pnpm --version 2>$null
if (-not $pnpmVersion) {
    Write-Error "pnpm is not installed. Run: npm install -g pnpm"
    exit 1
}
Write-Host "pnpm: $pnpmVersion" -ForegroundColor Green

# Check Rust
$cargoVersion = cargo --version 2>$null
if (-not $cargoVersion) {
    Write-Error "Rust/Cargo is not installed. Run: rustup-init"
    exit 1
}
Write-Host "Cargo: $cargoVersion" -ForegroundColor Green

Write-Host "`nInstalling Node.js dependencies..." -ForegroundColor Cyan
pnpm install

Write-Host "`nSetup complete. Run 'pnpm tauri dev' to start." -ForegroundColor Green
