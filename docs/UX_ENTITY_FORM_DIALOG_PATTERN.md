# UX Pattern: Entity Create / Edit Dialog (Maintafox Enterprise Dialog Standard)

> **Status:** Adopted  
> **Applies to:** All Create and Edit forms (Asset, Work Order, PM, Inventory, Org, Reference managers, …)  
> **Reference implementation:** `src/components/assets/AssetCreateForm.tsx`, `AssetEditForm.tsx`  
> **Primitives:** `src/components/entity-form/*`  
> **Tokens:** `mfEntityForm` in `src/design-system/tokens.ts`

---

## Decision

Every Create/Edit surface uses the same **Entity Form Dialog** shell: header, scrollable sectioned body, sticky footer. Do not invent per-module layouts.

Detail dialogs remain under `docs/UX_DETAIL_DIALOG_PATTERN.md` (UX-DW-001).

---

## Specification

| Property | Value |
|----------|-------|
| Primitive | Radix Dialog via `EntityFormDialog` |
| Max width | `max-w-2xl` (`mfEntityForm.dialog`); use `dialogWide` only when justified |
| Max height | `max-h-[90vh]` |
| Layout | Flex column — header / scroll body / sticky footer |
| Backdrop click | Prevented |
| Close | Header × · Cancel · Escape |
| Footer buttons | Cancel (outline) then Primary (explicit label) |

---

## Required structure

1. **Header** — title + short subtitle  
2. **Body** — `EntityFormSection` blocks (Identification, Classification, …)  
3. **Field groups** — cascading fields inside `EntityFormFieldGroup`  
4. **RAMS / Advanced** — `EntityFormCollapsible` default collapsed  
5. **Images / Attachments** — `EntityFormImageUploader` / `EntityFormAttachments`  
6. **Footer** — Cancel + primary action with entity-specific label  

---

## Rules

- Reference-backed fields use `ReferenceCombobox` (see `.cursor/rules/reference-combobox.mdc`).
- Validation messages are localized enterprise copy (e.g. “Ce champ est obligatoire.”), never raw `"required"`.
- Minimize mandatory fields to business-critical ones.
- Do not hardcode Asset chrome classes in other modules — import entity-form components.

---

## Adoption checklist (new dialog)

1. Wrap with `EntityFormDialog`.
2. Split fields into `EntityFormSection`s.
3. Use `EntityFormFieldGroup` for dependent cascades.
4. Put advanced blocks in `EntityFormCollapsible`.
5. Reuse media components when the entity supports files.
6. Primary button label names the entity (“Créer un actif”, not “Créer”).
