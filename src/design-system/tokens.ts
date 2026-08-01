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

/**
 * Create / Edit entity form dialogs — official Maintafox Enterprise Dialog Standard.
 * See docs/UX_ENTITY_FORM_DIALOG_PATTERN.md. Prefer `EntityFormDialog` over one-off shells.
 */
export const mfEntityForm = {
  dialog: "flex max-h-[90vh] w-full max-w-2xl flex-col gap-0 overflow-hidden p-0 sm:rounded-xl",
  dialogWide: "flex max-h-[90vh] w-full max-w-3xl flex-col gap-0 overflow-hidden p-0 sm:rounded-xl",
  header: "shrink-0 space-y-1.5 px-6 pb-3 pt-6 text-left",
  title: "text-lg font-semibold tracking-tight text-text-primary",
  description: "text-sm text-text-muted",
  body: "min-h-0 flex-1 space-y-6 overflow-y-auto px-6 py-2",
  footer:
    "flex shrink-0 flex-col-reverse gap-2 border-t border-surface-border bg-surface-0 px-6 py-4 sm:flex-row sm:justify-end sm:gap-2 sm:space-x-0",
  section: "space-y-3",
  sectionTitle: "text-sm font-semibold text-text-primary",
  sectionDescription: "text-xs text-text-muted",
  fieldGroup: "space-y-3 rounded-lg border border-surface-border/60 bg-surface-2/40 p-3",
  fieldGroupTitle: "text-xs font-semibold uppercase tracking-wide text-text-secondary",
  divider: "border-t border-surface-border",
  hint: "rounded-md border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-sm text-status-warning",
  hintInfo:
    "rounded-md border border-primary/20 bg-primary-bg/80 px-3 py-2 text-sm text-text-primary",
  collapsibleTrigger:
    "flex w-full items-center justify-between rounded-md border border-surface-border bg-surface-2/50 px-3 py-2 text-left text-sm font-semibold text-text-primary hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
  mediaDropzone:
    "flex min-h-[120px] cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-surface-border bg-surface-2/40 px-4 py-6 text-center text-sm text-text-muted transition-colors hover:border-primary/40 hover:bg-surface-2/70",
  mediaGrid: "grid grid-cols-3 gap-2 sm:grid-cols-4",
  mediaTile:
    "group relative aspect-square overflow-hidden rounded-md border border-surface-border bg-surface-2",
} as const;

/**
 * Identification / QR label dialogs — official Maintafox enterprise label modal.
 * See docs/UX_IDENTIFICATION_DIALOG_PATTERN.md. Prefer `IdentificationLabelDialog`.
 * Export/print: docs/UX_EXPORT_SYSTEM.md.
 */
export const mfIdentification = {
  dialog:
    "flex max-h-[90vh] w-full max-w-md flex-col gap-0 overflow-hidden p-0 sm:max-w-lg sm:rounded-xl",
  header: "shrink-0 space-y-1.5 px-6 pb-3 pt-6 text-center",
  title: "text-lg font-semibold tracking-tight text-text-primary",
  description: "text-sm text-text-muted",
  body: "min-h-0 flex-1 overflow-y-auto px-6 py-4",
  footer:
    "flex shrink-0 flex-col-reverse gap-2 border-t border-surface-border bg-surface-0 px-6 py-4 sm:flex-row sm:justify-end sm:gap-2 sm:space-x-0",
  labelStack: "flex flex-col items-center text-center space-y-3",
  logoRow: "flex flex-col items-center gap-1.5",
  /** QR sits directly on the dialog surface — no card / border / shadow. */
  qrSize:
    "mx-auto flex h-[min(72vw,280px)] w-[min(72vw,280px)] items-center justify-center [&_svg]:h-full [&_svg]:w-full",
  primaryCode: "font-mono text-base font-bold tracking-tight text-text-primary",
  entityName: "text-sm text-text-primary",
  metaRow: "text-xs text-text-muted",
  barcodeSlot: "hidden min-h-[48px] w-full data-[enabled=true]:block",
} as const;

/**
 * Shared export footer actions — use with `ExportActions`.
 * @see docs/UX_EXPORT_SYSTEM.md
 */
export const mfExport = {
  actions: "flex w-full flex-col-reverse gap-2 sm:flex-row sm:justify-end sm:gap-2 sm:space-x-0",
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

/**
 * Shared Kanban board — used by `components/kanban/KanbanBoard.tsx` only.
 * One design language for every status-lane board in the app.
 */
export const mfKanban = {
  root: "relative flex h-full flex-col",
  scroller: "flex flex-1 gap-3 overflow-x-auto p-1 pb-2",
  column:
    "flex min-w-[220px] max-w-[260px] flex-1 flex-col rounded-lg border border-surface-border bg-muted/30",
  columnDragOver: "ring-2 ring-primary ring-offset-1",
  columnHeader: "flex items-center justify-between gap-2 rounded-t-lg border-b px-3 py-2",
  columnHeaderLabel: "flex min-w-0 items-center gap-2",
  columnTitle: "truncate text-xs font-semibold",
  countBadge: "shrink-0 border-0 bg-white/60 px-1.5 py-0 text-[10px]",
  columnBody: "min-h-[120px] flex-1 space-y-2 overflow-y-auto p-2",
  columnEmpty: "py-4 text-center text-[11px] text-muted-foreground",
  loadMore: "border-t p-2",
  loadMoreButton: "w-full gap-1 text-xs",
  card: "group cursor-pointer overflow-hidden rounded-md border border-surface-border bg-background shadow-sm transition-shadow hover:shadow-md focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
  cardSelected: "ring-2 ring-primary",
  cardDisabled: "pointer-events-none opacity-50",
  cardDraggable: "cursor-grab active:cursor-grabbing",
  accentBar: "h-1 w-full",
  cardBody: "space-y-1.5 px-3 pb-3 pt-2",
  cardHeader: "flex items-center gap-1",
  cardCode: "truncate font-mono text-[11px] text-muted-foreground",
  cardTitle: "text-xs font-medium leading-tight line-clamp-2",
  cardSubtitle: "truncate text-[11px] text-muted-foreground",
  cardDescription: "text-[11px] leading-snug text-muted-foreground line-clamp-2",
  cardMeta: "flex items-center justify-between gap-2 pt-0.5 text-[10px] text-muted-foreground",
  metaItem: "flex min-w-0 items-center gap-0.5",
  metaIcon: "h-2.5 w-2.5 shrink-0",
  metaLabel: "truncate",
  badgeRow: "flex flex-wrap items-center gap-1",
  badge: "h-5 gap-0.5 border-0 px-1.5 py-0 text-[10px] font-normal",
  badgeIcon: "h-2.5 w-2.5 shrink-0",
  linkBadge: "h-5 px-1.5 py-0 text-[10px]",
  cardActions: "flex flex-wrap items-center gap-1 pt-1",
  cardActionButton: "h-6 gap-1 px-1.5 text-[10px]",
  cardActionIcon: "h-3 w-3 shrink-0",
  boardEmpty: "py-8 text-center text-sm text-muted-foreground",
} as const;

/**
 * Shared vertical timeline — used by `components/timeline/Timeline.tsx` only.
 * One design language for every chronological rail in the app.
 */
export const mfTimeline = {
  root: "space-y-4",
  list: "relative space-y-0",
  item: "relative flex gap-3 last:pb-0",
  connector: "absolute left-4 top-8 h-full w-px bg-surface-border",
  marker: "z-10 flex shrink-0 items-center justify-center rounded-full border-2 transition-colors",
  icon: "[&_svg]:h-4 [&_svg]:w-4",
  content: "rounded-md transition-colors hover:bg-surface-2/40",
  title: "text-sm font-medium text-text-primary",
  subtitle: "text-sm text-text-secondary",
  description: "text-sm text-muted-foreground",
  meta: "mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-muted-foreground",
  badge: "h-5 px-1.5 text-[10px] font-normal",
  groupHeader: "text-xs font-semibold uppercase tracking-wide text-text-muted",
} as const;

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
