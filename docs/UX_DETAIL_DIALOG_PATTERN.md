# UX Pattern: Entity Detail Dialog (UX-DW-001)

> **Status:** Adopted — S4 Sprint  
> **Applies to:** DI (Intervention Requests), WO (Work Orders), Equipment, Archive explorer  
> **First implemented:** `src/components/di/DiDetailDialog.tsx`

---

## Problem

Using a **side panel** for entity detail views causes the following UX issues
on the desktop app:

1. **Content squish** — the main workspace (Kanban board, DataTable) is
   compressed to ~55 % width, making columns and cards unreadable.
2. **No close affordance** — users have no visible button to dismiss the panel.
3. **Lost context** — the underlying view is obscured, preventing quick
   multi-entity comparisons.

## Decision

All entity detail views use a **centered floating dialog** (Radix UI Dialog
primitive) instead of a side panel. The dialog renders as a modal overlay;
the underlying workspace remains fully visible behind it and is not resized.

## Specification

| Property | Value |
|----------|-------|
| Primitive | `@radix-ui/react-dialog` via `src/components/ui/dialog.tsx` |
| Max width | `max-w-3xl` (768 px) |
| Max height | `max-h-[85vh]` |
| Layout | `flex flex-col` — header / scrollable body / footer |
| Backdrop click | Prevented (`onPointerDownOutside → e.preventDefault()`) |
| Close affordances | Header × button • Footer "Close" button |
| Keyboard | `Escape` closes the dialog (Radix default) |

### Header

- Entity code (monospace) + title (truncated)
- Status badge + safety/urgency badges
- Radix `DialogClose` × button (top-right)

### Scrollable Body

- **Info card** — grid of metadata fields (dates, reporter, priority, origin,
  asset, org node, impact flags)
- **Tab panel** — module-specific tabs (Attachments, Audit Trail, etc.)

### Footer

- Right-aligned action buttons (Close, Approve, Reject, Convert to WO…)
- Buttons depend on user permissions and entity state

## Implementation Checklist

| Module | Component | Store action | Status |
|--------|-----------|------------|--------|
| DI | `DiDetailDialog.tsx` | `closeDi()` | ✅ Done |
| WO | `WoDetailDialog.tsx` | `closeWo()` | ⬜ Planned |
| Equipment | `AssetDetailDialog.tsx` | `closeAsset()` | ⬜ Planned |
| Archive | `ArchiveDetailDialog.tsx` | — | ⬜ Planned |

## File References

- Pattern source: `src/components/di/DiDetailDialog.tsx`
- Dialog primitive: `src/components/ui/dialog.tsx`
- DI store: `src/stores/di-store.ts` (`closeDi` action)
- DI page: `src/pages/RequestsPage.tsx` (dialog wired, side panel removed)

## Migration Guide (for existing side-panel code)

1. Create `<EntityDetailDialog>` component using the Dialog primitive.
2. Add a `closeEntity()` action to the Zustand store (`set({ activeEntity: null })`).
3. In the page component:
   - Remove the conditional `w-[55%]` / `flex-1` side-panel split.
   - Render `<EntityDetailDialog open={!!activeEntity} onClose={closeEntity} />`.
4. Add i18n keys: `detail.close`, `detail.safety`, `detail.fields.*`.
5. Update tests to mock the Dialog overlay instead of inline panel.
