# Maintafox Desktop — Frontend Coding Standards

This document defines the mandatory conventions for all TypeScript and React code in the
Maintafox Desktop frontend. Every contributor must follow these rules. Automated tooling
(ESLint, Prettier, Biome) enforces most of them at commit time.

---

## 1. Naming Conventions

React components use **PascalCase**. The component name must describe the UI element it
renders and the domain it belongs to. Examples: `WorkOrderCard`, `EquipmentSearchBar`,
`InterventionRequestForm`, `SiteTreeNode`.

Functions and variables use **camelCase**. Names should be descriptive enough to understand
without reading the implementation. Examples: `fetchWorkOrders`, `isLoading`,
`computeNextMaintenanceDate`, `handleSubmit`.

Constants use **SCREAMING_SNAKE_CASE**. Constants are values that never change at runtime
and are defined at module scope. Examples: `MAX_OFFLINE_GRACE_DAYS`, `DEFAULT_LOCALE`,
`SYNC_RETRY_INTERVAL_MS`.

File names for pages, hooks, and services use **kebab-case** matching the export name.
A component named `WorkOrderCard` lives in `work-order-card.tsx`. A hook named
`useWorkOrder` lives in `use-work-order.ts`. A service named `workOrderService` lives in
`work-order-service.ts`.

IPC command names use **snake_case** matching the Rust command function name exactly. The
frontend calls `invoke("create_work_order", ...)` which maps to the Rust function
`pub async fn create_work_order(...)`. This convention is enforced by Tauri.

i18n keys use **dot-notation scoped by module**. Each key starts with the module name,
followed by the section, followed by the specific label. Examples:
`workOrder.status.inProgress`, `equipment.form.serialNumber`,
`auth.error.invalidCredentials`.

---

## 2. Component Structure Rules

All hooks must be declared at the top of the component function body, before any
conditional logic or early returns. React requires hooks to be called in the same order on
every render. Placing hooks after an `if` statement or after an early `return` violates
the Rules of Hooks and causes runtime errors.

No business logic may appear inside JSX expressions. Compute all values — formatted dates,
filtered lists, conditional class names — in named variables above the `return` statement.
JSX should read like a template, not like a program.

Use early returns for loading and error states before the main render block. If the data
is not yet available, return a loading spinner immediately. If an error occurred, return
an error message immediately. The main render block at the bottom of the function assumes
data is available and types are narrowed.

Never define a component inside another component's function body. Inner component
definitions are recreated on every render, destroying React's ability to reconcile the
DOM. Extract inner components into their own named exports in the same file or in a
separate file.

Components receive data through props or read from the Zustand store. They never call
IPC directly. All data fetching and mutation is handled by services called from hooks.
Components are pure renderers of state.

Maximum component file length is **250 lines**. When a component file exceeds this limit,
extract sub-components into separate files in the same directory. This keeps files
scannable and encourages composition.

---

## 3. Import Ordering

Imports are organized into four groups, separated by blank lines:

**Group 1 — Node built-ins.** Rarely needed in frontend code. Example: `import { resolve }
from "path"`.

**Group 2 — External packages.** Everything installed via pnpm that is not part of the
project. Examples: `import { useState } from "react"`, `import { z } from "zod"`,
`import { useTranslation } from "react-i18next"`.

**Group 3 — Internal aliased imports.** Imports using the `@/` or `@shared/` path aliases.
Examples: `import { cn } from "@/lib/utils"`, `import type { WorkOrder } from
"@shared/ipc-types"`.

**Group 4 — Relative imports.** Imports from sibling or parent files using `./` or `../`.
Examples: `import { StatusBadge } from "./status-badge"`.

One blank line separates each group. Within each group, imports are sorted alphabetically
by path. The ESLint `import/order` rule enforces this automatically and will block commits
that violate the ordering.

---

## 4. State Management

**Local component state** (`useState`): Use for UI-only state that no other component
needs. Examples: whether a dropdown is open, the current value of a text field before
submission, a toggled accordion panel.

**Derived state** (`useMemo`, `useCallback`): Use for values computed from props or store
data. Never store derived values in `useState` — compute them on render. `useMemo` avoids
expensive recomputations; `useCallback` stabilizes function references passed as props.

**Cross-component shared state**: Zustand stores living in `src/store/`. Each major domain
gets its own store file: `work-order-store.ts`, `equipment-store.ts`, `auth-store.ts`.
Stores hold normalized data, loading states, and actions that modify state.

**Server/IPC state**: Wrapped in custom hooks in `src/hooks/` that internally call
functions from `src/services/`. These hooks encapsulate the fetch-validate-store cycle
and expose clean return values to components.

**Critical rule**: No module may call `invoke()` directly from a component or a generic
hook. All IPC calls are isolated in `src/services/` modules. This creates a single layer
where IPC contracts are defined, typed, and validated with Zod.

---

## 5. Error Handling Patterns

IPC service functions return typed result unions — never raw `try/catch` blocks in
component code. Each service function catches IPC errors and returns a discriminated
result that components can pattern-match on. Components never handle raw exceptions.

Use **Zod schemas** to validate all data arriving from IPC before passing it to component
state. The Rust backend may evolve independently; Zod validation at the service boundary
catches shape mismatches early and produces clear error messages instead of cryptic
runtime failures deep in the component tree.

Display errors using the centralized `ErrorBoundary` component for unrecoverable errors
and toast notifications for recoverable ones. Never use `alert()` in production code.
Never expose raw console output to the user. All error presentation goes through the
design system's error components.

All error messages shown to the user must use the `t()` translation function from
`react-i18next`. No error string may be hard-coded in English or French. Error keys follow
the module-scoped pattern: `workOrder.error.createFailed`, `auth.error.sessionExpired`.

---

## 6. IPC Contract Rule

Every Tauri `invoke` call lives exclusively in a file under `src/services/`. No other
directory may import from `@tauri-apps/api/core`. This rule is non-negotiable — it is the
single most important architectural boundary in the frontend.

Service files are named after the domain they serve: `work-order-service.ts`,
`equipment-service.ts`, `auth-service.ts`, `structure-service.ts`. Each file exports
typed async functions that accept domain parameters and return validated results.

Service functions are typed end-to-end. Input types are defined in `shared/ipc-types.ts`
and shared with the Rust backend. Output types are validated with Zod schemas at the
service boundary before being returned to callers. This guarantees that the frontend
never operates on unvalidated data from the IPC bridge.

No component, store, or hook may import from `@tauri-apps/api/core` directly. If a new
IPC command is needed, add a function to the appropriate service file. If no service file
exists for that domain, create one following the naming convention.

---

## 7. i18n Rule

Every string visible to the user must be wrapped in `t("key")` using `react-i18next`.
This applies to labels, placeholders, error messages, tooltips, confirmation dialogs,
and any other user-facing text. There are no exceptions.

No French or English text may appear as a string literal in any `.tsx` or `.ts` file
outside of `src/i18n/`. String literals for CSS class names, IPC command names, and
internal identifiers are acceptable. User-visible text is not.

Missing translation keys must fail the build in production mode. The i18next configuration
includes a `missingKeyHandler` that throws an error in development, alerting developers
immediately when a key is used but not defined. In production builds, the handler logs
a warning but does not crash.

Module-scoped namespace files are preferred over a single large `common.json`. Each major
module gets its own namespace file added in its implementation sprint. The `common`
namespace holds only cross-cutting terms (app name, generic loading/error messages). Module
namespaces are named after their domain: `workOrder.json`, `equipment.json`, `auth.json`.
This keeps translation files manageable as the application grows.
