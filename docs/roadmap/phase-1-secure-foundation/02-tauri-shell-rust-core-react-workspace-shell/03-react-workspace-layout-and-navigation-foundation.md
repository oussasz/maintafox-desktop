# Phase 1 · Sub-phase 02 · File 03
# React Workspace Layout and Navigation Foundation

## Context and Purpose

This file builds the complete React shell that all 26 functional modules will live inside.
Every module sprint in Phase 2 onward drops its page component into a slot in the layout
established here. Getting this right now prevents a rewrite later.

The shell has four permanent regions defined by PRD §13.2:

1. **Top bar** — search/command surface, sync badge, notification badge, user menu
2. **Navigation sidebar** — role-scoped module links, collapsible groups, active state
3. **Content area** — module header, action row, filter row, primary working surface
4. **Status bar** — offline/online state, pending sync count, DB health, app version

The React router, global Zustand store, startup event bridge, and i18n provider are all
wired here in a way that is visible and testable before a single functional module exists.

## Prerequisites

- Sub-phase 01 fully complete (scaffold, CI, IPC contracts, coding standards)
- File 01 of this sub-phase complete (`startup.rs`, `lib.rs`, `StartupEvent` emitted)
- File 02 of this sub-phase complete (`AppState`, `get_app_info`, `get_startup_state` IPC
  commands, background task supervisor)

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Design Tokens, Theme Provider, and Global CSS | CSS custom properties, Tailwind token overrides, ThemeProvider component, typography |
| S2 | App Shell Layout — TopBar, Sidebar, StatusBar | Three layout components, CSS grid shell, live startup-event bridge |
| S3 | Router, Navigation Model, and Placeholder Pages | React Router v6, nav registry, lazy-loaded placeholder pages for all 26 modules |

---

## Sprint S1 — Design Tokens, Theme Provider, and Global CSS

### AI Agent Prompt

```
You are a senior React and TypeScript engineer working on Maintafox Desktop (Tauri 2.x).
The monorepo scaffold from Sub-phase 01 is in place. Your task is to implement the
complete Maintafox design token system, Tailwind configuration update, global CSS, and
ThemeProvider component that every subsequent module sprint will consume.

PRD REFERENCES (read carefully before coding):
  §13.1 — Visual Identity and Product Feel
  §13.5 — Accessibility and Industrial Usability

────────────────────────────────────────────────────────────────────
STEP 1 — Update tailwind.config.ts
────────────────────────────────────────────────────────────────────
Replace the placeholder tailwind.config.ts with the complete token system:

```typescript
// tailwind.config.ts
import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Primary brand palette (PRD §13.1)
        primary: {
          DEFAULT: "#003d8f",
          dark:    "#002b6a",
          light:   "#4d7bc5",
          bg:      "#e8eef8",
        },
        // Accent / warning surface
        accent: {
          DEFAULT: "#f0a500",
          dark:    "#c47f00",
        },
        // Semantic status palette (used by badges, alerts, state markers)
        status: {
          success: "#198754",
          danger:  "#dc3545",
          warning: "#ffc107",
          info:    "#0dcaf0",
          neutral: "#6c757d",
        },
        // Surface and border tokens for the industrial dark workspace
        surface: {
          0:    "#0f172a",   // deepest background (window chrome, status bar)
          1:    "#1e293b",   // sidebar, top bar
          2:    "#263244",   // card, panel, table header
          3:    "#334155",   // hover states, row highlights
          border: "#2d3f55",
        },
        // Text hierarchy
        text: {
          primary:   "#f1f5f9",
          secondary: "#94a3b8",
          muted:     "#64748b",
          danger:    "#f87171",
          success:   "#4ade80",
          warning:   "#fbbf24",
        },
      },
      fontFamily: {
        sans: [
          "Inter",
          "ui-sans-serif",
          "system-ui",
          "-apple-system",
          "BlinkMacSystemFont",
          '"Segoe UI"',
          "Roboto",
          "sans-serif",
        ],
        mono: [
          '"JetBrains Mono"',
          '"Fira Code"',
          "ui-monospace",
          "monospace",
        ],
      },
      fontSize: {
        "2xs": ["0.625rem", { lineHeight: "1rem" }],
        xs:    ["0.75rem",  { lineHeight: "1rem" }],
        sm:    ["0.8125rem",{ lineHeight: "1.25rem" }],
        base:  ["0.875rem", { lineHeight: "1.5rem" }],   // 14px base
        lg:    ["1rem",     { lineHeight: "1.5rem" }],
        xl:    ["1.125rem", { lineHeight: "1.75rem" }],
        "2xl": ["1.25rem",  { lineHeight: "1.75rem" }],
      },
      spacing: {
        sidebar:        "240px",
        "sidebar-sm":   "64px",
        topbar:         "52px",
        statusbar:      "24px",
      },
      animation: {
        "fade-in":  "fadeIn 120ms ease-in",
        "slide-in": "slideIn 150ms ease-out",
        "spin-slow":"spin 2s linear infinite",
      },
      keyframes: {
        fadeIn:  { from: { opacity: "0" }, to: { opacity: "1" } },
        slideIn: { from: { transform: "translateX(-8px)", opacity: "0" }, to: { transform: "translateX(0)", opacity: "1" } },
      },
      borderRadius: {
        sm:   "4px",
        md:   "6px",
        lg:   "8px",
        xl:   "12px",
      },
      boxShadow: {
        card:  "0 1px 3px 0 rgba(0,0,0,0.4)",
        panel: "0 4px 16px 0 rgba(0,0,0,0.5)",
        focus: "0 0 0 2px #4d7bc5",
      },
      transitionDuration: {
        fast:   "100ms",
        base:   "150ms",
        normal: "200ms",
        slow:   "300ms",
      },
    },
  },
  plugins: [
    require("@tailwindcss/forms"),
    require("@tailwindcss/typography"),
  ],
} satisfies Config;
```

────────────────────────────────────────────────────────────────────
STEP 2 — Update src/styles/globals.css
────────────────────────────────────────────────────────────────────
Replace the placeholder globals.css with the full global stylesheet:

```css
/* src/styles/globals.css */
@tailwind base;
@tailwind components;
@tailwind utilities;

/* ── CSS Custom Properties (design tokens, referenced by JS where Tailwind is
       insufficient, e.g. D3 chart rendering) ───────────────────────────── */
:root {
  /* Brand */
  --color-primary:       #003d8f;
  --color-primary-dark:  #002b6a;
  --color-primary-light: #4d7bc5;
  --color-primary-bg:    #e8eef8;
  --color-accent:        #f0a500;

  /* Status */
  --color-success:  #198754;
  --color-danger:   #dc3545;
  --color-warning:  #ffc107;
  --color-info:     #0dcaf0;

  /* Surface */
  --surface-0:      #0f172a;
  --surface-1:      #1e293b;
  --surface-2:      #263244;
  --surface-3:      #334155;
  --surface-border: #2d3f55;

  /* Text */
  --text-primary:   #f1f5f9;
  --text-secondary: #94a3b8;
  --text-muted:     #64748b;

  /* Layout */
  --sidebar-width:    240px;
  --sidebar-width-sm: 64px;
  --topbar-height:    52px;
  --statusbar-height: 24px;

  /* Transitions */
  --transition-fast:   100ms;
  --transition-base:   150ms;
  --transition-normal: 200ms;
  --transition-slow:   300ms;
}

@layer base {
  *, *::before, *::after {
    box-sizing: border-box;
  }

  html, body, #root {
    height: 100%;
    width: 100%;
    overflow: hidden;
  }

  body {
    font-family: Inter, ui-sans-serif, system-ui, -apple-system,
                 BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    font-size: 14px;
    line-height: 1.5;
    background-color: var(--surface-0);
    color: var(--text-primary);
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
    /* Disable browser text selection on UI chrome (not on content) */
    user-select: none;
  }

  /* Re-enable selection inside text content areas */
  p, span, td, th, li, .selectable {
    user-select: text;
  }

  /* Remove browser default focus ring; use custom focus-visible only */
  :focus {
    outline: none;
  }
  :focus-visible {
    outline: 2px solid var(--color-primary-light);
    outline-offset: 2px;
  }

  /* Scrollbar styling — industrial dark theme */
  ::-webkit-scrollbar { width: 6px; height: 6px; }
  ::-webkit-scrollbar-track { background: var(--surface-1); }
  ::-webkit-scrollbar-thumb {
    background: var(--surface-3);
    border-radius: 3px;
  }
  ::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }

  /* Tauri drag region for title-bar-less windows */
  [data-tauri-drag-region] {
    -webkit-app-region: drag;
    cursor: default;
  }
  [data-tauri-drag-region] * {
    -webkit-app-region: no-drag;
  }
}

@layer components {
  /* ── Status badge base ───────────────────────────────────────────── */
  .badge {
    @apply inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium;
  }
  .badge-success { @apply bg-status-success/20 text-status-success; }
  .badge-danger  { @apply bg-status-danger/20  text-status-danger; }
  .badge-warning { @apply bg-status-warning/20 text-text-warning; }
  .badge-info    { @apply bg-status-info/20    text-status-info; }
  .badge-neutral { @apply bg-surface-3         text-text-secondary; }

  /* ── Button primitives (extended by Shadcn variants) ─────────────── */
  .btn-primary {
    @apply inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5
           text-sm font-medium text-white transition-colors duration-fast
           hover:bg-primary-dark focus-visible:ring-2 focus-visible:ring-primary-light
           disabled:pointer-events-none disabled:opacity-50;
  }
  .btn-ghost {
    @apply inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm
           font-medium text-text-secondary transition-colors duration-fast
           hover:bg-surface-3 hover:text-text-primary disabled:opacity-50;
  }
  .btn-danger {
    @apply inline-flex items-center gap-1.5 rounded-md bg-status-danger px-3 py-1.5
           text-sm font-medium text-white transition-colors duration-fast
           hover:brightness-90 disabled:opacity-50;
  }

  /* ── Form field primitives ───────────────────────────────────────── */
  .field-label {
    @apply block text-xs font-medium text-text-secondary uppercase tracking-wide mb-1;
  }
  .field-input {
    @apply w-full rounded-md border border-surface-border bg-surface-2 px-3 py-1.5
           text-sm text-text-primary placeholder-text-muted
           focus-visible:border-primary-light focus-visible:ring-1
           focus-visible:ring-primary-light transition-colors duration-fast;
  }
  .field-error {
    @apply mt-1 text-xs text-text-danger;
  }

  /* ── Table primitives ───────────────────────────────────────────── */
  .tbl-header {
    @apply bg-surface-2 text-xs font-semibold uppercase tracking-wide
           text-text-secondary;
  }
  .tbl-row {
    @apply border-b border-surface-border transition-colors duration-fast
           hover:bg-surface-3;
  }
  .tbl-cell {
    @apply px-3 py-2 text-sm text-text-primary;
  }
}
```

────────────────────────────────────────────────────────────────────
STEP 3 — Create src/components/ui/ThemeProvider.tsx
────────────────────────────────────────────────────────────────────
```tsx
// src/components/ui/ThemeProvider.tsx
import type { ReactNode } from "react";
import { createContext, useContext, useEffect, useState } from "react";

type Theme = "dark" | "light";

interface ThemeContextValue {
  theme: Theme;
  setTheme: (t: Theme) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => {
    const saved = localStorage.getItem("maintafox:theme");
    return saved === "light" ? "light" : "dark";
  });

  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove("light", "dark");
    root.classList.add(theme);
    localStorage.setItem("maintafox:theme", theme);
  }, [theme]);

  function setTheme(t: Theme) {
    setThemeState(t);
  }

  return (
    <ThemeContext.Provider value={{ theme, setTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used inside ThemeProvider");
  return ctx;
}
```

────────────────────────────────────────────────────────────────────
STEP 4 — Add Inter font loading to public/ and index.html
────────────────────────────────────────────────────────────────────
Inter must NOT be loaded from Google Fonts (CDN blocked by CSP and offline requirement).
Add the Inter variable font to public/fonts/ and reference it in globals.css.

Create the @font-face declaration AS THE FIRST RULE in globals.css (before @tailwind):
```css
/* Self-hosted Inter variable font — no CDN; offline-safe */
@font-face {
  font-family: "Inter";
  font-style: normal;
  font-weight: 100 900;
  font-display: swap;
  src: url("/fonts/inter-variable.woff2") format("woff2");
}
```

Create public/fonts/ directory with a .gitkeep placeholder and add a note in
docs/DEV_ENVIRONMENT.md (Section 4) that the Inter woff2 file must be downloaded from
https://fonts.google.com/specimen/Inter and placed at public/fonts/inter-variable.woff2.

The application falls back to system fonts gracefully if inter-variable.woff2 is absent —
the @font-face declaration uses font-display: swap and the Tailwind font-family stack
has full system fallbacks. The build must not fail if the file is absent.

────────────────────────────────────────────────────────────────────
STEP 5 — Update src/types/index.ts
────────────────────────────────────────────────────────────────────
```typescript
// src/types/index.ts

/** Application-level status values used in the UI */
export type AppStatus = "loading" | "ready" | "error" | "offline";

/** Semantic status badge variants */
export type StatusVariant = "success" | "danger" | "warning" | "info" | "neutral";

/** Module identifier keys — one per PRD §6 module */
export type ModuleKey =
  | "dashboard"
  | "auth"
  | "org"
  | "equipment"
  | "requests"
  | "work-orders"
  | "personnel"
  | "users"
  | "inventory"
  | "pm"
  | "reliability"
  | "analytics"
  | "archive"
  | "lookups"
  | "notifications"
  | "documentation"
  | "planning"
  | "activity"
  | "settings"
  | "profile"
  | "training"
  | "iot"
  | "erp"
  | "permits"
  | "budget"
  | "inspections"
  | "configuration";

export type Theme = "dark" | "light";
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- pnpm run typecheck passes with 0 errors
- pnpm run lint:check passes with 0 errors
- The application window background is #0f172a (deep navy) when pnpm run dev is running
- ThemeProvider wraps the application in main.tsx
- All CSS custom properties (--color-primary, --surface-0, etc.) are present in globals.css
- src/types/index.ts exports ModuleKey covering all 27 route keys
```

---

### Supervisor Verification — Sprint S1

**V1 — Application background color is dark navy.**
Run `pnpm run dev`. The Tauri window should open with a dark background — close to black
with a slight blue tone, not pure white or grey. If the window appears with a white or
grey background, the CSS is not loaded. Flag it.

**V2 — Tooling passes.**
In the terminal, run:
```
pnpm run typecheck
```
It should complete with no `error TS` lines. If errors appear, copy the first 5 lines and
flag them.

**V3 — Design token CSS file is complete.**
Open `src/styles/globals.css`. Scroll through it and confirm you can see lines beginning
with `--color-primary`, `--surface-0`, `--topbar-height`, and `--sidebar-width`. These are
the CSS variables used by every future module. If the file is less than 80 lines, or these
variable names are absent, flag it.

**V4 — Font fallback works offline.**
Disconnect from the internet. Restart the application with `pnpm run dev`. The text should
still be readable (system font fallback). If the window shows no text at all or an error
about fonts, flag it.

---

## Sprint S2 — App Shell Layout: TopBar, Sidebar, StatusBar

### AI Agent Prompt

```
You are a senior React and TypeScript engineer continuing work on Maintafox Desktop.
Sprint S1 is complete: design tokens, ThemeProvider, globals.css, and type exports are
all in place.

YOUR TASK: Implement the three permanent layout chrome components that surround every
module in the application: TopBar, Sidebar, and StatusBar. Additionally, wire the
Tauri StartupEvent bridge so the frontend transitions from "Loading" to "Ready" based on
what the Rust core emits.

PRD REFERENCES:
  §13.2 — Desktop Workspace Model (top bar, sidebar, status bar rules)
  §13.3 — Component and Workflow Conventions
  §4.2  — Tauri startup event integration (@tauri-apps/api/event)

────────────────────────────────────────────────────────────────────
STEP 1 — Create src/store/app-store.ts
────────────────────────────────────────────────────────────────────
The global Zustand store holds the application-level state: startup phase, sync status,
notification count, session stub, sidebar collapse state.

```typescript
// src/store/app-store.ts
import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { AppStatus } from "@/types";

export interface SyncStatus {
  state: "idle" | "syncing" | "pending" | "error";
  pendingCount: number;
  lastSyncAt: string | null;
  errorMessage: string | null;
}

export interface AppStore {
  // Startup state
  appStatus: AppStatus;
  startupMessage: string;
  appVersion: string;

  // Sync and connectivity
  isOnline: boolean;
  syncStatus: SyncStatus;

  // Notification badge
  unreadNotificationCount: number;

  // Session (Phase 1 stub — replaced in Sub-phase 04)
  hasActiveSession: boolean;
  currentUserDisplayName: string | null;

  // UI: sidebar
  sidebarCollapsed: boolean;
  activePath: string;

  // Actions
  setAppStatus: (status: AppStatus, message?: string) => void;
  setAppVersion: (version: string) => void;
  setOnline: (online: boolean) => void;
  setSyncStatus: (s: Partial<SyncStatus>) => void;
  setUnreadNotificationCount: (n: number) => void;
  setSessionStub: (hasSession: boolean, displayName: string | null) => void;
  toggleSidebar: () => void;
  setActivePath: (path: string) => void;
}

export const useAppStore = create<AppStore>()(
  persist(
    (set) => ({
      appStatus: "loading",
      startupMessage: "",
      appVersion: "",
      isOnline: true,
      syncStatus: {
        state: "idle",
        pendingCount: 0,
        lastSyncAt: null,
        errorMessage: null,
      },
      unreadNotificationCount: 0,
      hasActiveSession: false,
      currentUserDisplayName: null,
      sidebarCollapsed: false,
      activePath: "/",

      setAppStatus: (appStatus, startupMessage = "") =>
        set({ appStatus, startupMessage }),
      setAppVersion: (appVersion) => set({ appVersion }),
      setOnline: (isOnline) => set({ isOnline }),
      setSyncStatus: (s) =>
        set((st) => ({ syncStatus: { ...st.syncStatus, ...s } })),
      setUnreadNotificationCount: (unreadNotificationCount) =>
        set({ unreadNotificationCount }),
      setSessionStub: (hasActiveSession, currentUserDisplayName) =>
        set({ hasActiveSession, currentUserDisplayName }),
      toggleSidebar: () =>
        set((st) => ({ sidebarCollapsed: !st.sidebarCollapsed })),
      setActivePath: (activePath) => set({ activePath }),
    }),
    {
      name: "maintafox-app",
      // Only persist UI preferences, not runtime state
      partialize: (s) => ({
        sidebarCollapsed: s.sidebarCollapsed,
      }),
    },
  ),
);
```

────────────────────────────────────────────────────────────────────
STEP 2 — Create src/hooks/use-startup-bridge.ts
────────────────────────────────────────────────────────────────────
This hook subscribes to the `startup_event` Tauri event emitted by `startup.rs` and
transitions the app store from "loading" → "ready" or "error".

```typescript
// src/hooks/use-startup-bridge.ts
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "@/store/app-store";
import type { StartupEvent } from "@shared/ipc-types";

export function useStartupBridge(): void {
  const setAppStatus = useAppStore((s) => s.setAppStatus);
  const setAppVersion = useAppStore((s) => s.setAppVersion);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    void listen<StartupEvent>("startup_event", (event) => {
      const payload = event.payload;
      switch (payload.phase) {
        case "db_ready":
          setAppStatus("loading", payload.message ?? "Base de données prête");
          break;
        case "migrations_complete":
          setAppStatus("loading", payload.message ?? "Migrations appliquées");
          break;
        case "config_loaded":
          setAppStatus("loading", payload.message ?? "Configuration chargée");
          break;
        case "ready":
          if (payload.version) setAppVersion(payload.version);
          setAppStatus("ready");
          break;
        case "error":
          setAppStatus("error", payload.message ?? "Erreur de démarrage");
          break;
        default:
          break;
      }
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, [setAppStatus, setAppVersion]);
}
```

Also add `StartupEvent` to `shared/ipc-types.ts`:
```typescript
export interface StartupEvent {
  phase:
    | "db_ready"
    | "migrations_complete"
    | "config_loaded"
    | "ready"
    | "error";
  message?: string;
  version?: string;
}
```

────────────────────────────────────────────────────────────────────
STEP 3 — Create src/components/layout/TopBar.tsx
────────────────────────────────────────────────────────────────────
The top bar is fixed, full-width, 52px tall. It contains:
- Left: hamburger/toggle for sidebar collapse
- Center: placeholder search trigger (Phase 2 populates this)
- Right: sync badge, notification badge (with count), user menu trigger

```tsx
// src/components/layout/TopBar.tsx
import { Menu, Bell, RefreshCw, AlertCircle, User } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/store/app-store";
import { cn } from "@/lib/utils";

export function TopBar() {
  const { t } = useTranslation("shell");
  const toggleSidebar     = useAppStore((s) => s.toggleSidebar);
  const syncStatus        = useAppStore((s) => s.syncStatus);
  const unreadCount       = useAppStore((s) => s.unreadNotificationCount);
  const isOnline          = useAppStore((s) => s.isOnline);
  const displayName       = useAppStore((s) => s.currentUserDisplayName);

  return (
    <header
      className="fixed inset-x-0 top-0 z-30 flex h-topbar items-center
                 border-b border-surface-border bg-surface-1 px-3 gap-2"
      data-tauri-drag-region
    >
      {/* Sidebar toggle */}
      <button
        onClick={toggleSidebar}
        aria-label={t("sidebar.toggle")}
        className="btn-ghost px-2 py-1.5"
      >
        <Menu className="h-4 w-4" />
      </button>

      {/* Logo / product name */}
      <span className="text-sm font-semibold text-text-primary select-none mr-4">
        Maintafox
      </span>

      {/* Search placeholder — populated in Phase 2 */}
      <div className="flex-1 hidden md:flex items-center">
        <div
          className="flex items-center gap-2 rounded-md border border-surface-border
                     bg-surface-2 px-3 py-1 text-sm text-text-muted cursor-pointer
                     hover:border-primary-light transition-colors duration-fast w-72"
        >
          <span>⌘K</span>
          <span>{t("search.placeholder")}</span>
        </div>
      </div>

      {/* Right controls */}
      <div className="ml-auto flex items-center gap-1">

        {/* Sync status indicator */}
        <SyncIndicator state={syncStatus.state} isOnline={isOnline} />

        {/* Notification bell */}
        <button
          aria-label={t("notifications.label", { count: unreadCount })}
          className="relative btn-ghost px-2 py-1.5"
        >
          <Bell className="h-4 w-4" />
          {unreadCount > 0 && (
            <span className="absolute -right-0.5 -top-0.5 flex h-4 w-4 items-center
                             justify-center rounded-full bg-status-danger
                             text-2xs font-bold text-white">
              {unreadCount > 99 ? "99+" : unreadCount}
            </span>
          )}
        </button>

        {/* User menu trigger */}
        <button
          aria-label={displayName ?? t("user.menu")}
          className="btn-ghost flex items-center gap-2 px-2 py-1.5"
        >
          <div className="flex h-6 w-6 items-center justify-center
                          rounded-full bg-primary text-xs font-semibold text-white">
            {displayName ? displayName.charAt(0).toUpperCase() : (
              <User className="h-3.5 w-3.5" />
            )}
          </div>
          {displayName && (
            <span className="hidden lg:inline text-sm text-text-secondary max-w-32 truncate">
              {displayName}
            </span>
          )}
        </button>
      </div>
    </header>
  );
}

function SyncIndicator({
  state,
  isOnline,
}: {
  state: string;
  isOnline: boolean;
}) {
  const { t } = useTranslation("shell");

  if (!isOnline) {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-warning/10 text-status-warning"
        title={t("sync.offline")}
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">{t("sync.offline")}</span>
      </span>
    );
  }

  if (state === "syncing") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   text-text-secondary"
        title={t("sync.syncing")}
      >
        <RefreshCw className="h-3.5 w-3.5 animate-spin-slow" />
        <span className="hidden sm:inline">{t("sync.syncing")}</span>
      </span>
    );
  }

  if (state === "error") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-danger/10 text-status-danger"
        title={t("sync.error")}
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">{t("sync.error")}</span>
      </span>
    );
  }

  return null; // idle — no indicator needed
}
```

────────────────────────────────────────────────────────────────────
STEP 4 — Create src/components/layout/StatusBar.tsx
────────────────────────────────────────────────────────────────────
The status bar is fixed to the bottom, 24px tall, full-width. It shows:
- Left: connectivity badge (Online / Offline)
- Center: pending sync count (if > 0)
- Right: database health indicator, application version

```tsx
// src/components/layout/StatusBar.tsx
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/store/app-store";
import { cn } from "@/lib/utils";

export function StatusBar() {
  const { t }       = useTranslation("shell");
  const isOnline    = useAppStore((s) => s.isOnline);
  const syncStatus  = useAppStore((s) => s.syncStatus);
  const appVersion  = useAppStore((s) => s.appVersion);

  return (
    <footer
      className="fixed inset-x-0 bottom-0 z-30 flex h-statusbar items-center
                 justify-between border-t border-surface-border bg-surface-0
                 px-3 text-2xs text-text-muted select-none"
    >
      {/* Left: connectivity */}
      <div className="flex items-center gap-3">
        <span className="flex items-center gap-1">
          <span
            className={cn(
              "inline-block h-1.5 w-1.5 rounded-full",
              isOnline ? "bg-status-success" : "bg-status-warning",
            )}
          />
          {isOnline ? t("status.online") : t("status.offline")}
        </span>

        {syncStatus.pendingCount > 0 && (
          <span className="text-status-warning">
            {t("status.pendingSync", { count: syncStatus.pendingCount })}
          </span>
        )}
      </div>

      {/* Right: db health + version */}
      <div className="flex items-center gap-3">
        <span title={t("status.dbHealthy")}>
          <span className="inline-block h-1.5 w-1.5 rounded-full bg-status-success mr-1" />
          {t("status.db")}
        </span>
        {appVersion && (
          <span>v{appVersion}</span>
        )}
      </div>
    </footer>
  );
}
```

────────────────────────────────────────────────────────────────────
STEP 5 — Create src/components/layout/Sidebar.tsx
────────────────────────────────────────────────────────────────────
The sidebar is fixed left, runs from top-bar to (above) status-bar.
It is collapsible. In expanded state (240px) it shows icon + label.
In collapsed state (64px) it shows icon + tooltip.

Navigation items are grouped: Core Operations, Planning, Compliance, Inventory,
Analytics & Reporting, Administration. The nav registry lives in Sprint S3.
For now, the Sidebar accepts a `navItems` prop.

```tsx
// src/components/layout/Sidebar.tsx
import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/store/app-store";
import { cn } from "@/lib/utils";

export interface NavItem {
  key: string;
  labelKey: string;   // i18n key in "shell" namespace
  path: string;
  icon: ReactNode;
  groupKey?: string;  // group header i18n key
  isGroupHeader?: boolean;
}

interface SidebarProps {
  items: NavItem[];
}

export function Sidebar({ items }: SidebarProps) {
  const { t }           = useTranslation("shell");
  const collapsed       = useAppStore((s) => s.sidebarCollapsed);
  const activePath      = useAppStore((s) => s.activePath);
  const setActivePath   = useAppStore((s) => s.setActivePath);

  // Group items by groupKey, preserving order
  type Group = { header: NavItem | null; children: NavItem[] };
  const groups = items.reduce<Group[]>((acc, item) => {
    if (item.isGroupHeader) {
      acc.push({ header: item, children: [] });
    } else {
      const lastGroup = acc[acc.length - 1];
      if (lastGroup) lastGroup.children.push(item);
      else acc.push({ header: null, children: [item] });
    }
    return acc;
  }, []);

  return (
    <nav
      className={cn(
        "fixed left-0 top-topbar bottom-statusbar z-20 flex flex-col",
        "border-r border-surface-border bg-surface-1",
        "overflow-y-auto overflow-x-hidden transition-all duration-normal",
        collapsed ? "w-sidebar-sm" : "w-sidebar",
      )}
    >
      <div className="flex flex-col gap-0.5 py-2 px-1.5">
        {groups.map((group, gi) => (
          <div key={gi}>
            {/* Group header — hidden when collapsed */}
            {group.header && !collapsed && (
              <div className="px-2 pt-3 pb-1 text-2xs font-semibold uppercase
                              tracking-wider text-text-muted">
                {t(group.header.labelKey)}
              </div>
            )}
            {/* Nav items */}
            {group.children.map((item) => {
              const isActive = activePath === item.path;
              return (
                <Link
                  key={item.key}
                  to={item.path}
                  title={collapsed ? t(item.labelKey) : undefined}
                  onClick={() => setActivePath(item.path)}
                  className={cn(
                    "flex items-center gap-2.5 rounded-md px-2 py-1.5",
                    "text-sm transition-colors duration-fast",
                    "hover:bg-surface-3",
                    isActive
                      ? "bg-primary-bg/10 text-primary-light font-medium"
                      : "text-text-secondary",
                    collapsed && "justify-center",
                  )}
                >
                  <span className="h-4 w-4 shrink-0">{item.icon}</span>
                  {!collapsed && (
                    <span className="truncate">{t(item.labelKey)}</span>
                  )}
                </Link>
              );
            })}
          </div>
        ))}
      </div>
    </nav>
  );
}
```

────────────────────────────────────────────────────────────────────
STEP 6 — Create src/components/layout/AppShell.tsx
────────────────────────────────────────────────────────────────────
The shell composes TopBar, Sidebar, and StatusBar into the grid layout.
It also renders the startup loading screen until `appStatus === "ready"`.

```tsx
// src/components/layout/AppShell.tsx
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/store/app-store";
import { useStartupBridge } from "@/hooks/use-startup-bridge";
import { TopBar } from "./TopBar";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { defaultNavItems } from "@/navigation/nav-registry";
import { cn } from "@/lib/utils";

interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps) {
  const { t }       = useTranslation("shell");
  const appStatus   = useAppStore((s) => s.appStatus);
  const startupMsg  = useAppStore((s) => s.startupMessage);
  const collapsed   = useAppStore((s) => s.sidebarCollapsed);

  // Bridge Tauri startup events → app store
  useStartupBridge();

  if (appStatus === "loading") {
    return (
      <div className="flex h-screen flex-col items-center justify-center
                      bg-surface-0 gap-4">
        <div className="h-8 w-8 animate-spin rounded-full border-2
                        border-surface-3 border-t-primary" />
        <p className="text-sm text-text-secondary">
          {startupMsg || t("startup.loading")}
        </p>
      </div>
    );
  }

  if (appStatus === "error") {
    return (
      <div className="flex h-screen flex-col items-center justify-center
                      bg-surface-0 gap-4 px-8 text-center">
        <p className="text-lg font-semibold text-text-danger">
          {t("startup.errorTitle")}
        </p>
        <p className="text-sm text-text-secondary max-w-md">{startupMsg}</p>
        <button
          className="btn-primary mt-2"
          onClick={() => window.location.reload()}
        >
          {t("startup.retry")}
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-screen flex-col bg-surface-0">
      <TopBar />
      <div
        className={cn(
          "flex flex-1 overflow-hidden",
          "pt-topbar pb-statusbar",
        )}
      >
        <Sidebar items={defaultNavItems} />
        <main
          className={cn(
            "flex-1 overflow-auto transition-all duration-normal",
            collapsed ? "ml-sidebar-sm" : "ml-sidebar",
          )}
        >
          {children}
        </main>
      </div>
      <StatusBar />
    </div>
  );
}
```

────────────────────────────────────────────────────────────────────
STEP 7 — Add i18n namespace: src/i18n/fr/shell.json and en/shell.json
────────────────────────────────────────────────────────────────────
```json
// src/i18n/fr/shell.json
{
  "sidebar": { "toggle": "Afficher/masquer le menu" },
  "search":  { "placeholder": "Rechercher ou ouvrir un module…" },
  "notifications": { "label": "{{count}} notifications non lues" },
  "user":    { "menu": "Menu utilisateur" },
  "sync": {
    "offline": "Hors ligne",
    "syncing": "Synchronisation…",
    "error":   "Erreur de sync"
  },
  "status": {
    "online":      "En ligne",
    "offline":     "Hors ligne",
    "db":          "BDD",
    "dbHealthy":   "Base de données opérationnelle",
    "pendingSync": "{{count}} en attente"
  },
  "startup": {
    "loading":     "Démarrage en cours…",
    "errorTitle":  "Erreur de démarrage",
    "retry":       "Réessayer"
  },
  "nav": {
    "groups": {
      "core":          "Opérations",
      "planning":      "Planification",
      "compliance":    "Conformité",
      "inventory":     "Stocks",
      "analytics":     "Analytique",
      "admin":         "Administration"
    },
    "dashboard":     "Tableau de bord",
    "equipment":     "Équipements",
    "requests":      "Demandes d'intervention",
    "workOrders":    "Ordres de travail",
    "planning":      "Planification",
    "pm":            "Maintenance préventive",
    "permits":       "Permis de travail",
    "inspections":   "Rondes et checklists",
    "inventory":     "Pièces de rechange",
    "personnel":     "Personnel",
    "training":      "Formations & habilitations",
    "analytics":     "Tableaux de bord",
    "reliability":   "Fiabilité (RAMS)",
    "budget":        "Budget & coûts",
    "archive":       "Archivage",
    "lookups":       "Données de référence",
    "notifications": "Notifications",
    "documentation": "Documentation",
    "iot":           "Passerelle IoT",
    "erp":           "Connecteurs ERP",
    "activity":      "Journal d'activité",
    "settings":      "Paramètres",
    "users":         "Utilisateurs & rôles",
    "org":           "Organisation",
    "configuration": "Configuration",
    "profile":       "Mon profil"
  }
}
```

```json
// src/i18n/en/shell.json
{
  "sidebar": { "toggle": "Toggle sidebar" },
  "search":  { "placeholder": "Search or open a module…" },
  "notifications": { "label": "{{count}} unread notifications" },
  "user":    { "menu": "User menu" },
  "sync": {
    "offline": "Offline",
    "syncing": "Syncing…",
    "error":   "Sync error"
  },
  "status": {
    "online":      "Online",
    "offline":     "Offline",
    "db":          "DB",
    "dbHealthy":   "Database operational",
    "pendingSync": "{{count}} pending"
  },
  "startup": {
    "loading":     "Starting up…",
    "errorTitle":  "Startup error",
    "retry":       "Retry"
  },
  "nav": {
    "groups": {
      "core":          "Operations",
      "planning":      "Planning",
      "compliance":    "Compliance",
      "inventory":     "Inventory",
      "analytics":     "Analytics",
      "admin":         "Administration"
    },
    "dashboard":     "Dashboard",
    "equipment":     "Equipment",
    "requests":      "Intervention Requests",
    "workOrders":    "Work Orders",
    "planning":      "Planning",
    "pm":            "Preventive Maintenance",
    "permits":       "Work Permits",
    "inspections":   "Inspection Rounds",
    "inventory":     "Spare Parts",
    "personnel":     "Personnel",
    "training":      "Training & Habilitation",
    "analytics":     "Dashboards",
    "reliability":   "Reliability (RAMS)",
    "budget":        "Budget & Costs",
    "archive":       "Archive",
    "lookups":       "Reference Data",
    "notifications": "Notifications",
    "documentation": "Documentation",
    "iot":           "IoT Gateway",
    "erp":           "ERP Connectors",
    "activity":      "Activity Log",
    "settings":      "Settings",
    "users":         "Users & Roles",
    "org":           "Organization",
    "configuration": "Configuration",
    "profile":       "My Profile"
  }
}
```

Also update src/i18n/index.ts to import and register the shell namespace:
```typescript
import frShell from "./fr/shell.json";
import enShell from "./en/shell.json";
// Add to resources:
// fr: { common: frCommon, shell: frShell }
// en: { common: enCommon, shell: enShell }
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- pnpm run typecheck and pnpm run lint:check both pass with 0 errors
- pnpm run dev shows: dark top bar, dark sidebar, dark status bar
- Status bar bottom shows "En ligne" (or "Online") with a green dot
- Sidebar has links visible (icons + labels in French)
- Clicking the hamburger icon collapses the sidebar to icon-only width
- Sidebar preference persists across page reload (Zustand persist middleware)
- Loading screen shows while appStatus === "loading" (before Rust emits ready)
```

---

### Supervisor Verification — Sprint S2

**V1 — The shell layout is correct.**
Run `pnpm run dev`. The Tauri window should show:
- A dark bar across the very top (top bar)
- A dark panel on the left side with navigation icons (sidebar)
- A narrow dark bar at the very bottom (status bar)
The three zones should all be visible simultaneously without scrolling.
If any of the three zones is absent, flag which one is missing.

**V2 — Status bar shows connectivity.**
Look at the very bottom bar. You should see a small green dot followed by the word
"En ligne" (French for Online) or "Online". If the dot is orange or the text says
"Hors ligne / Offline" unexpectedly, flag it.

**V3 — Sidebar collapse works.**
Click the hamburger icon in the top bar (three horizontal lines, far left). The sidebar
should shrink to a narrow icon-only strip. Click it again — the sidebar should expand with
text labels visible. If the sidebar does not change size on click, flag it.

**V4 — Sidebar preference is remembered.**
With the sidebar collapsed, close the application window (press Ctrl+C in terminal to
stop, then `pnpm run dev` again). The sidebar should reopen already in the collapsed
state. If it opens expanded instead, flag it.

**V5 — French text is displayed.**
Look at the sidebar labels — they should be in French (e.g., "Équipements", "Ordres de
travail", "Planification"). If you see English labels or raw translation keys like
`nav.workOrders`, flag it.

**V6 — Loading screen appears briefly on startup.**
Watch carefully when you run `pnpm run dev` — for a moment before the shell appears, a
dark screen with a spinner and text "Démarrage en cours…" should flash. If the shell
appears instantly with no loading screen, it may be working too fast to notice — this is
acceptable. Flag only if an error screen appears instead of the shell.

---

## Sprint S3 — Router, Navigation Registry, and Placeholder Pages

### AI Agent Prompt

```
You are a senior React and TypeScript engineer continuing work on Maintafox Desktop.
Sprints S1 and S2 are complete: design tokens, AppShell, TopBar, Sidebar, StatusBar,
startup bridge, and store are all in place.

YOUR TASK: Wire React Router v6, create the navigation registry matching all 27 module
routes from PRD §6, build lazy-loaded placeholder pages for each module, and update the
i18n namespace registration in shared/ipc-types.ts as needed.

────────────────────────────────────────────────────────────────────
STEP 1 — Install react-router-dom
────────────────────────────────────────────────────────────────────
Add to package.json dependencies:
  "react-router-dom": "^6.28.0"
  "@types/react-router-dom" is not needed — react-router-dom ships its own types.

────────────────────────────────────────────────────────────────────
STEP 2 — Create src/navigation/nav-registry.tsx
────────────────────────────────────────────────────────────────────
This file defines all 27 module routes, their sidebar entries, and their group
assignments. It is the single source of truth for navigation structure.

```tsx
// src/navigation/nav-registry.tsx
// One entry per PRD §6 module. Routes map directly to ModuleKey in src/types/index.ts.

import {
  LayoutDashboard, Cog, Building2, Wrench, ClipboardList, Users,
  UserCog, Package, CalendarClock, Activity, BarChart3, Archive,
  BookOpen, Bell, HelpCircle, Calendar, ScrollText, Settings,
  User, GraduationCap, Radio, Link, ShieldCheck, DollarSign,
  CheckSquare, Sliders,
} from "lucide-react";
import type { NavItem } from "@/components/layout/Sidebar";

export const defaultNavItems: NavItem[] = [
  // ── Core Operations ───────────────────────────────────────
  { key: "g-core",       labelKey: "nav.groups.core",       path: "#", icon: null, isGroupHeader: true },
  { key: "dashboard",    labelKey: "nav.dashboard",          path: "/",                  icon: <LayoutDashboard className="h-4 w-4" /> },
  { key: "equipment",    labelKey: "nav.equipment",          path: "/equipment",         icon: <Cog className="h-4 w-4" /> },
  { key: "requests",     labelKey: "nav.requests",           path: "/requests",          icon: <ClipboardList className="h-4 w-4" /> },
  { key: "work-orders",  labelKey: "nav.workOrders",         path: "/work-orders",       icon: <Wrench className="h-4 w-4" /> },

  // ── Planning ──────────────────────────────────────────────
  { key: "g-planning",   labelKey: "nav.groups.planning",    path: "#", icon: null, isGroupHeader: true },
  { key: "planning",     labelKey: "nav.planning",           path: "/planning",          icon: <Calendar className="h-4 w-4" /> },
  { key: "pm",           labelKey: "nav.pm",                 path: "/pm",                icon: <CalendarClock className="h-4 w-4" /> },

  // ── Compliance ────────────────────────────────────────────
  { key: "g-compliance", labelKey: "nav.groups.compliance",  path: "#", icon: null, isGroupHeader: true },
  { key: "permits",      labelKey: "nav.permits",            path: "/permits",           icon: <ShieldCheck className="h-4 w-4" /> },
  { key: "inspections",  labelKey: "nav.inspections",        path: "/inspections",       icon: <CheckSquare className="h-4 w-4" /> },
  { key: "training",     labelKey: "nav.training",           path: "/training",          icon: <GraduationCap className="h-4 w-4" /> },

  // ── Inventory ─────────────────────────────────────────────
  { key: "g-inventory",  labelKey: "nav.groups.inventory",   path: "#", icon: null, isGroupHeader: true },
  { key: "inventory",    labelKey: "nav.inventory",          path: "/inventory",         icon: <Package className="h-4 w-4" /> },

  // ── Analytics & Reporting ─────────────────────────────────
  { key: "g-analytics",  labelKey: "nav.groups.analytics",   path: "#", icon: null, isGroupHeader: true },
  { key: "analytics",    labelKey: "nav.analytics",          path: "/analytics",         icon: <BarChart3 className="h-4 w-4" /> },
  { key: "reliability",  labelKey: "nav.reliability",        path: "/reliability",       icon: <Activity className="h-4 w-4" /> },
  { key: "budget",       labelKey: "nav.budget",             path: "/budget",            icon: <DollarSign className="h-4 w-4" /> },

  // ── Administration ────────────────────────────────────────
  { key: "g-admin",      labelKey: "nav.groups.admin",       path: "#", icon: null, isGroupHeader: true },
  { key: "personnel",    labelKey: "nav.personnel",          path: "/personnel",         icon: <Users className="h-4 w-4" /> },
  { key: "users",        labelKey: "nav.users",              path: "/users",             icon: <UserCog className="h-4 w-4" /> },
  { key: "org",          labelKey: "nav.org",                path: "/org",               icon: <Building2 className="h-4 w-4" /> },
  { key: "lookups",      labelKey: "nav.lookups",            path: "/lookups",           icon: <BookOpen className="h-4 w-4" /> },
  { key: "notifications",labelKey: "nav.notifications",      path: "/notifications",     icon: <Bell className="h-4 w-4" /> },
  { key: "documentation",labelKey: "nav.documentation",      path: "/documentation",     icon: <HelpCircle className="h-4 w-4" /> },
  { key: "iot",          labelKey: "nav.iot",                path: "/iot",               icon: <Radio className="h-4 w-4" /> },
  { key: "erp",          labelKey: "nav.erp",                path: "/erp",              icon: <Link className="h-4 w-4" /> },
  { key: "archive",      labelKey: "nav.archive",            path: "/archive",           icon: <Archive className="h-4 w-4" /> },
  { key: "activity",     labelKey: "nav.activity",           path: "/activity",          icon: <ScrollText className="h-4 w-4" /> },
  { key: "settings",     labelKey: "nav.settings",           path: "/settings",          icon: <Settings className="h-4 w-4" /> },
  { key: "configuration",labelKey: "nav.configuration",      path: "/configuration",     icon: <Sliders className="h-4 w-4" /> },
  { key: "profile",      labelKey: "nav.profile",            path: "/profile",           icon: <User className="h-4 w-4" /> },
];

// Flat route list for React Router (excludes group headers)
export const appRoutes = defaultNavItems.filter((i) => !i.isGroupHeader);
```

────────────────────────────────────────────────────────────────────
STEP 3 — Create placeholder pages for all 26 functional modules
────────────────────────────────────────────────────────────────────
Create src/pages/placeholder/ModulePlaceholder.tsx — one reusable component
used by every not-yet-implemented module page:

```tsx
// src/pages/placeholder/ModulePlaceholder.tsx
import { Construction } from "lucide-react";
import { useTranslation } from "react-i18next";

interface Props {
  moduleName: string;
  prdSection: string;
  phase: string;
}

export function ModulePlaceholder({ moduleName, prdSection, phase }: Props) {
  const { t } = useTranslation("shell");
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 text-center p-8">
      <Construction className="h-12 w-12 text-text-muted" />
      <p className="text-xl font-semibold text-text-primary">{moduleName}</p>
      <p className="text-sm text-text-secondary max-w-sm">
        {t("placeholder.notYetImplemented", { phase })}
      </p>
      <p className="text-xs text-text-muted">PRD §{prdSection}</p>
    </div>
  );
}
```

Add to fr/shell.json and en/shell.json:
```json
"placeholder": {
  "notYetImplemented": "Ce module sera implémenté lors de la {{phase}}."
}
// en:
"placeholder": {
  "notYetImplemented": "This module will be implemented in {{phase}}."
}
```

Create one page file per module under src/pages/. Each file is a lazy-loadable
module that renders ModulePlaceholder. Create all of the following:

src/pages/DashboardPage.tsx
src/pages/EquipmentPage.tsx         — PRD §6.3, Phase 2
src/pages/RequestsPage.tsx          — PRD §6.4, Phase 2
src/pages/WorkOrdersPage.tsx        — PRD §6.5, Phase 2
src/pages/PlanningPage.tsx          — PRD §6.16, Phase 3
src/pages/PmPage.tsx                — PRD §6.9, Phase 3
src/pages/PermitsPage.tsx           — PRD §6.23, Phase 3
src/pages/InspectionsPage.tsx       — PRD §6.25, Phase 3
src/pages/TrainingPage.tsx          — PRD §6.20, Phase 3
src/pages/InventoryPage.tsx         — PRD §6.8, Phase 3
src/pages/AnalyticsPage.tsx         — PRD §6.11, Phase 5
src/pages/ReliabilityPage.tsx       — PRD §6.10, Phase 5
src/pages/BudgetPage.tsx            — PRD §6.24, Phase 3
src/pages/PersonnelPage.tsx         — PRD §6.6, Phase 2
src/pages/UsersPage.tsx             — PRD §6.7, Phase 2
src/pages/OrgPage.tsx               — PRD §6.2, Phase 2
src/pages/LookupsPage.tsx           — PRD §6.13, Phase 2
src/pages/NotificationsPage.tsx     — PRD §6.14, Phase 2
src/pages/DocumentationPage.tsx     — PRD §6.15, Phase 2
src/pages/IotPage.tsx               — PRD §6.21, Phase 4
src/pages/ErpPage.tsx               — PRD §6.22, Phase 4
src/pages/ArchivePage.tsx           — PRD §6.12, Phase 2
src/pages/ActivityPage.tsx          — PRD §6.17, Phase 2
src/pages/SettingsPage.tsx          — PRD §6.18, Phase 1 (Sub-phase 06)
src/pages/ConfigurationPage.tsx     — PRD §6.26, Phase 2
src/pages/ProfilePage.tsx           — PRD §6.19, Phase 2

Pattern for each (example for WorkOrdersPage.tsx):
```tsx
// src/pages/WorkOrdersPage.tsx
import { ModulePlaceholder } from "./placeholder/ModulePlaceholder";

export function WorkOrdersPage() {
  return (
    <ModulePlaceholder
      moduleName="Ordres de travail"
      prdSection="6.5"
      phase="Phase 2"
    />
  );
}
```

DashboardPage.tsx should render a more informative placeholder:
```tsx
// src/pages/DashboardPage.tsx
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/store/app-store";

export function DashboardPage() {
  const { t }      = useTranslation("shell");
  const appVersion = useAppStore((s) => s.appVersion);
  const isOnline   = useAppStore((s) => s.isOnline);

  return (
    <div className="p-6 space-y-4">
      <h1 className="text-2xl font-semibold text-text-primary">Maintafox</h1>
      <p className="text-text-secondary">
        {t("startup.loading")}... Phase 1 — infrastructure en place.
      </p>
      <div className="grid grid-cols-2 gap-3 max-w-sm mt-4">
        <div className="rounded-lg bg-surface-2 border border-surface-border p-4">
          <p className="text-xs text-text-muted">Version</p>
          <p className="text-lg font-mono text-text-primary">{appVersion || "—"}</p>
        </div>
        <div className="rounded-lg bg-surface-2 border border-surface-border p-4">
          <p className="text-xs text-text-muted">Connexion</p>
          <p className={`text-lg font-semibold ${isOnline ? "text-status-success" : "text-status-warning"}`}>
            {isOnline ? "En ligne" : "Hors ligne"}
          </p>
        </div>
      </div>
    </div>
  );
}
```

────────────────────────────────────────────────────────────────────
STEP 4 — Create src/router.tsx and update src/App.tsx
────────────────────────────────────────────────────────────────────
```tsx
// src/router.tsx
import { lazy, Suspense } from "react";
import { createBrowserRouter, type RouteObject } from "react-router-dom";
import { AppShell } from "@/components/layout/AppShell";
import { DashboardPage } from "@/pages/DashboardPage";

// Lazy-load all module pages
const EquipmentPage     = lazy(() => import("@/pages/EquipmentPage").then((m) => ({ default: m.EquipmentPage })));
const RequestsPage      = lazy(() => import("@/pages/RequestsPage").then((m) => ({ default: m.RequestsPage })));
const WorkOrdersPage    = lazy(() => import("@/pages/WorkOrdersPage").then((m) => ({ default: m.WorkOrdersPage })));
const PlanningPage      = lazy(() => import("@/pages/PlanningPage").then((m) => ({ default: m.PlanningPage })));
const PmPage            = lazy(() => import("@/pages/PmPage").then((m) => ({ default: m.PmPage })));
const PermitsPage       = lazy(() => import("@/pages/PermitsPage").then((m) => ({ default: m.PermitsPage })));
const InspectionsPage   = lazy(() => import("@/pages/InspectionsPage").then((m) => ({ default: m.InspectionsPage })));
const TrainingPage      = lazy(() => import("@/pages/TrainingPage").then((m) => ({ default: m.TrainingPage })));
const InventoryPage     = lazy(() => import("@/pages/InventoryPage").then((m) => ({ default: m.InventoryPage })));
const AnalyticsPage     = lazy(() => import("@/pages/AnalyticsPage").then((m) => ({ default: m.AnalyticsPage })));
const ReliabilityPage   = lazy(() => import("@/pages/ReliabilityPage").then((m) => ({ default: m.ReliabilityPage })));
const BudgetPage        = lazy(() => import("@/pages/BudgetPage").then((m) => ({ default: m.BudgetPage })));
const PersonnelPage     = lazy(() => import("@/pages/PersonnelPage").then((m) => ({ default: m.PersonnelPage })));
const UsersPage         = lazy(() => import("@/pages/UsersPage").then((m) => ({ default: m.UsersPage })));
const OrgPage           = lazy(() => import("@/pages/OrgPage").then((m) => ({ default: m.OrgPage })));
const LookupsPage       = lazy(() => import("@/pages/LookupsPage").then((m) => ({ default: m.LookupsPage })));
const NotificationsPage = lazy(() => import("@/pages/NotificationsPage").then((m) => ({ default: m.NotificationsPage })));
const DocumentationPage = lazy(() => import("@/pages/DocumentationPage").then((m) => ({ default: m.DocumentationPage })));
const IotPage           = lazy(() => import("@/pages/IotPage").then((m) => ({ default: m.IotPage })));
const ErpPage           = lazy(() => import("@/pages/ErpPage").then((m) => ({ default: m.ErpPage })));
const ArchivePage       = lazy(() => import("@/pages/ArchivePage").then((m) => ({ default: m.ArchivePage })));
const ActivityPage      = lazy(() => import("@/pages/ActivityPage").then((m) => ({ default: m.ActivityPage })));
const SettingsPage      = lazy(() => import("@/pages/SettingsPage").then((m) => ({ default: m.SettingsPage })));
const ConfigurationPage = lazy(() => import("@/pages/ConfigurationPage").then((m) => ({ default: m.ConfigurationPage })));
const ProfilePage       = lazy(() => import("@/pages/ProfilePage").then((m) => ({ default: m.ProfilePage })));

function PageSuspense({ children }: { children: React.ReactNode }) {
  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
        </div>
      }
    >
      {children}
    </Suspense>
  );
}

const routes: RouteObject[] = [
  {
    path: "/",
    element: <AppShell><DashboardPage /></AppShell>,
  },
  { path: "/equipment",      element: <AppShell><PageSuspense><EquipmentPage /></PageSuspense></AppShell> },
  { path: "/requests",       element: <AppShell><PageSuspense><RequestsPage /></PageSuspense></AppShell> },
  { path: "/work-orders",    element: <AppShell><PageSuspense><WorkOrdersPage /></PageSuspense></AppShell> },
  { path: "/planning",       element: <AppShell><PageSuspense><PlanningPage /></PageSuspense></AppShell> },
  { path: "/pm",             element: <AppShell><PageSuspense><PmPage /></PageSuspense></AppShell> },
  { path: "/permits",        element: <AppShell><PageSuspense><PermitsPage /></PageSuspense></AppShell> },
  { path: "/inspections",    element: <AppShell><PageSuspense><InspectionsPage /></PageSuspense></AppShell> },
  { path: "/training",       element: <AppShell><PageSuspense><TrainingPage /></PageSuspense></AppShell> },
  { path: "/inventory",      element: <AppShell><PageSuspense><InventoryPage /></PageSuspense></AppShell> },
  { path: "/analytics",      element: <AppShell><PageSuspense><AnalyticsPage /></PageSuspense></AppShell> },
  { path: "/reliability",    element: <AppShell><PageSuspense><ReliabilityPage /></PageSuspense></AppShell> },
  { path: "/budget",         element: <AppShell><PageSuspense><BudgetPage /></PageSuspense></AppShell> },
  { path: "/personnel",      element: <AppShell><PageSuspense><PersonnelPage /></PageSuspense></AppShell> },
  { path: "/users",          element: <AppShell><PageSuspense><UsersPage /></PageSuspense></AppShell> },
  { path: "/org",            element: <AppShell><PageSuspense><OrgPage /></PageSuspense></AppShell> },
  { path: "/lookups",        element: <AppShell><PageSuspense><LookupsPage /></PageSuspense></AppShell> },
  { path: "/notifications",  element: <AppShell><PageSuspense><NotificationsPage /></PageSuspense></AppShell> },
  { path: "/documentation",  element: <AppShell><PageSuspense><DocumentationPage /></PageSuspense></AppShell> },
  { path: "/iot",            element: <AppShell><PageSuspense><IotPage /></PageSuspense></AppShell> },
  { path: "/erp",            element: <AppShell><PageSuspense><ErpPage /></PageSuspense></AppShell> },
  { path: "/archive",        element: <AppShell><PageSuspense><ArchivePage /></PageSuspense></AppShell> },
  { path: "/activity",       element: <AppShell><PageSuspense><ActivityPage /></PageSuspense></AppShell> },
  { path: "/settings",       element: <AppShell><PageSuspense><SettingsPage /></PageSuspense></AppShell> },
  { path: "/configuration",  element: <AppShell><PageSuspense><ConfigurationPage /></PageSuspense></AppShell> },
  { path: "/profile",        element: <AppShell><PageSuspense><ProfilePage /></PageSuspense></AppShell> },
];

export const router = createBrowserRouter(routes);
```

Update src/App.tsx:
```tsx
// src/App.tsx
import { RouterProvider } from "react-router-dom";
import { ThemeProvider } from "@/components/ui/ThemeProvider";
import { router } from "@/router";
import "../src/i18n";  // initialize i18n before rendering

export function App() {
  return (
    <ThemeProvider>
      <RouterProvider router={router} />
    </ThemeProvider>
  );
}
```

Update src/main.tsx to import i18n before App to ensure locale is loaded:
```tsx
import "@/i18n"; // must be first
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

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- pnpm run typecheck and pnpm run lint:check pass with 0 errors
- pnpm run dev: clicking each sidebar link navigates to the correct placeholder page
- All 26 module placeholder pages are present in src/pages/
- DashboardPage shows the version and online/offline status cards
- Navigation active state (highlighted item) matches the current route
- French labels appear on all navigation items
- pnpm run test passes (existing test suite remains green)
```

---

### Supervisor Verification — Sprint S3

**V1 — All navigation items are visible.**
Run `pnpm run dev`. Count the items in the left sidebar. You should be able to scroll
through and find groups labelled "Opérations", "Planification", "Conformité", "Stocks",
"Analytique", and "Administration". Each group should have items under it. If any group
heading is missing, flag it.

**V2 — Navigation works.**
Click each of the following sidebar items and confirm the main content area changes to a
placeholder message:
- "Équipements" — should show "PRD §6.3" and "Phase 2"
- "Ordres de travail" — should show "PRD §6.5" and "Phase 2"
- "Permis de travail" — should show "PRD §6.23" and "Phase 3"
- "Fiabilité (RAMS)" — should show "PRD §6.10" and "Phase 5"

If any of these shows a blank white area or a browser error instead of the placeholder,
flag it with the module name.

**V3 — Dashboard shows system information.**
Click "Tableau de bord" in the sidebar. The main area should show two cards: one with
"Version" and a version number (e.g., `0.1.0`), and one with "Connexion" showing "En
ligne" in green. If both cards are present and show values, it means the Tauri IPC
bridge from File 02 (get_app_info command) is working correctly. If the cards are empty
or show "—", flag it.

**V4 — Active navigation state is highlighted.**
Click "Personnel" in the sidebar. That item should appear in a slightly different color
or brightness than the other items — a visual indicator that it is the current page.
Click a different item — the highlight should move. If there is no visible difference
between active and inactive nav items, flag it.

**V5 — 26 module page files exist.**
In VS Code Explorer, expand `src/pages/`. Count the files. There should be at least 26
module page files plus the `placeholder/` subfolder. If the count is below 26, flag how
many are missing.

---

*End of Phase 1 · Sub-phase 02 · File 03*
*Next: File 04 — Shell Integration Testing and Runtime Hardening*
