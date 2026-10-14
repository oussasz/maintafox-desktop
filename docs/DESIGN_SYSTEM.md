# Maintafox desktop design system

One look and one behaviour for every module, extracted from the **Work Orders (OT)** and
**Intervention Requests (DI)** screens, which the owner chose as the reference.

- Living reference (development builds): `/__dev/components` (`src/pages/dev/ComponentShowcase.tsx`).
- Detailed pattern notes kept as before: `UX_DETAIL_DIALOG_PATTERN.md`, `UX_ENTITY_FORM_DIALOG_PATTERN.md`,
  `UX_IDENTIFICATION_DIALOG_PATTERN.md`, `UX_EXPORT_SYSTEM.md`, `UI_COMPONENT_PATTERNS.md`.

## 1. Identity and tokens

| Role | Value | Where |
|---|---|---|
| Brand blue (primary actions, links, focus) | `#0049AC` | `primary` (Tailwind), `--color-primary` |
| Navy (text, sidebar) | `#0B1F3A` | `brand.navy`, `--text-primary` (light) |
| Logo orange (alerts, the one attention point of a screen) | `#F68714` | `accent`, `brand.orange` |
| Grounds | icy blue `#F3F6FB` / white | `surface-0` / `surface-1` (light theme) |

- Colours for surfaces and text come from CSS variables in `src/styles/globals.css` (light and dark).
  Brand colours are literal hex in `tailwind.config.ts` so opacity modifiers (`bg-primary/80`) work.
- Text contrast: `text-muted` is ≥ 4.5:1 on every surface in both themes. Do not use lighter greys.
- Class-string tokens for layout live in `src/design-system/tokens.ts` (`mfLayout`, `mfCard`, `mfModal`,
  `mfEntityForm`, `mfTable`, `mfKanban`, `mfTimeline`, …). Never hard-code hex or raw palette colours
  (`bg-amber-100`, `text-red-700`) in feature code.

## 2. Status tones — one colour vocabulary

`src/design-system/tone.ts`. Every business status maps to one of seven tones; the same meaning has the
same colour in every module.

| Tone | Meaning | Examples |
|---|---|---|
| `neutral` | not started, archived | draft, closed, deferred |
| `info` | queued, waiting on someone else | submitted, planning, ready |
| `progress` | work happening | in progress, in review |
| `attention` | the reader must decide (orange) | awaiting approval, on hold |
| `warning` | at risk | due soon, clarification requested, low stock |
| `success` | done and accepted | approved, completed, verified |
| `danger` | failed, overdue, blocked, cancelled, safety | overdue, cancelled, permit missing |

- Add a module's map next to the existing ones (`WO_STATUS_TONE`, `DI_STATUS_TONE`, `DI_URGENCY_TONE`,
  `URGENCY_TONE`) and render with `<StatusChip label={t(...)} tone={toneOf(MAP, code)} />`.
- Legacy `*_STYLE` maps (`STATUS_STYLE`, `DI_STATUS_STYLE`, …) are derived with `toneChipMap` and stay for
  existing call sites; new code uses `StatusChip`.
- Labels are always translated. A raw status code on screen is a bug.

## 3. Module page anatomy (OT/DI pattern)

```
ModulePageShell
├─ header: icon · title · count badge            [Create] [ViewToggle] [Filters] [Refresh]
├─ filterBar: SmartFilterBar + module context strip (e.g. review queue)
└─ workspace: ErrorState | TruncationNotice + view (DataTable | Kanban | Calendar | Dashboard)
   + archive panel, detail dialog, form dialog
```

- `ModulePageShell` (`components/layout`): pass `icon`, `title`, `count`, `actions`, `filterBar`;
  `bodyClassName={null}` when the view manages its own padding.
- `ModuleHeaderControls`: `ViewToggle` (radio group, hides permission-gated views), `FilterToggleButton`
  (shows the active-filter count when closed), `RefreshButton`. All icon-only controls have an accessible
  name and a 44 px target. The create button uses `mfLayout.primaryAction`, inside `PermissionGate`.
- One search box per page: the `SmartFilterBar`. Do not also turn on `DataTable searchable`.
- Lists: load pages from the backend (`limit` ≤ backend maximum, `offset`) and show `TruncationNotice`
  with "load more" whenever `items.length < total`. Never cap silently.
- Detail: centred dialog (`UX_DETAIL_DIALOG_PATTERN.md`), header with code, title, status chips and linked
  record badges (`LinkedEntityBadge`), body with info grid and tabs, footer with lifecycle actions.
- Forms: `EntityFormDialog` family (`UX_ENTITY_FORM_DIALOG_PATTERN.md`).

## 4. States

| Situation | Use |
|---|---|
| Loading the main content | `LoadingState` (or DataTable skeleton rows) |
| Main content failed to load | `ErrorState error={err} onRetry={reload}` in place of the content |
| Loaded, nothing matches | `EmptyState` with what to do next |
| Only part of the records shown | `TruncationNotice` with load more |
| An action failed (save, transition) | the global IPC toast only; inline only for field-specific messages |
| Destructive or irreversible action | `ConfirmDialog` naming the action; never `window.confirm` |

`describeAppError(err)` (`src/lib/app-error.ts`) turns any failure into translated text; it is shared by
the global toast and `ErrorState`. Never render `String(err)` or `err.message` directly
(IPC rejects are plain objects → "[object Object]").

## 5. KPI tiles

`KpiTile` (`components/metrics`): label, pre-formatted value (`useFormatters`), unit, one-line hint.
Grey by default; a `tone` only for an exception (overdue > 0 → `danger`). When the figure has a list
behind it, pass `onOpen` so the tile opens that filtered list. Every KPI states its period and unit.

## 6. Writing and data on screen

- FR and EN for every string (`t()` keys in both `src/i18n/*/` and `src/i18n/locale-data/*/`).
- No database ids, uuids, raw codes, JSON or English backend messages on screen. Show codes users know
  (OT-2026-0144) and names.
- Dates, numbers and money through `useFormatters()` (app language, tenant currency).
- Every record reference that has a screen is a link to it (`LinkedEntityBadge`).
- A button either works, or is removed, or is disabled with a visible reason.

## 7. Accessibility

- Targets: 44 px for module chrome (header actions, toggles, dialog buttons); dense table cells and form
  controls at least 36 px with spacing per WCAG 2.5.8.
- Keyboard: every action reachable by Tab; visible focus ring (`ring-ring`); dialogs trap focus (Radix).
- Contrast: AA for text; status is never colour-only (chips carry a label).
