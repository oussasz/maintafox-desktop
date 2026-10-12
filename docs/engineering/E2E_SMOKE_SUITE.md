# End-to-end smoke suite (plan, not built yet)

Phase 6.9 asked for a WebDriver smoke suite on the Tauri build "if it fits". It did not fit in
Phase 6: the work below needs a release build on a Windows runner with a matching Edge WebDriver,
and none of it can run inside the current unit-test jobs. This page records what to build so the
next step does not start from zero.

## What exists today

- Rust: about 1,100 library tests on in-memory SQLite (`cargo test --lib`), including a
  multi-device sync harness against the real control API (`pnpm sync:harness`).
- Frontend: vitest + Testing Library component tests (`npx vitest run --coverage`), with the IPC
  layer replaced per test.
- Nothing drives the real window: the webview, the IPC bridge, startup (database key, migrations,
  capture install) and the tray are only covered by hand.

## Tooling

- `tauri-driver` (cargo install; Tauri v2 "WebDriver" testing guide) wraps the platform
  WebDriver: `msedgedriver` on Windows (its version must match the installed WebView2),
  `WebKitWebDriver` on Linux. macOS has no WebDriver for WKWebView, so the suite runs on Windows
  (and optionally Linux) only.
- WebdriverIO (`@wdio/cli`, mocha framework) as the client, in a separate `e2e/` package so the
  unit-test toolchain is not affected.
- Build: `pnpm tauri build --debug --no-bundle` with a test control-API URL
  (`MAINTAFOX_CONTROL_PLANE_API_BASE_URL=http://127.0.0.1:<port>`), started against a fresh
  app-data directory per run (the database key is created on first start).

## Smoke scenarios (first set)

1. First start: the window opens, startup events reach "ready", the sign-in screen shows.
2. Sign-in with the seeded test administrator; the shell, sidebar and notification bell render.
3. Create an intervention request with the asset picker, convert it to a work order, move the
   work order to in progress and close it; the closed work order appears in the list.
4. Inventory: create an article, receive stock, see the balance; add a unit conversion and
   convert a quantity.
5. Reliability dashboard for a seeded equipment: KPI cards and the failure-mode Pareto render;
   the PDF export produces a file (saved through the dialog plugin to a fixed test path).
6. Audit log: "Verify integrity" reports the audit chains intact.
7. Lock the session (idle lock), unlock with the PIN.
8. Language switch FR ↔ EN changes visible labels (no raw keys on the main pages).

Each scenario asserts on visible text and roles (no CSS selectors) and fails on any console error
or IPC error toast.

## CI

- A Windows job (`windows-latest`) after the unit jobs: install the WebView2-matching
  `msedgedriver`, `cargo install tauri-driver --locked`, build debug, run `pnpm e2e`.
- Scenarios that sync (none in the first set) would reuse the sync harness's control-API start-up.
- Time budget about 25 minutes (the build dominates): run on pushes to release branches and
  nightly, not on every pull request.

## Effort

About 2–3 days: tooling and CI job (1 day), scenarios 1–4 (1 day), 5–8 and flake hardening
(0.5–1 day).
