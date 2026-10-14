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
  /** 40 px button inside a 2 px-padded group = 44 px target row */
  viewToggleButton:
    "inline-flex h-10 w-10 items-center justify-center rounded transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
  /** Icon-only toolbar button (filter toggle, refresh): 44 px target */
  toolbarIconButton: "h-11 w-11 p-0",
  /** Primary header action (create): 44 px high */
  primaryAction: "h-11 gap-1.5 px-4",
  /** Count badge next to the module title */
  moduleCount: "font-mono tabular-nums",
} as const;

/** Elevated panels — matches DI/OT cards and Settings sections (`border-surface-border`, `shadow-panel`). */
export const mfCard = {
  panel: "rounded-xl border border-surface-border bg-surface-1 p-4 shadow-panel",
  panelMuted: "rounded-xl border border-surface-border bg-surface-2/80 p-4 shadow-sm",
  insetCanvas: "rounded-xl border border-surface-border bg-surface-0 min-h-[280px] shadow-inner",
} as const;

/** Standalone auth / activation surfaces (login, lock, license gate, …) */
export const mfAuth = {
  /**
   * Page canvas. `mf-auth` (styles/auth.css) pins the light brand palette for these screens only;
   * `min-h-[100dvh]` + vertical padding lets short windows scroll instead of clipping the card.
   */
  shell:
    "mf-auth mf-auth-backdrop flex min-h-[100dvh] w-full flex-col items-center justify-center px-4 py-8 sm:px-6",
  /** Lock screen — same canvas as auth, no card */
  lockShell:
    "mf-auth mf-auth-backdrop flex min-h-[100dvh] w-full flex-col items-center justify-center px-4 py-8 sm:px-6",
  /** Primary login card: compact, white, subtle border and restrained shadow. */
  card: "w-full max-w-[400px] rounded-xl border border-surface-border bg-surface-1 p-6 shadow-[var(--mfa-shadow-card)] motion-safe:animate-in motion-safe:fade-in motion-safe:duration-200 sm:p-8",
  cardCompact:
    "w-full max-w-[480px] rounded-xl border border-surface-border bg-surface-1 p-6 shadow-[var(--mfa-shadow-card)] motion-safe:animate-in motion-safe:fade-in motion-safe:duration-200 sm:p-8",
  cardBrandSeparator: "mb-6 flex justify-center",
  title: "text-xl font-semibold tracking-tight text-text-primary",
  subtitle: "mt-1.5 text-sm leading-relaxed text-text-secondary",
  linkPrimary:
    "rounded-sm text-sm font-medium text-[var(--mfa-blue)] underline-offset-4 transition-colors hover:text-[var(--mfa-blue-dark)] hover:underline disabled:opacity-50",
  /** Primary action: logo orange with navy text (6.7:1). One per screen. */
  buttonPrimary:
    "inline-flex h-11 w-full items-center justify-center gap-2 rounded-lg bg-[var(--mfa-orange)] px-4 text-[15px] font-semibold text-[var(--mfa-navy)] shadow-sm transition-colors hover:bg-[var(--mfa-orange-dark)] active:translate-y-px disabled:cursor-not-allowed disabled:opacity-50",
  /** Secondary action: white with brand-blue outline. */
  buttonSecondary:
    "inline-flex h-11 w-full items-center justify-center gap-2 rounded-lg border border-[var(--mfa-blue)] bg-white px-4 text-[15px] font-semibold text-[var(--mfa-blue)] transition-colors hover:bg-[var(--mfa-blue-tint)] disabled:cursor-not-allowed disabled:opacity-50",
  /** Tertiary action: text only. */
  buttonGhost:
    "inline-flex h-10 w-full items-center justify-center gap-2 rounded-lg px-4 text-sm font-medium text-[var(--mfa-blue)] transition-colors hover:bg-[var(--mfa-blue-tint)] disabled:cursor-not-allowed disabled:opacity-50",
} as const;

/** Form controls aligned with shadcn `Input` + DI/OT filter bars */
export const mfInput = {
  filterSearch: "pl-9 h-8 text-sm",
  filterSelect: "h-8 text-sm",
  /** Auth-style fields (login) — use with `Input` or native `<input>` + cn() */
  authField:
    "h-11 w-full rounded-lg border border-[var(--mfa-border-strong)] bg-white pl-10 pr-3 text-[15px] text-text-primary placeholder:text-text-muted transition-[border-color,box-shadow] focus:border-[var(--mfa-blue)] focus:outline-none focus:ring-[3px] focus:ring-[var(--mfa-blue)]/20 disabled:cursor-not-allowed disabled:bg-surface-2 disabled:opacity-70 aria-[invalid=true]:border-[var(--mfa-danger)]",
  authFieldWithTrailing:
    "h-11 w-full rounded-lg border border-[var(--mfa-border-strong)] bg-white pl-10 pr-10 text-[15px] text-text-primary placeholder:text-text-muted transition-[border-color,box-shadow] focus:border-[var(--mfa-blue)] focus:outline-none focus:ring-[3px] focus:ring-[var(--mfa-blue)]/20 disabled:cursor-not-allowed disabled:bg-surface-2 disabled:opacity-70 aria-[invalid=true]:border-[var(--mfa-danger)]",
  authLabel: "mb-1.5 block text-[13px] font-medium text-text-primary",
} as const;

/**
 * Alert surfaces for authentication screens (scoped to `.mf-auth`): orange for attention,
 * red for errors, blue for information. Each is paired with an icon and `role` at the call site.
 */
export const mfAuthAlert = {
  info: "rounded-lg border border-[var(--mfa-border)] bg-[var(--mfa-blue-tint)] px-3.5 py-3 text-sm text-text-primary",
  warning:
    "rounded-lg border border-[var(--mfa-orange-border)] bg-[var(--mfa-orange-tint)] px-3.5 py-3 text-sm text-text-primary",
  danger:
    "rounded-lg border border-[#f3b8b3] bg-[var(--mfa-danger-tint)] px-3.5 py-3 text-sm text-text-primary",
  success:
    "rounded-lg border border-[#b5e0c4] bg-[var(--mfa-success-tint)] px-3.5 py-3 text-sm text-text-primary",
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
export { mfTone, type Tone } from "./tone";

export const mfTable = {
  rowHover: "transition-colors duration-fast hover:bg-surface-3/80",
  header: "bg-surface-2 text-xs font-semibold uppercase tracking-wide text-text-secondary",
} as const;

/**
 * Compact high-density metric grid (ISA-101 / industrial HMI).
 * Grey-scale for normal; status color only on exception values.
 * Canonical renderer: `src/components/metrics/CompactMetricGrid.tsx`.
 */
export const mfMetric = {
  panel: "overflow-hidden rounded-md border border-surface-border bg-surface-1",
  header:
    "flex items-baseline justify-between gap-3 border-b border-surface-border bg-surface-2/40 px-3 py-1.5",
  title: "font-mono text-[10px] font-semibold uppercase tracking-[0.14em] text-text-muted",
  hint: "min-w-0 truncate text-[10px] text-text-muted",
  /** Hairline cell grid — ISA-101 density, not card gutters. */
  grid: "grid gap-px bg-surface-border",
  cell: "flex min-h-[3.25rem] flex-col justify-center gap-0.5 bg-surface-1 px-3 py-2 text-start",
  cellInteractive:
    "transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
  label: "text-[10px] font-medium uppercase tracking-wide text-text-muted",
  value: "font-mono text-base font-semibold tabular-nums leading-none text-text-primary",
  valueMuted: "font-mono text-base font-semibold tabular-nums leading-none text-text-muted",
  delta: "font-mono text-[10px] tabular-nums text-text-muted",
  chartBody: "p-3",
  strip: "flex flex-wrap gap-px bg-surface-border",
  stripCell:
    "flex min-h-[3.25rem] min-w-[8.5rem] flex-1 items-center justify-between gap-2 bg-surface-1 px-2.5 py-1.5 text-start",
  barTrack: "flex h-2.5 w-full overflow-hidden rounded-sm bg-surface-2",
  barSegDanger: "h-full bg-status-danger",
  barSegWarn: "h-full bg-status-warning",
  barSegOk: "h-full bg-status-success",
  barSegNeutral: "h-full bg-surface-3",
  chipWarn:
    "ms-1 inline-block rounded-sm border border-status-warning/40 px-1 font-mono text-[9px] uppercase leading-4 text-text-warning",
  chipDanger:
    "ms-1 inline-block rounded-sm border border-status-danger/40 px-1 font-mono text-[9px] uppercase leading-4 text-text-danger",
} as const;

/**
 * Visual metric card — flat glass KPI unit with value, trend chip, and typed micro-viz.
 * Canonical renderer: `src/components/metrics/VisualMetricCard.tsx`.
 */
export const mfVisualMetric = {
  card: "relative overflow-hidden rounded-xl border border-surface-border bg-surface-1/80 text-left backdrop-blur-md transition-colors hover:bg-surface-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
  cardStatic:
    "relative overflow-hidden rounded-xl border border-surface-border bg-surface-1/80 backdrop-blur-md",
  body: "flex flex-row items-center justify-between gap-4 px-4 py-3",
  data: "min-w-0 shrink-0",
  dataWithViz: "min-w-0 flex-1 basis-[48%]",
  title: "text-[11px] font-medium uppercase tracking-[0.12em] text-text-muted",
  context: "mt-0.5 text-[11px] leading-snug text-text-secondary",
  valueRow: "mt-1.5 flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5",
  value: "font-mono text-2xl font-semibold tabular-nums leading-none text-text-primary",
  unit: "font-mono text-sm font-medium text-text-muted",
  badge:
    "inline-flex shrink-0 items-center rounded-md px-1.5 py-0.5 font-mono text-[10px] font-medium tabular-nums text-text-secondary",
  badgeUp: "bg-status-success/12 text-status-success",
  badgeDown: "bg-status-danger/12 text-status-danger",
  vizWrap:
    "flex h-[3.25rem] min-h-[3.25rem] min-w-[7rem] flex-1 basis-[48%] items-center justify-center self-stretch",
  viz: "h-full w-full min-h-[3.25rem]",
  /** Accent text color for title + micro-viz currentColor */
  accentTone: {
    default: "text-primary",
    brand: "text-primary",
    amber: "text-status-warning",
    danger: "text-status-danger",
    teal: "text-teal-600 dark:text-teal-400",
  },
  accentDot: {
    default: "bg-primary",
    brand: "bg-primary",
    amber: "bg-status-warning",
    danger: "bg-status-danger",
    teal: "bg-teal-500",
  },
  alertNormal:
    "inline-flex items-center rounded-md border border-surface-border bg-surface-0/60 px-2 py-1 font-mono text-[10px] font-medium uppercase tracking-wide text-text-muted",
  alertWarn:
    "inline-flex animate-pulse items-center rounded-md bg-status-warning/15 px-2 py-1 font-mono text-[10px] font-semibold uppercase tracking-wide text-status-warning",
  alertDanger:
    "inline-flex animate-pulse items-center rounded-md bg-status-danger/15 px-2 py-1 font-mono text-[10px] font-semibold uppercase tracking-wide text-status-danger",
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
