# ReferenceCombobox — developer guideline

Any field whose values are managed in **Données de référence** must use `ReferenceCombobox` instead of a raw `Select` / ad-hoc lookup.

**Governance:** See [REFERENCE_GOVERNANCE.md](./REFERENCE_GOVERNANCE.md) (accepted A/B/C philosophy).  
**Enforcement:** `ACTIVE_ENFORCEMENT_PHASE = Target` in `governance.rs`.

## Why

- One UX for searchable reference dropdowns (label-only options, optional create-in-place).
- **Category B** (Operational Dictionary): create-from-dropdown writes the working catalog immediately — same rules as Données de référence (`can_operational_create`).
- **Category A** (System Catalog): read-only; never create-from-dropdown.
- **Category C** (Controlled Business Catalog): forms use published values only; no silent live create (versioned draft → publish in Manager).

## Add a new reference-backed field

1. **Assign category** A, B, or C per [REFERENCE_GOVERNANCE.md](./REFERENCE_GOVERNANCE.md) and ensure the domain exists with that category.
2. **Register** a `referenceType` in [`src/components/reference/reference-types.ts`](../../src/components/reference/reference-types.ts) (domain code + labels only — **no** create flags).
3. **Drop in** `<ReferenceCombobox referenceType="…" value={…} onChange={…} />`.
4. Use `valueMode="id"` only when the form persists `reference_values.id` (e.g. DI symptom); default is `"code"`.
5. Create eligibility comes from **governance capabilities** (`get_reference_governance_capabilities_by_code` → `can_operational_create`) + `ref.manage`. Never hardcode category in the form.

## Architecture pieces

| Piece | Role |
|-------|------|
| `reference-types.ts` | Typed registry of domains and UX copy (no permission flags) |
| `ReferenceCombobox.tsx` | Searchable combobox + empty CTA + create entry (B only via capabilities) |
| `ReferenceCreateModal.tsx` | Create dialog → `create_operational_reference_value` |
| Capabilities IPC | SSOT for create / Manager actions |

## Empty state

When the list is empty and create is allowed: show empty copy **and** a primary “Créer …” button — not a dead empty dropdown.

## Permission

Create-from-dropdown is gated by `can_operational_create` and `ref.manage`. Users without permission still get a normal searchable dropdown.

## Export vs Import

- **Import / mutate / publish / aliases** → capability + manage/publish permissions.
- **Export / list / search** → `ref.view` only; no category capability gate.

## Do not

- Call draft-only CRUD against published sets for Category C from the Combobox.
- Offer create on Category A or C.
- Reintroduce `allowOperationalCreate` or other registry permission flags.
- Show tenant/tech IDs in dropdown labels — show **label only**.
- Grow `lookup_*` for new reference fields.
