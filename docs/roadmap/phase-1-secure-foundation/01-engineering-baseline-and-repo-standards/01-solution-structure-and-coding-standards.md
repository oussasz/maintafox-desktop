# Phase 1 · Sub-phase 01 · File 01
# Solution Structure and Coding Standards

## Context and Purpose

This file establishes the foundational monorepo layout and coding conventions for the
Maintafox Desktop project. Every subsequent sprint, in every phase, depends on the
workspace structure and quality tooling assembled here. The AI agents that build later
modules must be able to drop into this workspace and follow the same conventions without
being re-briefed.

Maintafox is a **Tauri 2.x** desktop application. The frontend is **React 18 + TypeScript 5**,
the trusted application core is **Rust** (Tokio, sea-orm, sqlx, serde, tracing), and the
local data plane is **SQLite 3** (SQLCipher-capable). Styling uses **Tailwind CSS 3 +
Shadcn/ui + Radix UI**; complex data grids use **TanStack Table 8**; analytical
visualizations use **D3.js 7**; typed forms use **React Hook Form + Zod**.

The supervisor for this project is an **industrial maintenance engineer, not a programmer**.
Every Supervisor Verification section is written in plain operational language. No code
knowledge is required to complete a verification step.

## Prerequisites

- Git repository initialized and accessible
- Rust stable toolchain installed (`rustup` available)
- Node.js 20 LTS installed
- No existing files in the workspace other than the empty scaffold

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Monorepo Scaffold and Tauri Workspace | Full directory tree, root config files, pnpm workspace |
| S2 | TypeScript and React Coding Standards | tsconfig, ESLint, Prettier, Biome, standards document |
| S3 | Rust Workspace Standards | Cargo workspace, Clippy, rustfmt, AppError, standards document |

---

## Sprint S1 — Monorepo Scaffold and Tauri Workspace

### AI Agent Prompt

You are initializing the Maintafox Desktop monorepo from a clean repository. Your task is
to create the complete directory structure, configure the pnpm workspace, bootstrap the
Tauri project, and ensure the workspace compiles cleanly before any application logic is
added.

**Project identity:**
- Product: Maintafox Desktop
- Runtime: Tauri 2.x (cross-platform desktop shell)
- Frontend: React 18.x + TypeScript 5.x
- Application core: Rust (Tokio async, sea-orm, sqlx, serde, tracing)
- Database: SQLite 3.x (SQLCipher 4.x available as opt-in)
- Styling: Tailwind CSS 3.x + Shadcn/ui + Radix UI
- Package manager: pnpm with workspaces enabled

---

**Step 1 — Create the full directory tree.**

Produce every directory and placeholder file listed below. Use `.gitkeep` files to
preserve empty directories in Git.

```
maintafox-desktop/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── errors.rs
│   │   ├── commands/
│   │   │   └── mod.rs
│   │   ├── services/
│   │   │   └── mod.rs
│   │   ├── models/
│   │   │   └── mod.rs
│   │   ├── db/
│   │   │   └── mod.rs
│   │   ├── auth/
│   │   │   └── mod.rs
│   │   ├── sync/
│   │   │   └── mod.rs
│   │   ├── background/
│   │   │   └── mod.rs
│   │   └── security/
│   │       └── mod.rs
│   ├── migrations/
│   │   └── .gitkeep
│   ├── capabilities/
│   │   └── default.json
│   ├── icons/
│   │   └── .gitkeep
│   ├── Cargo.toml
│   ├── build.rs
│   └── tauri.conf.json
├── src/
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/
│   │   └── ui/
│   │       └── .gitkeep
│   ├── pages/
│   │   └── .gitkeep
│   ├── hooks/
│   │   └── .gitkeep
│   ├── services/
│   │   └── .gitkeep
│   ├── store/
│   │   └── .gitkeep
│   ├── lib/
│   │   └── utils.ts
│   ├── types/
│   │   └── index.ts
│   ├── i18n/
│   │   ├── index.ts
│   │   ├── fr/
│   │   │   └── common.json
│   │   └── en/
│   │       └── common.json
│   ├── test/
│   │   └── setup.ts
│   └── styles/
│       └── globals.css
├── shared/
│   └── ipc-types.ts
├── scripts/
│   ├── setup.ps1
│   ├── setup.sh
│   └── check-env.ts
├── docs/
│   └── adr/
│       └── .gitkeep
├── dev-data/
│   └── .gitkeep
├── .github/
│   ├── workflows/
│   │   └── .gitkeep
│   └── ISSUE_TEMPLATE/
│       └── .gitkeep
├── package.json
├── pnpm-workspace.yaml
├── tsconfig.json
├── vite.config.ts
├── tailwind.config.ts
├── postcss.config.js
├── index.html
├── .env.example
├── .gitignore
├── .nvmrc
└── README.md
```

---

**Step 2 — Write root configuration files.**

**`package.json`** (root):
```json
{
  "name": "maintafox-desktop",
  "version": "0.1.0-dev",
  "private": true,
  "packageManager": "pnpm@9.15.0",
  "engines": {
    "node": ">=20.0.0"
  },
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "tauri": "tauri",
    "lint": "eslint src shared --fix",
    "lint:check": "eslint src shared",
    "lint:rust": "cd src-tauri && cargo clippy -- -D warnings",
    "format": "prettier --write src shared",
    "format:check": "prettier --check src shared",
    "format:rust": "cd src-tauri && cargo fmt",
    "format:rust:check": "cd src-tauri && cargo fmt --check",
    "typecheck": "tsc --noEmit",
    "test": "vitest run",
    "test:watch": "vitest",
    "test:rust": "cd src-tauri && cargo test",
    "clean": "rimraf dist src-tauri/target node_modules/.cache"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@vitejs/plugin-react-swc": "^3.7.0",
    "@tauri-apps/vite-plugin": "^2.0.0",
    "vite": "^6.0.0",
    "typescript": "^5.7.0",
    "tailwindcss": "^3.4.0",
    "postcss": "^8.4.0",
    "autoprefixer": "^10.4.0",
    "@tailwindcss/forms": "^0.5.0",
    "@tailwindcss/typography": "^0.5.0",
    "prettier": "^3.4.0",
    "rimraf": "^6.0.0",
    "tsx": "^4.19.0",
    "vitest": "^2.1.0",
    "@vitest/coverage-v8": "^2.1.0",
    "jsdom": "^25.0.0",
    "@testing-library/react": "^16.0.0",
    "@testing-library/jest-dom": "^6.6.0",
    "better-sqlite3": "^11.0.0",
    "@types/better-sqlite3": "^7.6.0",
    "@types/node": "^22.0.0"
  },
  "dependencies": {
    "react": "^18.3.0",
    "react-dom": "^18.3.0",
    "@tauri-apps/api": "^2.0.0",
    "@tauri-apps/plugin-shell": "^2.0.0",
    "@tauri-apps/plugin-dialog": "^2.0.0",
    "@tauri-apps/plugin-fs": "^2.0.0",
    "i18next": "^24.0.0",
    "react-i18next": "^15.0.0",
    "@tanstack/react-table": "^8.20.0",
    "react-hook-form": "^7.54.0",
    "@hookform/resolvers": "^3.9.0",
    "zod": "^3.24.0",
    "zustand": "^5.0.0",
    "d3": "^7.9.0",
    "@types/d3": "^7.4.0",
    "clsx": "^2.1.0",
    "tailwind-merge": "^2.5.0",
    "lucide-react": "^0.468.0",
    "@radix-ui/react-dialog": "^1.1.0",
    "@radix-ui/react-dropdown-menu": "^2.1.0",
    "@radix-ui/react-select": "^2.1.0",
    "@radix-ui/react-toast": "^1.2.0",
    "@radix-ui/react-tooltip": "^1.1.0"
  }
}
```

**`pnpm-workspace.yaml`**:
```yaml
packages:
  - "src"
  - "shared"
```

**`tsconfig.json`** (root — complete strict configuration; details in Sprint S2):
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noImplicitOverride": true,
    "exactOptionalPropertyTypes": true,
    "noPropertyAccessFromIndexSignature": true,
    "forceConsistentCasingInFileNames": true,
    "skipLibCheck": true,
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"],
      "@shared/*": ["shared/*"]
    }
  },
  "include": ["src", "shared"],
  "exclude": ["node_modules", "src-tauri"]
}
```

**`vite.config.ts`**:
```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import { internalIpV4 } from "internal-ip";
import { resolve } from "path";

const mobile = !!/android|ios/.exec(process.env.TAURI_ENV_PLATFORM ?? "");

export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": resolve(__dirname, "./src"),
      "@shared": resolve(__dirname, "./shared"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: mobile ? "0.0.0.0" : false,
    hmr: mobile
      ? {
          protocol: "ws",
          host: await internalIpV4(),
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
```

**`tailwind.config.ts`**:
```typescript
import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        maintafox: {
          50: "#f8fafc",
          100: "#f1f5f9",
          200: "#e2e8f0",
          500: "#64748b",
          700: "#334155",
          900: "#0f172a",
          accent: "#f97316",
        },
      },
    },
  },
  plugins: [
    require("@tailwindcss/forms"),
    require("@tailwindcss/typography"),
  ],
} satisfies Config;
```

**`postcss.config.js`**:
```javascript
export default {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
};
```

**`index.html`**:
```html
<!doctype html>
<html lang="fr">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/svg+xml" href="/icons/maintafox.svg" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta
      http-equiv="Content-Security-Policy"
      content="default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' asset: https:; connect-src ipc: http://ipc.localhost"
    />
    <title>Maintafox</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

**`.nvmrc`**: content is `20`

**`.gitignore`**:
```
# Dependencies
node_modules/
.pnpm-store/

# Build outputs
dist/
build/
.tauri/

# Rust build artifacts
src-tauri/target/

# Environment files (never commit secrets)
.env
.env.local
.env.*.local

# Database files
*.db
*.db-shm
*.db-wal
dev-data/

# OS files
.DS_Store
Thumbs.db
desktop.ini

# Editor files
.vscode/settings.json
.idea/
*.suo
*.user

# CI artifacts
coverage/
*.lcov

# Logs
*.log
npm-debug.log*
pnpm-debug.log*

# Tauri
WixTools/
```

---

**Step 3 — Write Rust workspace files.**

**`src-tauri/Cargo.toml`**:
```toml
[package]
name = "maintafox"
version = "0.1.0-dev"
edition = "2021"
rust-version = "1.78"
build = "build.rs"

[lib]
name = "maintafox_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = ["protocol-asset"] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
sea-orm = { version = "1", features = ["sqlx-sqlite", "runtime-tokio-rustls", "macros"] }
sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio", "macros"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
thiserror = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
argon2 = "0.5"
keyring = "2"

[profile.release]
lto = "thin"
opt-level = "s"
strip = true
```

**`src-tauri/build.rs`**:
```rust
fn main() {
    tauri_build::build()
}
```

**`src-tauri/src/main.rs`**:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    maintafox_lib::run();
}
```

**`src-tauri/src/lib.rs`**:
```rust
pub mod auth;
pub mod background;
pub mod commands;
pub mod db;
pub mod errors;
pub mod models;
pub mod security;
pub mod services;
pub mod sync;

use tracing_subscriber::EnvFilter;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("maintafox=info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Maintafox application");
}
```

**`src-tauri/src/errors.rs`**: placeholder — written in full in Sprint S3.

**`src-tauri/src/commands/mod.rs`**:
```rust
use crate::errors::AppResult;

#[tauri::command]
pub async fn health_check() -> AppResult<serde_json::Value> {
    tracing::info!("health_check called");
    Ok(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
```

All other `mod.rs` files in `src-tauri/src/` can be empty module stubs for now.

**`src-tauri/tauri.conf.json`**:
```json
{
  "productName": "Maintafox",
  "version": "0.1.0-dev",
  "identifier": "systems.maintafox.desktop",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "Maintafox",
        "width": 1280,
        "height": 800,
        "minWidth": 1024,
        "minHeight": 600,
        "resizable": true,
        "center": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' asset: https:; connect-src ipc: http://ipc.localhost"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

**`src-tauri/capabilities/default.json`**:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capability set for Maintafox Desktop",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-open",
    "dialog:default",
    "fs:default"
  ]
}
```

---

**Step 4 — Write minimal React bootstrap files.**

**`src/main.tsx`**:
```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles/globals.css";

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("Root element not found");

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

**`src/App.tsx`**:
```tsx
export function App() {
  return (
    <div className="flex h-screen items-center justify-center bg-maintafox-900 text-white">
      <p className="text-xl font-semibold">Maintafox — initializing</p>
    </div>
  );
}
```

**`src/styles/globals.css`**:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;

@layer base {
  :root {
    font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI",
      Roboto, "Helvetica Neue", Arial, sans-serif;
    font-size: 14px;
    line-height: 1.5;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }

  * {
    box-sizing: border-box;
  }
}
```

**`src/lib/utils.ts`**:
```typescript
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
```

**`src/types/index.ts`**: empty placeholder with a comment:
```typescript
// Shared TypeScript type definitions for the Maintafox frontend.
// Module-specific types live alongside their module files.
// IPC contract types live in shared/ipc-types.ts.
export {};
```

**`shared/ipc-types.ts`**:
```typescript
// IPC contract types shared between src/ (frontend) and the Tauri command layer.
// Types defined here must be kept in sync with Rust structs in src-tauri/src/.

export interface HealthCheckResponse {
  status: "ok" | "error";
  version: string;
}
```

**`src/i18n/fr/common.json`**:
```json
{
  "app": {
    "name": "Maintafox",
    "loading": "Chargement en cours…",
    "error": "Une erreur est survenue"
  }
}
```

**`src/i18n/en/common.json`**:
```json
{
  "app": {
    "name": "Maintafox",
    "loading": "Loading…",
    "error": "An error occurred"
  }
}
```

**`src/i18n/index.ts`**:
```typescript
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import frCommon from "./fr/common.json";
import enCommon from "./en/common.json";

void i18n.use(initReactI18next).init({
  resources: {
    fr: { common: frCommon },
    en: { common: enCommon },
  },
  lng: "fr",
  fallbackLng: "en",
  defaultNS: "common",
  interpolation: {
    escapeValue: false,
  },
});

export default i18n;
```

**`src/test/setup.ts`**:
```typescript
import "@testing-library/jest-dom";
import { vi } from "vitest";

// Mock the Tauri IPC runtime. Tests do not have access to the Tauri binary.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));
```

---

**Step 5 — Write the `.env.example` file.**

The file contents are defined in detail in File 03 (Dev Environment) Sprint S1.
For now, create a minimal placeholder:

```
MAINTAFOX_ENV=development
MAINTAFOX_DB_PATH=
MAINTAFOX_DB_ENCRYPT=false
MAINTAFOX_SQL_LOG=false
MAINTAFOX_VPS_URL=
RUST_LOG=maintafox=info
# DO NOT set TAURI_SIGNING_PRIVATE_KEY in development environments
```

---

**Acceptance criteria:**
- `pnpm install` completes without errors
- `pnpm run typecheck` reports 0 TypeScript errors
- `cargo check` inside `src-tauri/` finishes with no errors
- `pnpm run dev` starts the Vite development server on port 1420
- `src/i18n/fr/common.json` and `src/i18n/en/common.json` are both present

---

### Supervisor Verification — Sprint S1

*You do not need to understand code to complete these checks. Follow each step exactly.*

**V1 — Project folder structure is correct.**
Open VS Code in the project folder. In the left Explorer panel, confirm you can see all of
the following folders: `src-tauri/`, `src/`, `shared/`, `scripts/`, `docs/`, `.github/`.
If any are missing, flag it with the name of the missing folder.

**V2 — Dependencies install without errors.**
Open the Terminal (View → Terminal). Run:
```
pnpm install
```
Wait for it to finish. The last lines should show a summary of packages installed with no
lines beginning with the word `ERROR`. If you see red error lines, copy the last 10 lines
and flag them.

**V3 — TypeScript check passes.**
In the same terminal, run:
```
pnpm run typecheck
```
The command should finish and show either no output or a message containing "0 errors". Any
output containing `error TS` followed by a number is a failure — copy the message and flag
it.

**V4 — Rust compiles cleanly.**
In the terminal, run:
```
cd src-tauri
cargo check
cd ..
```
The last line should say `Finished`. Any line starting with `error[E` is a failure — copy
the full error and flag it.

**V5 — Development server starts.**
Run `pnpm run dev`. Within 15 seconds, a line containing `localhost:1420` should appear.
Press Ctrl+C to stop. If the line never appears or an error is printed, flag it.

**V6 — i18n locale files are present.**
In Explorer, navigate to `src/i18n/`. Confirm you see two subfolders: `fr/` and `en/`.
Open `fr/common.json` — it should contain text with French words. Open `en/common.json` —
it should contain the same structure in English. If either file is missing or empty, flag it.

---

## Sprint S2 — TypeScript and React Coding Standards

### AI Agent Prompt

The monorepo scaffold from Sprint S1 is in place. Your task is to configure the full
TypeScript and React code-quality toolchain, enforce strict conventions, and write the
frontend coding standards reference document.

---

**Step 1 — Harden `tsconfig.json`** (update the root file created in S1 to add these
additional strict options):
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noImplicitOverride": true,
    "exactOptionalPropertyTypes": true,
    "noPropertyAccessFromIndexSignature": true,
    "noImplicitReturns": true,
    "noFallthroughCasesInSwitch": true,
    "forceConsistentCasingInFileNames": true,
    "skipLibCheck": true,
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"],
      "@shared/*": ["shared/*"]
    }
  },
  "include": ["src", "shared"],
  "exclude": ["node_modules", "src-tauri"]
}
```

---

**Step 2 — Install ESLint 9 and configure flat config.**

Install these devDependencies (add to `package.json`):
- `eslint@^9.0.0`
- `@typescript-eslint/eslint-plugin@^8.0.0`
- `@typescript-eslint/parser@^8.0.0`
- `eslint-plugin-react@^7.37.0`
- `eslint-plugin-react-hooks@^5.0.0`
- `eslint-plugin-import@^2.31.0`
- `eslint-import-resolver-typescript`

Create `eslint.config.js` at project root:
```javascript
import tseslint from "@typescript-eslint/eslint-plugin";
import tsparser from "@typescript-eslint/parser";
import reactPlugin from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import importPlugin from "eslint-plugin-import";

export default [
  {
    ignores: [
      "dist/**",
      "src-tauri/**",
      "node_modules/**",
      "*.config.js",
      "*.config.ts",
      "scripts/**",
    ],
  },
  {
    files: ["src/**/*.{ts,tsx}", "shared/**/*.ts"],
    languageOptions: {
      parser: tsparser,
      parserOptions: {
        project: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      react: reactPlugin,
      "react-hooks": reactHooks,
      import: importPlugin,
    },
    settings: {
      react: { version: "detect" },
      "import/resolver": { typescript: { alwaysTryTypes: true } },
    },
    rules: {
      // TypeScript strict rules
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-floating-promises": "error",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
      "@typescript-eslint/consistent-type-imports": ["error", { prefer: "type-imports" }],
      "@typescript-eslint/no-non-null-assertion": "warn",

      // React rules
      "react/jsx-uses-react": "off",
      "react/react-in-jsx-scope": "off",
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",

      // Import rules
      "import/no-cycle": "error",
      "import/no-unresolved": "error",
      "import/order": [
        "error",
        {
          "groups": ["builtin", "external", "internal", "parent", "sibling"],
          "pathGroups": [
            { "pattern": "@/**", "group": "internal" },
            { "pattern": "@shared/**", "group": "internal" }
          ],
          "newlines-between": "always",
          "alphabetize": { "order": "asc" }
        }
      ],

      // General quality rules
      "no-console": ["error", { allow: ["warn", "error"] }],
      "no-debugger": "error",
    },
  },
  // Stricter rules for service and store layers: explicit return types required
  {
    files: ["src/services/**/*.ts", "src/store/**/*.ts"],
    rules: {
      "@typescript-eslint/explicit-function-return-type": "warn",
    },
  },
  // No default exports in components, pages, hooks
  {
    files: [
      "src/components/**/*.tsx",
      "src/pages/**/*.tsx",
      "src/hooks/**/*.ts",
    ],
    rules: {
      "import/no-default-export": "error",
    },
  },
];
```

---

**Step 3 — Configure Prettier.**

Create `.prettierrc` at project root:
```json
{
  "semi": true,
  "singleQuote": false,
  "tabWidth": 2,
  "trailingComma": "all",
  "printWidth": 100,
  "bracketSameLine": false,
  "arrowParens": "always",
  "endOfLine": "lf"
}
```

Create `.prettierignore`:
```
dist/
src-tauri/
node_modules/
*.md
*.sql
dev-data/
```

---

**Step 4 — Configure Biome (fast CI linter/formatter).**

Install `@biomejs/biome@^1.9.0` as a devDependency.

Create `biome.json` at project root:
```json
{
  "$schema": "https://biomejs.dev/schemas/1.9.4/schema.json",
  "organizeImports": { "enabled": true },
  "linter": {
    "enabled": true,
    "rules": {
      "recommended": true,
      "suspicious": {
        "noExplicitAny": "error"
      }
    }
  },
  "formatter": {
    "enabled": true,
    "indentStyle": "space",
    "indentWidth": 2,
    "lineWidth": 100
  },
  "files": {
    "include": ["src/**", "shared/**"],
    "ignore": ["src/test/**"]
  }
}
```

Add to `package.json` scripts:
```json
"lint:biome": "biome check src shared",
"format:biome": "biome format --write src shared"
```

---

**Step 5 — Configure lint-staged and Husky pre-commit hook.**

Install devDependencies: `lint-staged@^15.0.0`, `husky@^9.0.0`.

Create `.lintstagedrc.json`:
```json
{
  "*.{ts,tsx}": ["eslint --fix --max-warnings=0", "prettier --write"],
  "*.{json,css}": ["prettier --write"]
}
```

Initialize Husky:
```bash
pnpm husky init
```

Write `.husky/pre-commit`:
```sh
#!/usr/bin/env sh
pnpm lint-staged
```

---

**Step 6 — Configure Vitest.**

Create `vitest.config.ts` at project root:
```typescript
import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/test/**", "src/main.tsx", "src/i18n/**"],
      thresholds: {
        lines: 60,
        functions: 60,
        branches: 50,
        statements: 60,
      },
    },
  },
  resolve: {
    alias: {
      "@": resolve(__dirname, "./src"),
      "@shared": resolve(__dirname, "./shared"),
    },
  },
});
```

---

**Step 7 — Write `docs/CODING_STANDARDS_FRONTEND.md`.**

The document must contain these seven sections in full prose:

**1. Naming Conventions**
- React components: PascalCase (`WorkOrderCard`, `EquipmentSearchBar`)
- Functions and variables: camelCase (`fetchWorkOrders`, `isLoading`)
- Constants: SCREAMING_SNAKE_CASE (`MAX_OFFLINE_GRACE_DAYS`, `DEFAULT_LOCALE`)
- File names for pages, hooks, services: kebab-case matching the export name
  (`work-order-card.tsx`, `use-work-order.ts`, `work-order-service.ts`)
- IPC command names: snake_case matching the Rust command function name
  (`create_work_order`, `get_equipment_list`)
- i18n keys: dot-notation scoped by module (`workOrder.status.inProgress`)

**2. Component Structure Rules**
- All hooks declared at the top of the component, before any conditional logic
- No business logic in JSX expressions — compute values in variables above the return
- Use early return for loading and error states before the main render
- Never define a component inside another component's function body
- Components receive data through props or read from the store — they never call IPC
  directly
- Maximum component file length: 250 lines; extract sub-components when exceeded

**3. Import Ordering**
- Group 1: Node built-ins (rarely needed in frontend)
- Group 2: External packages (React, Zod, TanStack, etc.)
- Group 3: Internal `@/` aliased imports
- Group 4: Relative imports
- One blank line between groups; ESLint `import/order` rule enforces this automatically

**4. State Management**
- Local component state (`useState`): UI-only state, form field values before submission
- Derived state (`useMemo`, `useCallback`): computed values from props or store
- Cross-component shared state: Zustand stores living in `src/store/`
- Server/IPC state: wrapped in custom hooks in `src/hooks/` that call `src/services/`
- No module may call `invoke()` directly from a component or a generic hook — all
  IPC calls are isolated in `src/services/` modules

**5. Error Handling Patterns**
- IPC service functions return `Result<T, AppError>` typed unions — never raw `try/catch`
  in component code
- Use Zod schemas to validate all data arriving from IPC before passing to component state
- Display errors using the centralized `ErrorBoundary` component or toast notifications —
  never `alert()` or raw console output in production paths
- All error messages shown to the user must use the `t()` translation function

**6. IPC Contract Rule**
- Every Tauri `invoke` call lives exclusively in a file under `src/services/`
- Service files are named after the domain they serve: `work-order-service.ts`,
  `equipment-service.ts`, `auth-service.ts`
- Service functions are typed: input types defined in `shared/ipc-types.ts`, output types
  validated with Zod at the service boundary
- No component, store, or hook may import from `@tauri-apps/api/core` directly

**7. i18n Rule**
- Every string visible to the user must be wrapped in `t("key")` using `react-i18next`
- No French or English text may appear as a string literal in any `.tsx` or `.ts` file
  outside of `src/i18n/`
- Missing translation keys must fail the build in production mode (configure `i18next`
  with `missingKeyHandler` that throws in development)
- Module-scoped namespace files are preferred over a single large `common.json`; each
  major module gets its own namespace file added in its implementation sprint

---

**Acceptance criteria:**
- `pnpm run lint:check` completes with 0 errors on the scaffold
- `pnpm run format:check` reports no formatting violations
- `pnpm run typecheck` passes cleanly
- `biome.json` is present with linter and formatter enabled
- `.husky/pre-commit` invokes `lint-staged`
- `docs/CODING_STANDARDS_FRONTEND.md` is present and contains all 7 sections

---

### Supervisor Verification — Sprint S2

**V1 — Lint check passes.**
In the terminal at the project root, run:
```
pnpm run lint:check
```
The command should finish without printing any lines containing the word `error`. Lines with
`warning` are acceptable. Copy and flag any `error` lines.

**V2 — Format check passes.**
Run:
```
pnpm run format:check
```
The terminal should print `All matched files use Prettier formatting!` or `0 files changed`.
If it lists file names with "needs reformatting", flag it.

**V3 — Frontend standards document exists and is complete.**
In Explorer, navigate to `docs/` and open `CODING_STANDARDS_FRONTEND.md`. Scroll through
it — it must contain exactly 7 numbered sections. The section titles should include "Naming
Conventions", "IPC Contract Rule", and "i18n Rule". If the document is empty, has fewer
than 7 sections, or any section is only a title with no content, flag it.

**V4 — Pre-commit hook is registered.**
In Explorer, navigate to `.husky/`. Confirm a file named `pre-commit` is present. Open it —
it should mention `lint-staged`. Flag if the file is absent.

**V5 — Biome config is present.**
Confirm `biome.json` exists in the project root. Open it — it should contain `"linter"` and
`"formatter"` sections. Flag if absent.

---

## Sprint S3 — Rust Workspace Standards

### AI Agent Prompt

The Tauri scaffold and TypeScript standards from Sprints S1 and S2 are in place. Your task
is to harden the Rust workspace with proper toolchain pinning, lint configuration, error
type system, and coding standards documentation.

---

**Step 1 — Pin the Rust toolchain.**

Create `rust-toolchain.toml` at the project root (alongside `src-tauri/`):
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy", "rust-analyzer"]
targets = [
  "x86_64-pc-windows-msvc",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
]
```

---

**Step 2 — Configure Cargo build settings.**

Create `src-tauri/.cargo/config.toml`:
```toml
[env]
SQLX_OFFLINE = "true"

[target.x86_64-pc-windows-msvc]
linker = "link.exe"

[profile.release]
lto = "thin"
opt-level = "s"
strip = true

[profile.dev]
opt-level = 1
debug = true
```

---

**Step 3 — Configure Clippy.**

Create `src-tauri/.clippy.toml`:
```toml
msrv = "1.78.0"
```

Add a `#![deny(...)]` preamble to `src-tauri/src/lib.rs`. Replace the existing `lib.rs`
content with:
```rust
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod auth;
pub mod background;
pub mod commands;
pub mod db;
pub mod errors;
pub mod models;
pub mod security;
pub mod services;
pub mod sync;

use tracing_subscriber::EnvFilter;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("maintafox=info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
        ])
        .run(tauri::generate_context!())
        // EXPECT: If the Tauri context cannot be loaded, the application binary is corrupt or
        // the tauri.conf.json is missing. Panic at startup is the correct behavior.
        .expect("error while running Maintafox application");
}
```

---

**Step 4 — Configure rustfmt.**

Create `src-tauri/rustfmt.toml`:
```toml
edition = "2021"
tab_spaces = 4
max_width = 120
imports_granularity = "Crate"
group_imports = "StdExternalCrate"
trailing_comma = "Vertical"
newline_style = "Unix"
use_small_heuristics = "Default"
```

---

**Step 5 — Write the AppError type system in `src-tauri/src/errors.rs`.**

```rust
use thiserror::Error;

/// Unified application error type. All service and command functions return
/// `AppResult<T>` rather than mixing error types across the IPC boundary.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Record not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },

    #[error("Validation failed: {0:?}")]
    ValidationFailed(Vec<String>),

    #[error("Sync error: {0}")]
    SyncError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Permission denied: action '{action}' on resource '{resource}'")]
    Permission { action: String, resource: String },

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// Serialize AppError to JSON for the Tauri IPC boundary.
/// Frontend receives: { "code": "NOT_FOUND", "message": "...", "details": null }
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        let code = match self {
            AppError::Database(_) => "DATABASE_ERROR",
            AppError::Auth(_) => "AUTH_ERROR",
            AppError::NotFound { .. } => "NOT_FOUND",
            AppError::ValidationFailed(_) => "VALIDATION_FAILED",
            AppError::SyncError(_) => "SYNC_ERROR",
            AppError::Io(_) => "IO_ERROR",
            AppError::Serialization(_) => "SERIALIZATION_ERROR",
            AppError::Permission { .. } => "PERMISSION_DENIED",
            AppError::Internal(_) => "INTERNAL_ERROR",
        };
        state.serialize_field("code", code)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

/// Convenience alias used by all command and service functions.
pub type AppResult<T> = Result<T, AppError>;
```

---

**Step 6 — Update `src-tauri/src/commands/mod.rs` with tracing.**

Replace the placeholder from S1:
```rust
use crate::errors::AppResult;

/// Health check command. Returns application status and version.
/// Used by the frontend to verify the IPC bridge is operational.
#[tauri::command]
pub async fn health_check() -> AppResult<serde_json::Value> {
    tracing::info!("health_check invoked");
    Ok(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
```

---

**Step 7 — Write `docs/CODING_STANDARDS_RUST.md`.**

The document must contain these six sections in full prose:

**1. Module Organization**
- `commands/`: IPC handler functions only. A command function delegates immediately to a
  service function and does not contain business logic.
- `services/`: All business logic lives here. Service functions are async, accept typed
  parameters, and return `AppResult<T>`.
- `models/`: Domain entity structs, `serde` serialization implementations, and entity
  validation logic.
- `db/`: Database connection setup, migration runner, and low-level query helpers.
- `auth/`, `security/`, `sync/`, `background/`: Domain-specific runtime modules.
- `errors.rs`: `AppError` and `AppResult` only — no other code.

**2. Error Handling Rules**
- All functions must return `AppResult<T>` or a type that wraps `AppError`.
- Never use `.unwrap()` or `.expect()` in production code paths without an inline comment
  explaining why a panic is the correct behavior at that exact point (e.g., startup
  invariants).
- Use `?` operator to propagate errors. Use `AppError::ValidationFailed(vec![...])` for
  invalid inputs. Use `AppError::NotFound { entity, id }` for missing records.
- Do not swallow errors with `let _ = result`. If a result is intentionally ignored, add
  a `// SAFETY:` comment.

**3. Async Conventions**
- All service functions that touch the database, filesystem, or network are `async`.
- Never block an async thread with synchronous I/O. Use `tokio::task::spawn_blocking` for
  CPU-intensive or blocking operations with an inline comment explaining why.
- Never fire-and-forget a spawned task without tracking it in the background task
  supervisor. Untracked tasks that silently fail are bugs.

**4. Logging Rules**
- `tracing::info!`: normal operations (command invoked, record created, migration complete)
- `tracing::warn!`: recoverable anomalies (retry triggered, non-critical config missing)
- `tracing::error!`: failures that affect correctness (DB write failed, sync rejected)
- `tracing::debug!`: verbose diagnostic detail for development investigation
- Never use `println!` in production code paths.
- Never log secret values, passwords, session tokens, or private key material. Log IDs
  and non-sensitive labels only.

**5. Security Rules**
- All user-supplied data entering a Tauri command must be validated before reaching the
  service layer. Use `AppError::ValidationFailed` for rejections.
- Never construct SQL by string formatting with user input. Use sea-orm query builders or
  `sqlx` parameterized queries exclusively.
- Secrets are stored in the OS keyring using the `keyring` crate. Never write passwords,
  tokens, or encryption keys to SQLite rows, log output, or IPC responses.
- The `capabilities/default.json` file controls what the WebView is allowed to do. Never
  add a capability without documenting the reason in the PR description.

**6. IPC Boundary Rules**
- IPC commands are the only entry point from the frontend. Every command function is
  registered in `tauri::generate_handler![]` in `lib.rs`.
- Commands must validate inputs, delegate to services, and return `AppResult<T>`. No
  direct database access in command functions.
- Every new command must be added to `docs/IPC_COMMAND_REGISTRY.md` in the same PR that
  implements it.
- IPC response types must implement `serde::Serialize`. They must also be mirrored as
  TypeScript types in `shared/ipc-types.ts`.

---

**Acceptance criteria:**
- `cargo clippy -- -D warnings` passes with 0 errors inside `src-tauri/`
- `cargo fmt --check` reports 0 formatting violations
- `cargo test` completes (0 tests is acceptable at this stage)
- `cargo check` builds with no warnings
- `docs/CODING_STANDARDS_RUST.md` is present with all 6 sections
- `src-tauri/src/errors.rs` contains the full `AppError` enum and `AppResult` alias

---

### Supervisor Verification — Sprint S3

**V1 — Rust lint passes.**
In the terminal at the project root, run:
```
pnpm run lint:rust
```
The last line should say `Finished` or `0 warnings`. Any line starting with
`error[E` is a failure — copy the full error message and flag it.

**V2 — Rust format check passes.**
Run:
```
pnpm run format:rust:check
```
It should complete silently or print a message with "0 diffs". If it prints file names
followed by "diff:", flag it.

**V3 — Rust tests run.**
Run:
```
pnpm run test:rust
```
The output should end with `test result: ok. 0 passed; 0 failed` or higher. Any line
containing `FAILED` is a failure — flag it with the test name.

**V4 — Rust standards document is complete.**
Open `docs/CODING_STANDARDS_RUST.md`. It must contain 6 numbered sections. Confirm the
sections on "Error Handling Rules", "Security Rules", and "Logging Rules" are present and
contain several paragraphs each. A file with only section titles and no content is a
failure — flag it.

**V5 — Health check command works.**
Start the application with `pnpm run dev`. When the Tauri window opens, press F12 or
Ctrl+Shift+I to open Developer Tools. Click the "Console" tab. Type this command and press
Enter:
```javascript
window.__TAURI__.core.invoke('health_check')
```
The console should show a result like `{ status: 'ok', version: '0.1.0-dev' }`. If the
console shows `command not found` or any error, flag it.

---

*End of Phase 1 · Sub-phase 01 · File 01*
*Next: File 02 — Branching, Review, and Quality Gates*
