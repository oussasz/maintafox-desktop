# Maintafox Desktop

A cross-platform CMMS desktop application built with Tauri 2, React 18, TypeScript 5, and Rust.

## Prerequisites

- Node.js 20 LTS
- pnpm 9.15.0+
- Rust stable toolchain (1.78+)
- Tauri prerequisites for your platform: https://tauri.app/start/prerequisites/

## Getting Started

```bash
pnpm install
pnpm run dev
```

## Scripts

| Command | Description |
|---------|-------------|
| `pnpm dev` | Start Vite dev server (frontend only) |
| `pnpm tauri dev` | Start Tauri app with hot-reload |
| `pnpm build` | Type-check and build frontend |
| `pnpm typecheck` | Run TypeScript type checks |
| `pnpm lint:check` | Run ESLint (no fix) |
| `pnpm format:check` | Run Prettier check |
| `pnpm lint:rust` | Run Cargo Clippy |
| `pnpm test` | Run Vitest unit tests |
| `pnpm test:rust` | Run Cargo tests |
