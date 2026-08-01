# UX Pattern: Identification / QR Label Dialog

> **Status:** Adopted  
> **Applies to:** Asset, Work Order, Inventory, and any printable entity identity label  
> **Reference implementation:** `src/components/assets/AssetQrCode.tsx`  
> **Primitives:** `src/components/identification/*`  
> **Tokens:** `mfIdentification` in `src/design-system/tokens.ts`  
> **Export / print:** [`docs/UX_EXPORT_SYSTEM.md`](UX_EXPORT_SYSTEM.md) (`src/export/`)

---

## Decision

Entity identification labels (QR today, barcode later) use a **portaled Radix Dialog**, never an absolute popover inside a detail panel. The dialog must escape parent stacking contexts (`overflow`, `transform`, `isolation`) so page content cannot bleed through.

Export and print use the **official Maintafox export system** — not module-local `window.open` / QR-only canvas hacks.

Detail dialogs remain under `docs/UX_DETAIL_DIALOG_PATTERN.md`.  
Create/Edit forms remain under `docs/UX_ENTITY_FORM_DIALOG_PATTERN.md`.

---

## Specification

| Property | Value |
|----------|-------|
| Primitive | `@radix-ui/react-dialog` via `IdentificationLabelDialog` |
| Portal | Yes (`DialogContent` → `document.body`) |
| Max width | `max-w-md` / `sm:max-w-lg` |
| Max height | `max-h-[90vh]` |
| Backdrop | `mfModal.overlay` |
| Content background | Opaque `bg-surface-1` (`mfModal.content`) |
| QR presentation | Borderless — QR sits directly on the dialog surface |
| Layout | Centered: logo → company → QR → code → name → metadata |
| Backdrop click | Closes (unlike Entity Form / Detail) |
| ESC / focus trap | Radix defaults |
| Footer | `ExportActions` — Close · Print · Download PNG |

---

## Required structure

1. **Header** — title + short scan hint  
2. **Body** — centered industrial label (no QR card / outline)  
3. **Footer** — `ExportActions` from `@/export`

---

## Rules

- Do not invent a QR-specific visual language — reuse `Dialog`, `mfIdentification`, and `ExportActions`.
- Encode payloads in the consumer (e.g. Asset keeps `maintafox://asset/{id}`).
- Pass entity-agnostic `IdentificationLabelContext`; do not hardcode Asset fields in the dialog shell.
- Reserve the barcode slot via `barcode` prop; do not add barcode libraries until a dedicated story.
- Never implement print via `window.open` or download via bare `data:` `<a>` — use `@/export`.

---

## Adoption checklist

1. Map entity → `IdentificationLabelContext` (`payload`, `primaryCode`, `name`, `metadata`).
2. Open `IdentificationLabelDialog` from a trigger button (permission-gated as needed).
3. Do **not** nest an absolute popover inside scrollable panels.
4. Export via `ExportActions` / `exportDocument` (see UX_EXPORT_SYSTEM).
