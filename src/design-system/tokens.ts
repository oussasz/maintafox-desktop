/**
 * Maintafox design tokens — Tailwind class strings.
 *
 * Source of truth for layout/spacing: **DI (Requests)** and **OT (WorkOrders)** module pages.
 * Colors resolve via `globals.css` / `tailwind.config.ts` (surface-*, text-*, primary, status-*).
 *
 * Use with `cn()` from `@/lib/utils`. Do not duplicate one-off hex/rgba in feature pages;
 * extend this file or theme tokens instead.
 */

/** Full-height module shell (DI, OT, Settings, Admin, …) */
export const mfLayout = {
  moduleRoot: "flex h-full flex-col min-h-0",
  /** Top toolbar strip — matches DI/OT page header */
  moduleHeader: "flex items-center justify-between gap-4 px-6 py-3 border-b border-surface-border",
  /** Right cluster — primary/outline buttons, view toggles, refresh (DI/OT pattern) */
  moduleHeaderActions: "flex shrink-0 items-center gap-2",
  /** Optional secondary strip — search / filters */
  moduleFilterBar: "flex flex-wrap items-center gap-2 px-6 py-2 border-b border-surface-border",
  moduleWorkspace: "flex flex-1 min-h-0",
  moduleWorkspaceInner: "flex flex-col w-full min-h-0 overflow-auto",
  /** Default gutter inside workspace (matches DI list view `p-4`) */
  moduleWorkspaceBody: "p-4",
  /** Horizontal split layouts (e.g. asset registry) below header */
  moduleWorkspaceSplit: "flex min-h-0 flex-1 overflow-hidden",
  /** Scrollable content below header (Settings, Admin tabs body, …) */
  moduleContent: "flex flex-1 flex-col gap-6 overflow-auto p-6 min-h-0",
  moduleTitleRow: "flex items-center gap-3",
  moduleTitle: "text-xl font-semibold text-text-primary tracking-tight",
  /** Icon in module header (ClipboardList, Wrench, …) */
  moduleHeaderIcon: "h-5 w-5 shrink-0 text-text-muted",
  /** Segmented view control (list / kanban / …) */
  viewToggleGroup:
    "flex items-center rounded-md border border-surface-border p-0.5 gap-0.5 bg-surface-1/80",
  viewToggleButton: "h-7 px-2",
} as const;

/** Elevated panels — matches DI/OT cards and Settings sections (`border-surface-border`, `shadow-panel`). */
export const mfCard = {
  panel: "rounded-xl border border-surface-border bg-surface-1 p-4 shadow-panel",
  panelMuted: "rounded-xl border border-surface-border bg-surface-2/80 p-4 shadow-sm",
  insetCanvas: "rounded-xl border border-surface-border bg-surface-0 min-h-[280px] shadow-inner",
} as const;

/** Standalone auth / activation surfaces (login, lock, license gate, …) */
export const mfAuth = {
  shell: "flex min-h-screen w-screen items-center justify-center bg-surface-0 px-4",
  /** Lock screen — same canvas as auth, no card */
  lockShell: "flex min-h-screen w-full items-center justify-center bg-surface-0 px-4",
  /** Primary login card — elevated panel shadow from theme */
  card: "w-full max-w-[420px] rounded-2xl border border-surface-border bg-surface-1 px-10 py-12 shadow-panel animate-in fade-in zoom-in-95 duration-300",
  cardCompact:
    "w-full max-w-md rounded-xl border border-surface-border bg-surface-1 p-6 shadow-panel",
  cardBrandSeparator: "mb-6 flex justify-center border-b border-surface-border pb-5",
  linkPrimary: "text-sm font-medium text-primary hover:text-primary-dark transition-colors",
} as const;

/** Form controls aligned with shadcn `Input` + DI/OT filter bars */
export const mfInput = {
  filterSearch: "pl-9 h-8 text-sm",
  filterSelect: "h-8 text-sm",
  /** Auth-style fields (login) — use with `Input` or native `<input>` + cn() */
  authField:
    "w-full rounded-lg border border-surface-border bg-surface-2 py-2.5 pl-10 pr-3 text-sm text-text-primary shadow-sm placeholder:text-text-muted transition-all focus:border-primary-light focus:bg-surface-1 focus:outline-none focus:ring-2 focus:ring-primary/25",
  authFieldWithTrailing:
    "w-full rounded-lg border border-surface-border bg-surface-2 py-2.5 pl-10 pr-10 text-sm text-text-primary shadow-sm placeholder:text-text-muted transition-all focus:border-primary-light focus:bg-surface-1 focus:outline-none focus:ring-2 focus:ring-primary/25",
  authLabel: "mb-1.5 block text-sm font-medium text-text-secondary",
} as const;

/** Alert / callout surfaces (login errors, license meta, …) */
export const mfAlert = {
  info: "rounded-lg border border-primary/20 bg-primary-bg/90 px-4 py-3 text-sm text-text-primary",
  warning:
    "rounded-lg border border-status-warning/35 bg-status-warning/10 px-4 py-3 text-sm text-text-warning",
  danger:
    "rounded-lg border border-status-danger/30 bg-status-danger/10 px-4 py-3 text-sm text-text-danger",
  success:
    "rounded-lg border border-status-success/30 bg-status-success/10 px-4 py-3 text-sm text-text-success",
} as const;

/**
 * Modal / dialog — applied in `components/ui/dialog.tsx` so all Radix dialogs
 * match DI/OT detail dialogs (cinematic overlay + premium card).
 */
export const mfModal = {
  /** Base overlay only; Radix `data-[state=*]` fade classes are applied in `dialog.tsx`. */
  overlay: "fixed inset-0 z-50 bg-[rgb(6,10,18)]/78 backdrop-blur-[3px]",
  content:
    "gap-4 border border-surface-border bg-surface-1 p-6 text-text-primary shadow-panel duration-200 sm:rounded-xl",
} as const;

/** Table / list rows — prefer with DataTable; fallback for plain lists */
export const mfTable = {
  rowHover: "transition-colors duration-fast hover:bg-surface-3/80",
  header: "bg-surface-2 text-xs font-semibold uppercase tracking-wide text-text-secondary",
} as const;

/**
 * Buttons — mirrors `components/ui/button.tsx` (`buttonVariants`).
 * Prefer `<Button variant="…" />`; use these strings with `cn()` for native `<button>` when needed.
 */
export const mfButton = {
  base: "inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
  primary: "bg-primary text-primary-foreground hover:bg-primary-dark",
  secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80",
  destructive: "bg-destructive text-destructive-foreground hover:bg-destructive/90",
  outline: "border border-input bg-background hover:bg-accent hover:text-accent-foreground",
  ghost: "hover:bg-accent hover:text-accent-foreground",
  link: "text-primary underline-offset-4 hover:underline",
  sizeDefault: "h-10 px-4 py-2",
  sizeSm: "h-9 rounded-md px-3",
  sizeLg: "h-11 rounded-md px-8",
  sizeIcon: "h-10 w-10",
} as const;

/** Compact chips / overflow — replaces raw `gray-*` fallbacks */
export const mfChip = {
  neutral: "border border-surface-border bg-surface-3/70 text-text-secondary",
  neutralStrong: "border border-surface-border bg-surface-3 text-text-primary",
  fullAccess: "border border-accent/40 bg-accent/15 text-accent font-semibold",
} as const;

/**
 * Permission catalog domain chips — theme-friendly borders + tints (no slate/gray/zinc defaults).
 * Keys align with RBAC domain prefixes used in admin panels.
 */
export const mfPermissionDomainChip: Record<string, string> = {
  eq: "border border-orange-500/35 bg-orange-500/10 text-orange-200",
  di: "border border-blue-500/35 bg-blue-500/10 text-blue-200",
  ot: "border border-emerald-500/35 bg-emerald-500/10 text-emerald-200",
  org: "border border-pink-500/35 bg-pink-500/10 text-pink-200",
  per: "border border-cyan-500/35 bg-cyan-500/10 text-cyan-200",
  ref: "border border-surface-border bg-surface-3/70 text-text-secondary",
  inv: "border border-amber-500/35 bg-amber-500/10 text-amber-200",
  pm: "border border-violet-500/35 bg-violet-500/10 text-violet-200",
  ram: "border border-teal-500/35 bg-teal-500/10 text-teal-200",
  rep: "border border-indigo-500/35 bg-indigo-500/10 text-indigo-200",
  arc: "border border-surface-border bg-surface-2/90 text-text-secondary",
  doc: "border border-lime-500/35 bg-lime-500/10 text-lime-200",
  plan: "border border-sky-500/35 bg-sky-500/10 text-sky-200",
  log: "border border-surface-border bg-surface-3/70 text-text-secondary",
  trn: "border border-fuchsia-500/35 bg-fuchsia-500/10 text-fuchsia-200",
  iot: "border border-rose-500/35 bg-rose-500/10 text-rose-200",
  erp: "border border-yellow-500/35 bg-yellow-500/10 text-yellow-200",
  ptw: "border border-red-500/35 bg-red-500/10 text-red-200",
  fin: "border border-green-500/35 bg-green-500/10 text-green-200",
  ins: "border border-purple-500/35 bg-purple-500/10 text-purple-200",
  cfg: "border border-surface-border bg-surface-3/70 text-text-secondary",
  adm: "border border-red-500/40 bg-red-500/12 text-red-200",
  cst: "border border-surface-border bg-surface-3/70 text-text-secondary",
};

/** Recharts / export — use CSS vars in SVG where supported; PNG needs resolved hex */
export const mfChart = {
  /** Dark canvas / PNG export (matches default `--surface-0`) */
  exportPngBackground: "#0f172a",
  axisTickFill: "var(--text-secondary)",
  tooltipBg: "var(--surface-1)",
  tooltipBorder: "var(--surface-border)",
  barFill: "var(--color-accent)",
  gridStroke: "rgba(148, 163, 184, 0.25)",
} as const;
