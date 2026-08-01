# Reference Data UI Consistency

This guideline defines the required UI structure for all right-pane editors in `Données de référence`.

## Canonical Pattern

Use `src/components/lookups/ReferenceValueEditor.tsx` as the canonical visual/interaction pattern.

## Required Structure

Every right-pane editor must keep this order:

1. Header (`px-4 py-3 border-b`)
   - Left: entity/domain title
   - Left: set lifecycle badge when the pane is set-backed (`v{n} — {status}`)
     - Example: `v1 — published`
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
- Status column: use `Badge`, not plain text
  - active: `Badge variant="default" className="text-[10px]"`
  - inactive/new: `Badge variant="secondary" className="text-[10px]"`

## Action Buttons

- Row actions must use compact icon buttons:
  - `variant="ghost" size="icon" className="h-6 w-6"`
- Icons should follow standard semantics:
  - save: `Check` with `text-status-success`
  - cancel: `X`
  - edit: `Pencil`
  - delete/deactivate: `Trash2` with `text-status-danger`

## Confirmation and Destructive Flows

- Do not use `window.confirm`.
- Use dialog-based confirmation (`Dialog`, `DialogHeader`, `DialogFooter`).
- Confirm button must be `variant="destructive"`.

## Real Data Rule

Any visible action in Reference Data must be backed by a real service call.
If an action is not implemented yet, do not render it.

## Review Checklist (before merge)

- `pnpm typecheck` passes
- lints clear on modified lookups components
- no placeholder/disabled-only buttons left in the pane
- all statuses shown as badges, not plain text
- header contains set badge (`v{n} — {status}`) for set-backed panes
- destructive actions use confirm dialog
