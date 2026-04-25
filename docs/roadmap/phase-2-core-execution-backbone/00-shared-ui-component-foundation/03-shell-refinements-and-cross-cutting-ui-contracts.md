# Phase 2 · Sub-phase 00 · File 03
# Shell Refinements and Cross-Cutting UI Contracts

## Context and Purpose

The Phase 1 shell (SP02-F03) delivered a functional layout: sidebar with grouped
navigation, top bar with search placeholder/notification bell/user avatar, and a
status bar. However, several interactive behaviors are missing:

1. **Role-scoped sidebar** — The sidebar currently renders all 27 navigation items
   unconditionally. The `PermissionGate` component and `usePermissions` hook were
   created in SP04-F03, but they are not wired to the sidebar. Users should only
   see modules they have permission to access.

2. **Notification bell backend** — The bell icon exists in TopBar with an unread
   count badge from `useAppStore`, but there is no notification delivery system.
   Phase 2 SP07 will build the notification model; this file wires the existing
   bell to the notification service's IPC contract.

3. **Search / command palette** — The `⌘K` surface in TopBar is a placeholder div.
   This file creates a basic command palette dialog that navigates to modules by
   name, replacing the placeholder with a functional search.

4. **User menu enhancements** — SP04-F05 (bridge file) added a basic logout dropdown.
   This file enhances it with role-based admin links, a session timer indicator,
   and the device trust status badge.

**Gap addressed:** Category E from the Phase 1 gap analysis — cross-cutting UI
mismatches between the shell and the functional backend.

## Architecture Rules Applied

- **Permission-filtered navigation.** The sidebar calls `usePermissions().can()`
  for each nav item's `requiredPermission` field. Items the user cannot access
  are hidden (not greyed out — hiding is the standard for CMMS UX per PRD §13).
- **Notification polling, not WebSocket.** Maintafox is offline-first. Notifications
  come from local events (DI state change, WO assignment, PM trigger) via the Rust
  event bus. The bell queries an IPC command, not a socket.
- **Command palette uses existing nav-registry.** The search indexes `defaultNavItems`
  from `nav-registry.tsx`. No separate searchable index is needed.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/layout/Sidebar.tsx` (patch) | Filter nav items by permission |
| `src/navigation/nav-registry.tsx` (patch) | Add `requiredPermission` field to NavItem |
| `src/components/shell/CommandPalette.tsx` | ⌘K command palette dialog |
| `src/components/layout/TopBar.tsx` (patch) | Wire command palette, enhance user menu |
| `src/hooks/use-notification-count.ts` | Polling hook for unread notification count |

## Prerequisites

- SP04-F05 complete: AuthGuard, login flow, TopBar user menu
- SP00-F01 complete: Shadcn Dialog component available
- SP04-F03 complete: `usePermissions` hook, `PermissionGate` component
- SP07-F01 (or stub): Notification count IPC command available

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Role-Scoped Sidebar and Nav Permission Filtering | Nav item permission field, sidebar filtering |
| S2 | Command Palette and Search Surface | `CommandPalette.tsx`, TopBar wiring |
| S3 | Notification Polling and User Menu Enhancements | Notification count hook, user menu upgrades |

---

## Sprint S1 — Role-Scoped Sidebar and Nav Permission Filtering

### AI Agent Prompt

```
You are a React and TypeScript engineer. The sidebar renders all 27 navigation items
unconditionally. The usePermissions hook (SP04-F03) provides a `can(permissionName)`
function. Your task is to add a permission field to nav items and filter the sidebar.

────────────────────────────────────────────────────────────────────
STEP 1 — PATCH src/navigation/nav-registry.tsx
────────────────────────────────────────────────────────────────────

Add an optional `requiredPermission` field to the `NavItem` interface:

```typescript
export interface NavItem {
  key: string;
  label: string;        // i18n key in "shell" namespace
  icon: ReactNode;
  path: string;
  group: string;
  /** Permission required to see this nav item. If undefined, always visible. */
  requiredPermission?: string;
}
```

Then add appropriate permission strings to each nav item. Examples:
- Equipment → `"mod.equipment.read"`
- Work Orders → `"mod.workorders.read"`
- Users → `"adm.users.manage"`
- Settings → `"adm.settings"`
- Dashboard, Profile → no permission (always visible)

The exact permission names should match the permissions seeded in SP04-F03.

────────────────────────────────────────────────────────────────────
STEP 2 — PATCH src/components/layout/Sidebar.tsx
────────────────────────────────────────────────────────────────────

Import usePermissions and filter the items array:

```typescript
import { usePermissions } from "@/hooks/use-permissions";

// Inside the Sidebar component:
const { can } = usePermissions();

const visibleItems = items.filter(
  (item) => !item.requiredPermission || can(item.requiredPermission),
);
```

Then render `visibleItems` instead of `items`.

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- Admin user sees all 27 nav items (admin has all permissions)
- A non-admin user sees only the items their role grants access to
- Dashboard and Profile are always visible regardless of role
- `pnpm run typecheck` passes
```

---

### Supervisor Verification — Sprint S1

**V1 — Admin sees all items.**
Log in as admin. Count sidebar items — all 27 should be visible.

**V2 — Non-admin sees filtered items.**
Create a test user with limited permissions. Log in. Confirm the sidebar only
shows permitted modules. Modules without permission should be completely absent
(not greyed out).

---

## Sprint S2 — Command Palette and Search Surface

### AI Agent Prompt

```
You are a React engineer. The TopBar has a ⌘K placeholder. Create a command palette
dialog that lets users search and navigate to modules by name.

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE src/components/shell/CommandPalette.tsx
────────────────────────────────────────────────────────────────────

Create a dialog-based command palette that:
- Opens on ⌘K (Mac) / Ctrl+K (Windows)
- Shows a search input at the top
- Lists matching nav items from `defaultNavItems` filtered by the search query
- Navigates to the selected item's path and closes the dialog
- Is bilingual: uses `useTranslation("shell")` for placeholder text
- Filters by the current user's permissions (same as sidebar)
- Uses the Shadcn Dialog component

────────────────────────────────────────────────────────────────────
STEP 2 — PATCH src/components/layout/TopBar.tsx
────────────────────────────────────────────────────────────────────

Replace the static ⌘K placeholder div with an onClick handler that opens
the CommandPalette:

```tsx
<div
  onClick={() => setCommandPaletteOpen(true)}
  className="flex items-center gap-2 rounded-md border ..."
>
  <span>⌘K</span>
  <span>{t("search.placeholder")}</span>
</div>
<CommandPalette
  open={commandPaletteOpen}
  onOpenChange={setCommandPaletteOpen}
/>
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- Pressing Ctrl+K opens the command palette
- Typing "equip" filters to show "Équipements" (in French)
- Selecting an item navigates to its route and closes the palette
- `pnpm run typecheck` passes
```

---

### Supervisor Verification — Sprint S2

**V1 — Command palette opens.**
Run the app. Press Ctrl+K. A search dialog should appear.

**V2 — Navigation works.**
Type "equip" in the palette. Select the Equipment item. Confirm the app navigates
to `/equipment`.

---

## Sprint S3 — Notification Polling and User Menu Enhancements

### AI Agent Prompt

```
You are a React engineer. Wire the notification bell to a polling hook and enhance
the user menu with session information.

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE src/hooks/use-notification-count.ts
────────────────────────────────────────────────────────────────────

Create a hook that polls `get_unread_notification_count` IPC command every 30 seconds.
If the IPC command is not yet implemented (Phase 2 SP07), the hook should catch the
error silently and return 0. This allows the hook to exist now without blocking on
the notification backend.

────────────────────────────────────────────────────────────────────
STEP 2 — PATCH src/components/layout/TopBar.tsx
────────────────────────────────────────────────────────────────────

Replace the hardcoded `useAppStore((s) => s.unreadNotificationCount)` with the
new `useNotificationCount()` hook. Add to the user menu:
- A "Session active" indicator with time remaining
- Device trust status badge (trusted / untrusted)

These pull from `useSession().info` and `getDeviceTrustStatus()`.

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- Notification bell shows 0 when the notification backend is not yet available
- User menu shows session expiry information
- `pnpm run typecheck` passes
```

---

### Supervisor Verification — Sprint S3

**V1 — No console errors from notification polling.**
Open DevTools Console. Confirm no error spam from the notification count hook.
The hook should catch IPC errors silently.

**V2 — User menu shows session info.**
Click the user avatar. The dropdown should show session-related information
(display name, session timer or device trust badge).

**V3 — Full regression check.**
Run `pnpm test && pnpm run typecheck && pnpm run i18n:check`. All pass.

---

*End of Phase 2 · Sub-phase 00 · File 03*
