# Phase 1 · Sub-phase 01 · File 03
# Dev Environment, CI, and Tooling Baseline

## Context and Purpose

This file ensures that any developer or AI coding agent can reproduce a fully working local
development environment from a clean Windows machine in a single script execution. It also
establishes the environment variable model, secrets management governance, and local
database tooling that every subsequent sprint depends on.

Maintafox runs primarily on **Windows** — the maintenance engineer supervisor and the
majority of end users are Windows-based. Setup scripts are Windows-first, with Unix/macOS
equivalents for CI and cross-platform agents.

## Prerequisites

- File 01 (scaffold and coding standards) completed
- File 02 (branching and CI pipeline) completed
- Git, Rust stable, and Node.js 20 available on the developer machine

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | One-Command Dev Setup | Windows and Unix setup scripts, environment preflight checker |
| S2 | Secrets and Environment Baseline | `.env.example`, secrets policy, CI secrets guide, gitleaks integration |
| S3 | Local DB Tooling and Developer Utilities | DB init/seed/reset scripts, migration runner, DEV_ENVIRONMENT.md |

---

## Sprint S1 — One-Command Dev Setup Scripts

### AI Agent Prompt

You are building the developer environment onboarding system for the Maintafox Desktop
project. A person who has never worked on this codebase — including an AI coding agent
starting in a fresh container — should be able to run one script and arrive at a working
local development environment with all prerequisites verified.

---

**Step 1 — Write `scripts/setup.ps1` (Windows PowerShell).**

This script is the primary onboarding tool for Windows developers and the supervisor.
It must be fully idempotent — running it twice should produce no errors.

```powershell
<#
.SYNOPSIS
  Maintafox Desktop — one-command development environment setup for Windows.
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

# ── 1. Check Rust ──────────────────────────────────────────────────────────────
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

# ── 2. Check Node.js ───────────────────────────────────────────────────────────
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

# ── 3. Check or install pnpm ───────────────────────────────────────────────────
Write-Step "Checking pnpm..."
try {
    $pnpmVersion = pnpm --version 2>&1
    $pnpmMajor = [int]($pnpmVersion -replace "(\d+).*", '$1')
    if ($pnpmMajor -ge 9) {
        Write-Pass "pnpm found: $pnpmVersion"
    } else {
        Write-Info "pnpm $pnpmVersion is too old — upgrading to latest..."
        npm install -g pnpm@latest
        Write-Pass "pnpm upgraded."
    }
} catch {
    Write-Info "pnpm not found — installing..."
    npm install -g pnpm@latest
    Write-Pass "pnpm installed."
}

# ── 4. Check WebView2 Runtime (required by Tauri on Windows) ─────────────────
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

# ── 5. Check MSVC Build Tools ─────────────────────────────────────────────────
Write-Step "Checking Visual C++ Build Tools..."
$clPaths = @(
    "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC",
    "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Tools\MSVC"
)
$msvcFound = $false
foreach ($path in $clPaths) {
    if (Test-Path $path) { $msvcFound = $true; break }
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

# ── Abort if prerequisites are missing ────────────────────────────────────────
if ($Failed) {
    Write-Host ""
    Write-Host "One or more prerequisites are missing." -ForegroundColor Red
    Write-Host "Install the items marked [FAIL] above, then re-run this script." -ForegroundColor Red
    exit 1
}

# ── 6. Install Node.js dependencies ───────────────────────────────────────────
Write-Step "Installing Node.js dependencies (pnpm install)..."
pnpm install
Write-Pass "Node.js dependencies installed."

# ── 7. Pre-fetch Rust dependencies ────────────────────────────────────────────
Write-Step "Pre-fetching Rust dependencies (cargo fetch)..."
Push-Location src-tauri
cargo fetch
Pop-Location
Write-Pass "Rust dependencies fetched."

# ── 8. Create .env.local if it does not exist ─────────────────────────────────
Write-Step "Checking environment configuration..."
if (-not (Test-Path ".env.local")) {
    Copy-Item ".env.example" ".env.local"
    Write-Pass ".env.local created from .env.example."
    Write-Info "Review .env.local and adjust MAINTAFOX_ENV if needed."
} else {
    Write-Pass ".env.local already exists — not overwriting."
}

# ── 9. Run environment preflight checker ──────────────────────────────────────
Write-Step "Running environment preflight check..."
pnpm tsx scripts/check-env.ts

Write-Host ""
Write-Host "========================================" -ForegroundColor Green
Write-Host "  Maintafox dev environment is ready.  " -ForegroundColor Green
Write-Host "  Run: pnpm run dev                    " -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Green
```

---

**Step 2 — Write `scripts/setup.sh` (Unix/macOS).**

```bash
#!/usr/bin/env bash
# Maintafox Desktop — one-command development environment setup for macOS/Linux.
# Safe to run multiple times.
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

step()  { echo -e "\n${CYAN}[SETUP] $1${NC}"; }
pass()  { echo -e "  ${GREEN}[PASS] $1${NC}"; }
fail()  { echo -e "  ${RED}[FAIL] $1${NC}"; FAILED=true; }
info()  { echo -e "  ${YELLOW}[INFO] $1${NC}"; }

FAILED=false

# 1. Check Rust
step "Checking Rust toolchain..."
if command -v rustup &>/dev/null; then
    RUST_VER=$(rustup show active-toolchain 2>/dev/null || echo "unknown")
    pass "Rust found: $RUST_VER"
else
    fail "Rust not found."
    info "Install from: https://rustup.rs"
    info "Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

# 2. Check Node.js >= 20
step "Checking Node.js..."
if command -v node &>/dev/null; then
    NODE_VER=$(node --version)
    NODE_MAJOR=$(echo "$NODE_VER" | sed 's/v\([0-9]*\).*/\1/')
    if [ "$NODE_MAJOR" -ge 20 ]; then
        pass "Node.js found: $NODE_VER"
    else
        fail "Node.js $NODE_VER is too old. Version 20+ required."
        info "Download from: https://nodejs.org"
    fi
else
    fail "Node.js not found."
    info "Download Node.js 20 LTS from: https://nodejs.org"
fi

# 3. Check or install pnpm
step "Checking pnpm..."
if command -v pnpm &>/dev/null; then
    PNPM_VER=$(pnpm --version)
    PNPM_MAJOR=$(echo "$PNPM_VER" | sed 's/\([0-9]*\).*/\1/')
    if [ "$PNPM_MAJOR" -ge 9 ]; then
        pass "pnpm found: $PNPM_VER"
    else
        info "pnpm $PNPM_VER too old — upgrading..."
        npm install -g pnpm@latest
        pass "pnpm upgraded."
    fi
else
    info "pnpm not found — installing..."
    npm install -g pnpm@latest
    pass "pnpm installed."
fi

# 4. Check system dependencies for Tauri on Linux/macOS
step "Checking Tauri system dependencies..."
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    MISSING_PKGS=()
    for pkg in libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev; do
        if ! dpkg -l "$pkg" &>/dev/null; then
            MISSING_PKGS+=("$pkg")
        fi
    done
    if [ ${#MISSING_PKGS[@]} -eq 0 ]; then
        pass "Linux Tauri dependencies found."
    else
        fail "Missing Linux packages: ${MISSING_PKGS[*]}"
        info "Install with: sudo apt-get install -y ${MISSING_PKGS[*]}"
    fi
elif [[ "$OSTYPE" == "darwin"* ]]; then
    if xcode-select -p &>/dev/null; then
        pass "Xcode Command Line Tools found."
    else
        fail "Xcode Command Line Tools not found."
        info "Run: xcode-select --install"
    fi
fi

[ "$FAILED" = true ] && { echo -e "\n${RED}Prerequisites missing. Fix items above and re-run.${NC}"; exit 1; }

# 5. Install Node dependencies
step "Installing Node.js dependencies..."
pnpm install
pass "Node dependencies installed."

# 6. Pre-fetch Rust dependencies
step "Pre-fetching Rust dependencies..."
(cd src-tauri && cargo fetch)
pass "Rust dependencies fetched."

# 7. Create .env.local if absent
step "Checking environment config..."
if [ ! -f ".env.local" ]; then
    cp .env.example .env.local
    pass ".env.local created from .env.example."
else
    pass ".env.local already exists — not overwriting."
fi

# 8. Preflight check
step "Running environment preflight..."
pnpm tsx scripts/check-env.ts

echo -e "\n${GREEN}========================================${NC}"
echo -e "${GREEN}  Maintafox dev environment is ready.  ${NC}"
echo -e "${GREEN}  Run: pnpm run dev                    ${NC}"
echo -e "${GREEN}========================================${NC}"
```

---

**Step 3 — Write `scripts/check-env.ts` (TypeScript, run via tsx).**

This script prints a PASS/FAIL table and exits with code 1 if any check fails.

```typescript
import { execSync } from "child_process";
import { existsSync, readFileSync } from "fs";
import { resolve } from "path";

interface Check {
  name: string;
  pass: boolean;
  detail: string;
}

const checks: Check[] = [];

function check(name: string, fn: () => { pass: boolean; detail: string }): void {
  try {
    const result = fn();
    checks.push({ name, ...result });
  } catch (e) {
    checks.push({ name, pass: false, detail: String(e) });
  }
}

// ── 1. Node.js version ────────────────────────────────────────────────────────
check("Node.js >= 20", () => {
  const v = process.version; // e.g. "v20.11.0"
  const major = parseInt(v.slice(1).split(".")[0] ?? "0", 10);
  return {
    pass: major >= 20,
    detail: major >= 20 ? v : `Found ${v} — needs v20+`,
  };
});

// ── 2. pnpm version ───────────────────────────────────────────────────────────
check("pnpm >= 9", () => {
  const v = execSync("pnpm --version", { encoding: "utf-8" }).trim();
  const major = parseInt(v.split(".")[0] ?? "0", 10);
  return {
    pass: major >= 9,
    detail: major >= 9 ? `v${v}` : `Found v${v} — needs v9+`,
  };
});

// ── 3. MAINTAFOX_ENV is set ───────────────────────────────────────────────────
check("MAINTAFOX_ENV is set", () => {
  // Read from .env.local if it exists (tsx does not auto-load .env files)
  const envPath = resolve(process.cwd(), ".env.local");
  let envValue = process.env["MAINTAFOX_ENV"];
  if (!envValue && existsSync(envPath)) {
    const raw = readFileSync(envPath, "utf-8");
    const match = raw.match(/^MAINTAFOX_ENV=(.+)$/m);
    envValue = match?.[1]?.trim();
  }
  const valid = ["development", "staging", "production"];
  const pass = typeof envValue === "string" && valid.includes(envValue);
  return {
    pass,
    detail: pass ? envValue! : `Got "${envValue ?? "unset"}" — must be: ${valid.join(" | ")}`,
  };
});

// ── 4. .env.local exists ──────────────────────────────────────────────────────
check(".env.local exists", () => {
  const exists = existsSync(resolve(process.cwd(), ".env.local"));
  return {
    pass: exists,
    detail: exists ? "found" : "Missing — run setup script to create it",
  };
});

// ── 5. No signing key in dev environment ─────────────────────────────────────
check("No signing key in .env.local (security)", () => {
  const envPath = resolve(process.cwd(), ".env.local");
  if (!existsSync(envPath)) {
    return { pass: true, detail: "no .env.local to check" };
  }
  const raw = readFileSync(envPath, "utf-8");
  const lines = raw.split("\n").filter((l) => l.startsWith("TAURI_SIGNING_PRIVATE_KEY="));
  const hasValue = lines.some((l) => {
    const val = l.split("=")[1]?.trim() ?? "";
    return val.length > 0 && !val.startsWith("#");
  });
  return {
    pass: !hasValue,
    detail: hasValue
      ? "SECURITY VIOLATION: TAURI_SIGNING_PRIVATE_KEY is set in .env.local. Remove it immediately."
      : "not set (correct)",
  };
});

// ── Print results table ───────────────────────────────────────────────────────
const COL = 45;
console.log("\nMaintafox Environment Preflight Check");
console.log("─".repeat(60));
console.log(`${"Check".padEnd(COL)} Result`);
console.log("─".repeat(60));

let anyFail = false;
for (const c of checks) {
  const icon = c.pass ? "PASS" : "FAIL";
  const color = c.pass ? "\x1b[32m" : "\x1b[31m";
  const reset = "\x1b[0m";
  console.log(`${c.name.padEnd(COL)} ${color}${icon}${reset}  ${c.detail}`);
  if (!c.pass) anyFail = true;
}

console.log("─".repeat(60));
if (anyFail) {
  console.log("\n\x1b[31mOne or more checks failed. Resolve the items above.\x1b[0m\n");
  process.exit(1);
} else {
  console.log("\n\x1b[32mAll checks passed.\x1b[0m\n");
}
```

---

**Acceptance criteria:**
- `.\scripts\setup.ps1` runs to completion on Windows with all prerequisites present
- `scripts/check-env.ts` prints a PASS/FAIL table and exits 0 when the environment is correct
- Running `setup.ps1` twice produces no errors (idempotent)
- `.env.local` is created by the script when absent

---

### Supervisor Verification — Sprint S1

**V1 — Setup script runs and shows progress.**
Open PowerShell in the project folder. Run:
```powershell
.\scripts\setup.ps1
```
The script should print step-by-step output with green `[PASS]` lines for each tool found.
The last output should be the green "Maintafox dev environment is ready" box. If the
script stops early and prints a red `[FAIL]` line, read the instruction shown — it will
name the missing prerequisites. Follow the instruction, then re-run.

**V2 — Environment checker shows all PASS.**
After the setup script completes, run:
```
pnpm tsx scripts/check-env.ts
```
A table should appear with a column of `PASS` values in green. If any row shows `FAIL` in
red, read the detail column and flag the check name and detail text.

**V3 — `.env.local` was created.**
In VS Code Explorer, confirm `.env.local` is present in the project root. Open it — it
should contain `MAINTAFOX_ENV=development` at the top. It should NOT contain a long random
string next to `TAURI_SIGNING_PRIVATE_KEY`. If the file is missing or contains a signing
key value, flag it.

**V4 — Idempotency check.**
Run `.\scripts\setup.ps1` a second time without changing anything. It should complete
again with green output. If the second run crashes with an error about something already
existing, flag it.

---

## Sprint S2 — Secrets and Environment Baseline

### AI Agent Prompt

You are establishing the complete secrets management governance for the Maintafox Desktop
project. This sprint creates the security policy documents, configures the gitleaks
allowlist, and writes the CI secrets registration guide. No application logic is modified.

---

**Step 1 — Write the full `.env.example` file.**

Replace the placeholder from Sprint S1 of File 01 with the complete annotated version:

```
# =============================================================================
#  MAINTAFOX DESKTOP — Environment Configuration Template
# =============================================================================
#  Copy this file to .env.local for local development.
#  .env.local is gitignored and must NEVER be committed.
#  Do not add real values to this file — it is version-controlled.
# =============================================================================

# ── Core Environment ──────────────────────────────────────────────────────────
# Valid values: development | staging | production
MAINTAFOX_ENV=development

# ── Local Database ────────────────────────────────────────────────────────────
# Absolute or relative path to the SQLite database file.
# Leave blank to use the OS app-data default path (recommended for production).
# In development, the default is ./dev-data/maintafox_dev.db
MAINTAFOX_DB_PATH=

# Enable SQLCipher encryption for the local database.
# Set to true only when the encryption key is managed via the OS keyring.
# Valid values: true | false
MAINTAFOX_DB_ENCRYPT=false

# ── Developer Tools ───────────────────────────────────────────────────────────
# Enable verbose SQL query logging (sea-orm + sqlx).
# Set to true only in development — never in staging or production.
MAINTAFOX_SQL_LOG=false

# Rust log level filter (passed to tracing-subscriber EnvFilter).
# Examples: info  |  debug  |  maintafox=debug,sea_orm=warn
RUST_LOG=maintafox=info

# ── VPS Sync (optional in development) ───────────────────────────────────────
# Base URL for the Maintafox VPS control-plane API (sync, license, updates).
# Leave blank to run fully offline in development mode.
MAINTAFOX_VPS_URL=

# ── Tauri Update Signing Keys (CI/Release pipeline — NOT developer machines) ─
# THESE KEYS MUST NEVER BE SET IN A DEVELOPER'S .env.local FILE.
# They are injected by the GitHub Actions release workflow via repository secrets.
# If you see a real value here, rotate the key immediately and open a security issue.
# TAURI_SIGNING_PRIVATE_KEY=
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD=
```

---

**Step 2 — Write `docs/SECRETS_MANAGEMENT.md`.**

Create this file with six sections:

**Section 1 — Secret Categories and Where They Live**

| Secret Category | Storage Location | Who Manages It |
|---|---|---|
| Developer environment secrets | `.env.local` (gitignored, per machine) | Each developer |
| CI signing and deployment keys | GitHub Actions repository secrets | Release manager |
| Staging environment secrets | GitHub Actions `staging` environment secrets (gated) | Release manager |
| Production environment secrets | GitHub Actions `production` environment secrets (2 reviewers) | Release manager |
| Runtime session material | OS keyring via Rust `keyring` crate | Rust application core |
| Local database encryption key | OS keyring via Rust `keyring` crate | Rust application core |

**Section 2 — Secret Naming Convention**

All secret names follow: `MAINTAFOX_{CONTEXT}_{PURPOSE}`

Examples:
- `MAINTAFOX_TAURI_SIGNING_KEY` — Tauri update bundle signing private key
- `MAINTAFOX_TAURI_SIGNING_PASSWORD` — Password for the signing key
- `MAINTAFOX_VPS_DEPLOY_KEY` — SSH key for VPS deployment operations
- `MAINTAFOX_VPS_API_TOKEN` — Bearer token for VPS API authentication

**Section 3 — Rotation Policy**

| Secret Type | Rotation Frequency | Trigger for Immediate Rotation |
|---|---|---|
| Tauri signing key pair | 12 months | Suspected key exposure |
| VPS API tokens | 6 months | Personnel changes, suspected exposure |
| VPS deploy keys | 12 months | Suspected compromise |
| Session tokens | Per session | Logout, device revocation, or step-up reauth |
| DB encryption key | Only on security incident | Never routine-rotated (tied to data) |

**Section 4 — Prohibited Storage Locations**

Secrets must NEVER be stored in:
- Git history (including deleted files — git history is permanent)
- Application log files or tracing output
- SQLite rows in plaintext form (encrypted DB rows are acceptable for session material
  under OS keyring governance)
- IPC response payloads sent to the frontend
- `tauri.conf.json` or any file tracked in the repository
- CI workflow `echo` or `run` steps that would print to log output

If a secret is found in a prohibited location, treat it as a confirmed exposure and rotate
immediately, even if the repository is private.

**Section 5 — Secret Scanning**

GitHub secret scanning is enabled on the repository (Settings → Security). Additionally,
the CI `security-audit` job runs `gitleaks` on every PR to `main` and `staging`, and on
a weekly schedule against the full git history.

The `.gitleaks.toml` file at the project root defines allowlisted patterns for intentional
placeholder key names in `.env.example` and documentation files.

**Section 6 — Incident Response Protocol**

If a secret is suspected or confirmed to have leaked:

1. **Rotate immediately.** Do not investigate before rotating — rotate first.
2. Revoke the exposed token or key at the issuing service (GitHub secrets, VPS panel).
3. Generate and register replacement credentials.
4. Scan git history: `git log --all -p | grep "{first-8-chars-of-exposed-value}"`
   If found, note the commit SHA.
5. If the secret was in a public repository or accessed by an unauthorized party,
   assess scope of exposure and notify affected parties.
6. Open a GitHub issue labeled `type:security` and `priority:critical` documenting:
   - What was exposed, when, and how it was discovered
   - What was rotated and when
   - Whether unauthorized access is suspected
7. Conduct a post-incident review within 72 hours to close the exposure vector.

---

**Step 3 — Write `.github/environments.md` (manual GitHub setup reference).**

```markdown
# GitHub Environments Setup

This file describes the manual configuration required in GitHub repository Settings →
Environments for the Maintafox Desktop project.

## staging environment

- **Name:** `staging`
- **Required reviewers:** 1 (designated release manager or senior developer)
- **Deployment branch rule:** only deploy from `staging` branch
- **Environment secrets:**
  - `MAINTAFOX_VPS_URL` — staging VPS API endpoint
  - `MAINTAFOX_VPS_API_TOKEN` — staging API token

## production environment

- **Name:** `production`
- **Required reviewers:** 2 (both must be from the release-manager team)
- **Wait timer:** 5 minutes (gives reviewers time to abort if something looks wrong)
- **Deployment branch rule:** only deploy from `main` branch
- **Environment secrets:**
  - `MAINTAFOX_VPS_URL` — production VPS API endpoint
  - `MAINTAFOX_VPS_API_TOKEN` — production API token
  - `MAINTAFOX_TAURI_SIGNING_KEY` — Tauri bundle signing private key
  - `MAINTAFOX_TAURI_SIGNING_PASSWORD` — Signing key password

## Repository-level secrets (not environment-scoped)

These are available to all workflows regardless of environment:
  - `GITLEAKS_LICENSE` — Gitleaks Pro license (if in use)

## Setting up secrets

1. Go to repository Settings → Secrets and variables → Actions
2. For repository-level secrets: click "New repository secret"
3. For environment secrets: click the environment name, then "Add secret"
Never share secret values over email, chat, or issues.
```

---

**Step 4 — Update `.gitleaks.toml`** (verify or create):

```toml
[allowlist]
description = "Safe patterns — placeholder key names without values"
paths = [
  ".env.example",
  "docs/SECRETS_MANAGEMENT.md",
  ".github/environments.md",
  "docs/CODE_REVIEW_GUIDE.md",
]
regexes = [
  '''TAURI_SIGNING_PRIVATE_KEY=$''',
  '''TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$''',
  '''# TAURI_SIGNING''',
  '''MAINTAFOX_VPS_API_TOKEN=$''',
]
```

---

**Acceptance criteria:**
- `.env.example` contains all required entries including the security prohibition notice
- `docs/SECRETS_MANAGEMENT.md` contains all 6 sections
- `.github/environments.md` documents setup for `staging` and `production`
- `.gitleaks.toml` includes all allowlisted patterns
- Running `gitleaks detect --no-git` on the repository reports 0 leaks on the current
  file set

---

### Supervisor Verification — Sprint S2

**V1 — env.example has the security prohibition.**
Open `.env.example` in Explorer. Scroll to the section titled "Tauri Update Signing Keys".
It should contain a comment saying `THESE KEYS MUST NEVER BE SET IN A DEVELOPER'S .env.local
FILE` and both key lines should be commented out with `#`. If a real key value appears on
either of those lines, flag it immediately as a security issue.

**V2 — Secrets management document is complete.**
Open `docs/SECRETS_MANAGEMENT.md`. It should have 6 numbered sections. Confirm the
"Incident Response Protocol" section is present and contains numbered steps. If the
document is empty or fewer than 6 sections exist, flag it.

**V3 — No secrets in `.env.local`.**
Open `.env.local`. Read through all lines. Confirm there is nothing that looks like a real
password, token, or long cryptographic string next to any key name. The value next to
`MAINTAFOX_ENV` should just be `development`. Any long random string is suspicious — flag
it immediately.

**V4 — Gitleaks scan passes (action required — needs command line).**
In the terminal, if `gitleaks` is installed, run:
```
gitleaks detect --no-git
```
It should complete with the message `0 leaks detected`. If it reports a leak with a file
path and line number, copy the report and flag it.

**V5 — GitHub Environments (manual — requires GitHub admin access).**
In the GitHub repository, go to Settings → Environments. Confirm that both `staging` and
`production` environments are listed. Click `staging` — it should show at least 1 required
reviewer. Click `production` — it should show 2 required reviewers and a wait timer. If
either is missing or misconfigured, flag it as "pending manual setup".

---

## Sprint S3 — Local Database Tooling and Developer Utilities

### AI Agent Prompt

You are building the local developer database tooling for the Maintafox Desktop project.
A developer — including a newly onboarded AI coding agent — must be able to initialize a
clean local SQLite database, run all migrations, load seed data, and inspect the tables
visually, all in under two minutes from a fresh environment.

---

**Step 1 — Write the database connection module in Rust.**

Write `src-tauri/src/db/mod.rs`:
```rust
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::time::Duration;

use crate::errors::AppResult;

/// Initialize the SQLite database connection and apply WAL + performance pragmas.
///
/// `db_path` is the absolute path to the `.db` file.
/// The directory must already exist before calling this function.
pub async fn init_db(db_path: &str) -> AppResult<DatabaseConnection> {
    tracing::info!("Opening database: {}", db_path);

    let url = format!("sqlite://{}?mode=rwc", db_path);

    let mut opts = ConnectOptions::new(url);
    opts.max_connections(5)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(120))
        .sqlx_logging(
            std::env::var("MAINTAFOX_SQL_LOG")
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        );

    let db = Database::connect(opts).await?;

    // Apply SQLite pragmas for WAL mode, foreign keys, and performance
    use sea_orm::ConnectionTrait;
    db.execute_unprepared("PRAGMA journal_mode=WAL;").await?;
    db.execute_unprepared("PRAGMA foreign_keys=ON;").await?;
    db.execute_unprepared("PRAGMA busy_timeout=5000;").await?;
    db.execute_unprepared("PRAGMA cache_size=-20000;").await?;
    db.execute_unprepared("PRAGMA temp_store=MEMORY;").await?;

    tracing::info!("Database connection established with WAL mode.");
    Ok(db)
}

/// Run all pending sea-orm migrations.
pub async fn run_migrations(db: &DatabaseConnection) -> AppResult<()> {
    // Migration module will be populated progressively as schema is defined.
    // This function is a safe no-op when no migrations are pending.
    tracing::info!("Running pending database migrations...");
    // PLACEHOLDER: when sea-orm-migration crate is integrated, call:
    //   Migrator::up(db, None).await?;
    tracing::info!("Migrations complete.");
    Ok(())
}
```

---

**Step 2 — Create the initial migration files.**

Add `sea-orm-migration` to `src-tauri/Cargo.toml` dependencies:
```toml
sea-orm-migration = { version = "1", features = ["sqlx-sqlite", "runtime-tokio-rustls"] }
```

Create `src-tauri/migrations/mod.rs`:
```rust
use sea_orm_migration::prelude::*;

mod m20260331_000001_system_tables;
mod m20260331_000002_user_tables;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260331_000001_system_tables::Migration),
            Box::new(m20260331_000002_user_tables::Migration),
        ]
    }
}
```

Create `src-tauri/migrations/m20260331_000001_system_tables.rs`:
```rust
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260331_000001_system_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // system_config: app-level key-value settings
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("system_config"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("key")).string().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("value")).text())
                    .col(ColumnDef::new(Alias::new("updated_at")).timestamp().not_null())
                    .to_owned(),
            )
            .await?;

        // trusted_devices: devices that have completed online first-login
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("trusted_devices"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Alias::new("device_fingerprint")).string().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("device_label")).string())
                    .col(ColumnDef::new(Alias::new("user_id")).uuid().not_null())
                    .col(ColumnDef::new(Alias::new("trusted_at")).timestamp().not_null())
                    .col(ColumnDef::new(Alias::new("last_seen_at")).timestamp())
                    .col(ColumnDef::new(Alias::new("is_revoked")).boolean().not_null().default(false))
                    .col(ColumnDef::new(Alias::new("revoked_at")).timestamp())
                    .col(ColumnDef::new(Alias::new("revoked_reason")).text())
                    .to_owned(),
            )
            .await?;

        // audit_events: immutable append-only event journal
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("audit_events"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Alias::new("event_type")).string().not_null())
                    .col(ColumnDef::new(Alias::new("actor_id")).uuid())
                    .col(ColumnDef::new(Alias::new("actor_name")).string())
                    .col(ColumnDef::new(Alias::new("entity_type")).string())
                    .col(ColumnDef::new(Alias::new("entity_id")).uuid())
                    .col(ColumnDef::new(Alias::new("summary")).text())
                    .col(ColumnDef::new(Alias::new("detail_json")).text())
                    .col(ColumnDef::new(Alias::new("device_id")).uuid())
                    .col(ColumnDef::new(Alias::new("occurred_at")).timestamp().not_null())
                    .to_owned(),
            )
            .await?;

        // app_sessions: active local sessions
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("app_sessions"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Alias::new("user_id")).uuid().not_null())
                    .col(ColumnDef::new(Alias::new("device_id")).uuid())
                    .col(ColumnDef::new(Alias::new("created_at")).timestamp().not_null())
                    .col(ColumnDef::new(Alias::new("expires_at")).timestamp().not_null())
                    .col(ColumnDef::new(Alias::new("last_activity_at")).timestamp())
                    .col(ColumnDef::new(Alias::new("is_revoked")).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Alias::new("app_sessions")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("audit_events")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("trusted_devices")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("system_config")).to_owned()).await?;
        Ok(())
    }
}
```

Create `src-tauri/migrations/m20260331_000002_user_tables.rs` with analogous structure,
creating these tables: `user_accounts`, `roles`, `permissions`, `role_permissions`,
`user_scope_assignments` — using the exact column definitions from PRD Section 6.7.

---

**Step 3 — Write database developer scripts.**

Create `scripts/dev-db-seed.ts`:
```typescript
import Database from "better-sqlite3";
import { resolve } from "path";
import { randomUUID } from "crypto";

const DB_PATH = resolve(process.cwd(), "dev-data", "maintafox_dev.db");
const db = new Database(DB_PATH);

const now = new Date().toISOString();

// System config baseline
const insertConfig = db.prepare(
  "INSERT OR IGNORE INTO system_config (key, value, updated_at) VALUES (?, ?, ?)",
);
insertConfig.run("app_version", "0.1.0-dev", now);
insertConfig.run("locale_default", "fr", now);
insertConfig.run("offline_grace_days", "7", now);

// Default system roles (PRD Section 6.7)
const insertRole = db.prepare(
  `INSERT OR IGNORE INTO roles (id, name, description, is_system, role_type, status, created_at)
   VALUES (?, ?, ?, ?, ?, ?, ?)`,
);

const systemRoles = [
  ["Administrator", "Full access to all modules and settings", true],
  ["Planner", "Plan and schedule work orders; manage backlog", true],
  ["Technician", "Execute work orders; record actuals and close-out", true],
  ["Supervisor", "Approve requests; review and verify completed work", true],
  ["Storekeeper", "Manage spare parts, inventory movements, and procurement", true],
  ["Viewer", "Read-only access to work history and reports", true],
] as const;

for (const [name, desc, isSys] of systemRoles) {
  insertRole.run(randomUUID(), name, desc, isSys ? 1 : 0, "system", "active", now);
}

// Development admin account
const insertUser = db.prepare(
  `INSERT OR IGNORE INTO user_accounts
   (id, username, identity_mode, is_active, force_password_change, created_at, updated_at)
   VALUES (?, ?, ?, ?, ?, ?, ?)`,
);
insertUser.run(randomUUID(), "admin", "local", 1, 1, now, now);

db.close();
console.log("Seed data inserted successfully.");
console.log("  - system_config: 3 entries");
console.log("  - roles: 6 system roles");
console.log("  - user_accounts: admin (password change required on first login)");
```

Create `scripts/dev-db-reset.ts`:
```typescript
import { execSync } from "child_process";
import { rmSync, existsSync, mkdirSync } from "fs";
import { resolve } from "path";

const dbPath = resolve(process.cwd(), "dev-data", "maintafox_dev.db");
const dbDir = resolve(process.cwd(), "dev-data");

console.log("Resetting development database...");

if (existsSync(dbPath)) {
  rmSync(dbPath);
  console.log("  Removed existing database file.");
}

if (!existsSync(dbDir)) {
  mkdirSync(dbDir, { recursive: true });
  console.log("  Created dev-data/ directory.");
}

console.log("  Running Rust migration runner...");
execSync(
  "cargo run --manifest-path src-tauri/Cargo.toml --bin migrate -- --db-path dev-data/maintafox_dev.db",
  { stdio: "inherit" },
);

console.log("  Seeding baseline data...");
execSync("pnpm tsx scripts/dev-db-seed.ts", { stdio: "inherit" });

console.log("\nDatabase reset complete: dev-data/maintafox_dev.db");
```

Add to root `package.json` scripts:
```json
"db:seed": "pnpm tsx scripts/dev-db-seed.ts",
"db:reset": "pnpm tsx scripts/dev-db-reset.ts"
```

---

**Step 4 — Ensure `dev-data/` is in `.gitignore`.**

Verify `.gitignore` contains `dev-data/`. If not, add it.

---

**Step 5 — Write `docs/DEV_ENVIRONMENT.md`.**

The document must contain these six sections written in plain language:

**Section 1 — Prerequisites**
- Rust stable toolchain (install via https://rustup.rs)
- Node.js 20 LTS (install via https://nodejs.org)
- pnpm v9+ (auto-installed by setup script)
- Microsoft Edge WebView2 Runtime — Windows only (install from Microsoft)
- Visual C++ Build Tools — Windows only (install VS Build Tools, select "Desktop development
  with C++")
- DB Browser for SQLite — optional but recommended for database inspection
  (https://sqlitebrowser.org)

**Section 2 — One-Command Setup**
- Windows: open PowerShell in the project root, run `.\scripts\setup.ps1`
- macOS/Linux: open Terminal in the project root, run `chmod +x scripts/setup.sh && ./scripts/setup.sh`
- The script checks prerequisites, installs dependencies, and creates `.env.local`
- After the script completes, run `pnpm run dev` to start the application

**Section 3 — Database Tooling**
- Reset and seed the local development database: `pnpm run db:reset`
- Seed baseline data only (skips migration): `pnpm run db:seed`
- Visual inspection: open DB Browser for SQLite, File → Open Database, select
  `dev-data/maintafox_dev.db`
- Tables created: `system_config`, `trusted_devices`, `audit_events`, `app_sessions`,
  `user_accounts`, `roles`, `permissions`, `role_permissions`, `user_scope_assignments`
- The `dev-data/` directory is gitignored and must never be committed

**Section 4 — Running the Application**
- Start in development mode: `pnpm run dev`
- The Vite server starts on port 1420 and Tauri launches the desktop window
- Hot Module Replacement (HMR) is active for React components — saving a `.tsx` file
  updates the running window without restarting
- Rust code changes require a full restart: press Ctrl+C, then `pnpm run dev` again
- Use F12 or Ctrl+Shift+I to open Developer Tools in the Tauri window

**Section 5 — Running Tests**
- Frontend tests: `pnpm run test` (Vitest, runs once)
- Frontend tests in watch mode: `pnpm run test:watch`
- Frontend coverage report: `pnpm run test -- --coverage`
- Rust unit tests: `pnpm run test:rust` (or `cd src-tauri && cargo test`)
- All checks at once (CI equivalent): run the jobs in this order:
  `pnpm run typecheck`, `pnpm run lint:check`, `pnpm run format:check`, `pnpm run test`,
  `pnpm run lint:rust`, `pnpm run format:rust:check`, `pnpm run test:rust`

**Section 6 — Common Issues and Fixes**
| Problem | Cause | Fix |
|---|---|---|
| WebView2 not found | Missing runtime on Windows | Download from Microsoft WebView2 page and install |
| `cl.exe` not found | MSVC Build Tools missing | Install VS Build Tools with "Desktop development with C++" |
| Port 1420 already in use | Another dev server running | Run `pnpm run clean` or kill the process using port 1420 |
| `cargo check` fails with "missing feature" | SQLite dev library missing on Linux | Run `sudo apt-get install libsqlite3-dev` |
| `.env.local` missing | Setup script not run yet | Run `.\scripts\setup.ps1` |
| DB Browser shows empty tables | Seed not run | Run `pnpm run db:reset` |
| `SQLX_OFFLINE` error in CI | `.sqlx/` query cache missing | Run `cargo sqlx prepare` inside `src-tauri/` after schema stabilizes |

---

**Acceptance criteria:**
- `pnpm run db:seed` inserts baseline records without errors
- `pnpm run db:reset` creates the database file at `dev-data/maintafox_dev.db`
- `docs/DEV_ENVIRONMENT.md` contains all 6 sections and the troubleshooting table
- `dev-data/` is listed in `.gitignore`
- DB Browser for SQLite can open `dev-data/maintafox_dev.db` and shows expected tables

---

### Supervisor Verification — Sprint S3

**V1 — Database reset script runs successfully.**
In the terminal at the project root, run:
```
pnpm run db:reset
```
Watch for the messages "Migrations complete", "Seed data inserted successfully", and
"Database reset complete". If any red error lines appear, copy the last 10 lines and flag
them.

**V2 — Database file exists with correct tables.**
After the reset, install DB Browser for SQLite from https://sqlitebrowser.org if you have
not already. Open it, click "Open Database", and navigate to `dev-data/maintafox_dev.db`.
In the "Database Structure" tab, you should see a list of tables. Confirm these are
present: `system_config`, `user_accounts`, `roles`, `trusted_devices`, `audit_events`.
If any table is missing, flag it.

**V3 — Seed data is visible in DB Browser.**
In DB Browser, click the "Browse Data" tab. Select the table `user_accounts` from the
dropdown. You should see one row with the username `admin` and `is_active = 1`.
Select the `roles` table — you should see 6 rows with names including "Administrator",
"Technician", and "Planner". If the rows are absent, flag it.

**V4 — Database is NOT in Git.**
In VS Code Explorer, right-click the `dev-data/` folder and select "Source Control: Show
Commit History" or "Add to .gitignore". The folder should already appear greyed out (not
tracked). Alternatively, confirm `.gitignore` contains `dev-data/`.
If the `dev-data/` folder appears as untracked (orange U icon) in the VS Code source
control panel with the database file listed, the gitignore is not working — flag it.

**V5 — Dev environment document is usable.**
Open `docs/DEV_ENVIRONMENT.md`. Confirm it has 6 sections and that the final section
contains a table with at least 5 rows of problem/fix pairs. Test one entry: if you are on
Windows and WebView2 is installed, the WebView2 row should match your situation. Flag if
the document is less than two pages long or the troubleshooting table is missing.

---

*End of Phase 1 · Sub-phase 01 · File 03*
*Next: File 04 — Documentation Governance and Working Agreements*
