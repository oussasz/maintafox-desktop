// Maintafox Desktop — environment check script
// Usage: tsx scripts/check-env.ts

import { execSync } from "node:child_process";

interface Check {
  name: string;
  command: string;
  minVersion?: string;
}

const checks: Check[] = [
  { name: "Node.js", command: "node --version" },
  { name: "pnpm", command: "pnpm --version" },
  { name: "Cargo", command: "cargo --version" },
  { name: "rustup", command: "rustup --version" },
];

let failed = false;

for (const check of checks) {
  try {
    const result = execSync(check.command, { encoding: "utf-8" }).trim();
    console.log(`✓ ${check.name}: ${result}`);
  } catch {
    console.error(`✗ ${check.name}: NOT FOUND`);
    failed = true;
  }
}

if (failed) {
  console.error("\nSome prerequisites are missing. See README.md for setup instructions.");
  process.exit(1);
} else {
  console.log("\nAll prerequisites satisfied.");
}
