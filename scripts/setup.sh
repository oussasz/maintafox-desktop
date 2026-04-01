#!/usr/bin/env bash
# Maintafox Desktop — Unix setup script
# Run from project root: bash scripts/setup.sh
set -euo pipefail

echo "Checking environment..."

if ! command -v node &>/dev/null; then
  echo "ERROR: Node.js is not installed. Please install Node.js 20 LTS." >&2
  exit 1
fi
echo "Node.js: $(node --version)"

if ! command -v pnpm &>/dev/null; then
  echo "ERROR: pnpm is not installed. Run: npm install -g pnpm" >&2
  exit 1
fi
echo "pnpm: $(pnpm --version)"

if ! command -v cargo &>/dev/null; then
  echo "ERROR: Rust/Cargo is not installed. Run: rustup-init" >&2
  exit 1
fi
echo "Cargo: $(cargo --version)"

echo ""
echo "Installing Node.js dependencies..."
pnpm install

echo ""
echo "Setup complete. Run 'pnpm tauri dev' to start."
