<#
.SYNOPSIS
  Authenticode-signs one Windows binary for the release workflow (master plan Phase 5.2).

.DESCRIPTION
  Called by `tauri build` for every binary it produces (the app, the uninstaller, the NSIS
  installer) through `bundle.windows.signCommand`, which the release workflow adds only when a
  code-signing provider is configured. Signing happens inside the build, before the updater
  signature (.sig) is computed, so both signatures cover the same final installer.

  Provider (repository variable MAINTAFOX_CODESIGN_PROVIDER, passed as an environment variable):
    sslcom-esigner   SSL.com eSigner cloud signing through CodeSignTool. Needs the environment
                     variables CODESIGNTOOL_DIR (folder of CodeSignTool, installed by the workflow),
                     ES_USERNAME, ES_PASSWORD, ES_CREDENTIAL_ID, ES_TOTP_SECRET (GitHub secrets).
  Any other value stops the build: an unsigned file is never presented as signed.

  After signing, the file's Authenticode signature must be valid, or the build stops.
#>
param(
  [Parameter(Mandatory = $true, Position = 0)][string]$FilePath
)

$ErrorActionPreference = "Stop"

function Fail([string]$Text) {
  Write-Host "::error::code signing: $Text"
  exit 1
}

if (-not (Test-Path -LiteralPath $FilePath)) { Fail "file not found: $FilePath" }
$provider = $env:MAINTAFOX_CODESIGN_PROVIDER

switch ($provider) {
  "sslcom-esigner" {
    foreach ($name in @("CODESIGNTOOL_DIR", "ES_USERNAME", "ES_PASSWORD", "ES_CREDENTIAL_ID", "ES_TOTP_SECRET")) {
      if (-not [Environment]::GetEnvironmentVariable($name)) { Fail "$name is not set (see docs/release/CODE_SIGNING.md)" }
    }
    $tool = Join-Path $env:CODESIGNTOOL_DIR "CodeSignTool.bat"
    if (-not (Test-Path -LiteralPath $tool)) { Fail "CodeSignTool.bat not found in $env:CODESIGNTOOL_DIR" }
    Push-Location $env:CODESIGNTOOL_DIR
    try {
      # No -output_dir_path: CodeSignTool replaces the file with its signed version, where tauri expects it.
      & $tool sign `
        "-username=$env:ES_USERNAME" `
        "-password=$env:ES_PASSWORD" `
        "-credential_id=$env:ES_CREDENTIAL_ID" `
        "-totp_secret=$env:ES_TOTP_SECRET" `
        "-input_file_path=$FilePath"
      if ($LASTEXITCODE -ne 0) { Fail "CodeSignTool failed (exit $LASTEXITCODE) for $FilePath" }
    }
    finally {
      Pop-Location
    }
  }
  default {
    Fail "unknown MAINTAFOX_CODESIGN_PROVIDER '$provider' (supported: sslcom-esigner)"
  }
}

$signature = Get-AuthenticodeSignature -LiteralPath $FilePath
if ($signature.Status -ne "Valid") {
  Fail "the signature of $FilePath is not valid after signing (status: $($signature.Status))"
}
Write-Host "Signed: $FilePath ($($signature.SignerCertificate.Subject))"
