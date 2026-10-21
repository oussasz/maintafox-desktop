<#
.SYNOPSIS
  One-time, guided creation of the Maintafox updater signing key (master plan Phase 5.1).

.DESCRIPTION
  Run it yourself, on your own computer, from the repository folder:

      powershell -ExecutionPolicy Bypass -File scripts\release\setup-updater-key.ps1

  What it does:
    1. Checks the tools it needs (pnpm, GitHub CLI signed in to this repository).
    2. Runs `tauri signer generate`, which asks YOU for a password (typed twice, never shown).
       The private key file is written outside the repository (default: %USERPROFILE%\.maintafox\updater\).
    3. Copies the PUBLIC key into src-tauri\updater\maintafox-updater.pub (committed: the app embeds it).
    4. Shows the two `gh secret set` commands that you type to store the private key and its
       password as GitHub secrets for the release workflow, then checks that both secrets exist.

  Nothing secret is printed, logged or sent anywhere by this script. It never overwrites an
  existing key: every installed Maintafox only accepts updates signed with the key it was built
  with, so a new key means every customer reinstalls by hand once.
#>
[CmdletBinding()]
param(
  # Folder for the private key file (outside the repository).
  [string]$KeyDirectory = (Join-Path $env:USERPROFILE ".maintafox\updater")
)

$ErrorActionPreference = "Stop"
$KeyFileName = "maintafox-updater.key"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$PublicKeyTarget = Join-Path $RepoRoot "src-tauri\updater\maintafox-updater.pub"
$SecretKeyName = "TAURI_SIGNING_PRIVATE_KEY"
$SecretPasswordName = "TAURI_SIGNING_PRIVATE_KEY_PASSWORD"

function Step([string]$Text) { Write-Host ""; Write-Host "==> $Text" -ForegroundColor Cyan }
function Stop-WithMessage([string]$Text) { Write-Host ""; Write-Host "STOPPED: $Text" -ForegroundColor Red; exit 1 }

Step "Checking the tools"
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) { Stop-WithMessage "pnpm is not installed (https://pnpm.io/installation)." }
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { Stop-WithMessage "The GitHub CLI 'gh' is not installed (https://cli.github.com)." }
& gh auth status *> $null
if ($LASTEXITCODE -ne 0) { Stop-WithMessage "The GitHub CLI is not signed in. Run 'gh auth login' first, then run this script again." }
Push-Location $RepoRoot
try {
  $repo = (& gh repo view --json nameWithOwner --jq .nameWithOwner).Trim()
  if (-not $repo) { Stop-WithMessage "This folder is not a GitHub repository known to gh." }
  Write-Host "Repository: $repo"
  if (-not (Test-Path (Join-Path $RepoRoot "node_modules\.bin\tauri.cmd"))) {
    Stop-WithMessage "The project dependencies are not installed. Run 'pnpm install' in $RepoRoot first."
  }

  $KeyPath = Join-Path $KeyDirectory $KeyFileName
  Step "Checking that no key exists yet"
  if (Test-Path $PublicKeyTarget) {
    Stop-WithMessage "src-tauri\updater\maintafox-updater.pub already exists: the key was created before. Do not create another one (see src-tauri\updater\README.md)."
  }
  if (Test-Path $KeyPath) {
    Stop-WithMessage "A private key already exists at $KeyPath. Use it (copy its .pub file to $PublicKeyTarget) instead of creating a new one."
  }
  New-Item -ItemType Directory -Force -Path $KeyDirectory | Out-Null

  Step "Creating the key pair"
  Write-Host "Choose a strong password (you will type it twice; it is not shown)."
  Write-Host "Keep it in your password manager: without it the key cannot sign updates." -ForegroundColor Yellow
  & pnpm exec tauri signer generate -w $KeyPath
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path $KeyPath) -or -not (Test-Path "$KeyPath.pub")) {
    Stop-WithMessage "tauri signer generate did not create the key. Nothing was changed in the repository."
  }

  Step "Saving the public key in the repository"
  $publicKey = (Get-Content "$KeyPath.pub" -Raw).Trim()
  [System.IO.File]::WriteAllText($PublicKeyTarget, "$publicKey`n")
  Write-Host "Written: src-tauri\updater\maintafox-updater.pub (commit this file)."

  Step "Storing the private key and its password as GitHub secrets"
  Write-Host "Type these two commands yourself in this PowerShell window:" -ForegroundColor Yellow
  Write-Host ""
  Write-Host "  Get-Content `"$KeyPath`" -Raw | gh secret set $SecretKeyName --repo $repo"
  Write-Host "  gh secret set $SecretPasswordName --repo $repo"
  Write-Host ""
  Write-Host "The second command asks for the password: paste or type the password you chose above."
  Read-Host "Press Enter when both commands have been run"

  $names = & gh secret list --repo $repo --json name --jq ".[].name"
  $missing = @($SecretKeyName, $SecretPasswordName) | Where-Object { $names -notcontains $_ }
  if ($missing) { Stop-WithMessage "These secrets are not set yet: $($missing -join ', '). Run the commands above, then run 'gh secret list'." }
  Write-Host "Both secrets are set." -ForegroundColor Green

  Step "Done. Next steps"
  Write-Host "1. Back up the private key file and its password OFFLINE (password manager + a USB key kept safe):"
  Write-Host "     $KeyPath"
  Write-Host "   If both the file and the secret are lost, no installed Maintafox can be updated any more."
  Write-Host "2. Ask Claude (or a developer) to commit src-tauri\updater\maintafox-updater.pub."
  Write-Host "3. The server needs the same PUBLIC key: RELEASE_UPDATER_PUBLIC_KEY in its .env (deploy/RELEASES.md"
  Write-Host "   in the vendor console repository). It is public; this is its value:"
  Write-Host ""
  Write-Host $publicKey
}
finally {
  Pop-Location
}
