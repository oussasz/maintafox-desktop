# Reference Data UI Consistency

This guideline defines the required UI structure for all right-pane editors in `Données de référence`.

## Canonical Pattern

Use `src/components/lookups/ReferenceValueEditor.tsx` as the canonical visual/interaction pattern.

## Required Structure

Every right-pane editor must keep this order:

1. Header (`px-4 py-3 border-b`)
   - Left: entity/domain title
   - Left: the real version and lifecycle of the selected set: `Version {n}` + `StatusChip`
     (`REFERENCE_SET_STATUS_TONE`, translated status), and the publication date via `useFormatters`
     - Category B (working catalogue) shows "Working catalogue" instead of a version
     - Synthetic domains (live tables of other modules) show "Live table", never a version
   - Right: primary create action (`variant="outline" size="sm" className="gap-1.5"`)
2. Error banner (`px-4 py-2`, red background, `AlertTriangle` icon)
3. Table area (`flex-1 overflow-auto`)
   - Sticky table header (`bg-surface-0 border-b z-10`)
   - Inline create row (highlighted `bg-primary/5`)
   - Inline edit rows
4. Optional bottom sections
   - pagination / alias panel / dialogs

## Table and Rows

- Header cells: `font-medium text-text-muted`
- Normal rows: `border-b border-surface-border hover:bg-surface-1`
- Editing row: add `bg-primary/5`
- Code column: monospace text when read-only
- Description column: muted, truncated style
- Status column: switch + translated label; set and row states use `StatusChip`, never raw codes

## Action Buttons

- Row actions use icon buttons of at least 36 px (`refTableIconButtonClass()`), each with an
  accessible name (`aria-label`)
- Icons should follow standard semantics:
  - save: `Check` with `text-status-success`
  - cancel: `X`
  - edit: `Pencil`
  - delete/deactivate: `Trash2` with `text-status-danger`

## Confirmation and Destructive Flows

- Do not use `window.confirm`.
- Use the design-system `ConfirmDialog` (through `ReferenceValueTable`'s `confirm` prop); its
  `onConfirm` returns the promise so the dialog stays open when the action fails.

## Real Data Rule

Any visible action in Reference Data must be backed by a real service call.
If an action is not implemented yet, do not render it.

## Review Checklist (before merge)

- `pnpm typecheck` passes
- lints clear on modified lookups components
- no placeholder/disabled-only buttons left in the pane
- all statuses shown as `StatusChip` with translated labels
- header shows the real version and status for versioned panes; no version for live tables
- a disabled or missing action always has a visible reason (banner or `title`)
- destructive actions use confirm dialog
