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
