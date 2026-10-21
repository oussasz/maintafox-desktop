# Reference Governance Philosophy (Accepted)

**Status:** Accepted product philosophy  
**Date:** 2026-07-25  
**Scope:** Données de référence (`reference_domains` / sets / values)  
**Backend SSOT:** `src-tauri/src/reference/governance.rs`  
**Implementation:** See [REFERENCE_GOVERNANCE_IMPLEMENTATION_PLAN.md](./REFERENCE_GOVERNANCE_IMPLEMENTATION_PLAN.md)

This document is the single source of truth for how reference domains are governed. Every UI surface and backend service must derive behavior from **exactly one** governance category per domain.

## Backend decision engine (current)

Permissions are decided only by `crate::reference::governance`:

| API | Purpose |
|-----|---------|
| `resolve_category` / `category_of` | A/B/C classification |
| `allows_operational_create` | Combobox / live dictionary create |
| `allows_value_mutation` | Manager create/update/deactivate/move |
| `allows_create_draft_set` | New draft set |
| `allows_publish` | Publish path |
| `requires_analytical_protection` | Protected delete / impact semantics |
| `capabilities_for` / IPC `get_reference_governance_capabilities` (+ `_by_code`) | Serializable snapshot for Manager + Combobox |

**Enforcement phase:** `ACTIVE_ENFORCEMENT_PHASE = Target` — Category **A** read-only; **B** live CRUD on published working catalog (no draft/publish); **C** draft → publish. Compat B “published → create draft” path is retired.

**Manager / Combobox:** Must not decide permissions locally. Both load capabilities via IPC (`can_*` flags) and combine with `ref.manage` / `ref.publish`. Create-from-dropdown uses `can_operational_create` only — never registry booleans.

**Export:** `export_ref_domain_set` is view-only (`ref.view`). It does **not** need category capability gates (unlike Import / mutate / publish).

For the product domains of the canonical mapping below, the mapping in `governance.rs` wins over
the stored column (seeders keep that column aligned); tenant-created domains use the stored value.

**Product-seeded values** (`metadata_json.origin = "system"`) of the equipment taxonomy domains are
locked (`system_values_locked`): they cannot be edited, deactivated or moved, because the startup
integrity re-applies them. Tenants add their own values and use aliases for local wording.

**Publish keeps links:** publishing a version re-points every record (foreign keys to
`reference_values`, and parents in other domains) from the earlier versions to the value with the
same code (`publish::remap_usage_to_published_set`).

Column `reference_domains.governance_category` stores the category; legacy `governance_level` / `is_extendable` remain for compatibility and must not be used for new permission branches.

---

## Categories

| Code | Name | Owner | Mutability | Versioning |
|------|------|-------|------------|------------|
| **A** | System Catalog | Product | Read-only for tenants | Single product-owned published set; updated only by upgrades / integrity |
| **B** | Operational Dictionary | Tenant | Live create / edit / soft-deactivate | **No** draft/publish UX; one working catalog |
| **C** | Controlled Business Catalog | Tenant | Mutate draft only | Draft → validate → publish → supersede; rollback via republish; impact analysis |

### Hard rules

1. A domain belongs to **exactly one** category.
2. Never show a button the backend will reject.
3. Category alone drives enablement (no parallel `is_extendable` / UI-only governance remaps once migrated).
4. Reference Manager and ReferenceCombobox obey the **same** category rules.
5. Near-term persistence for B: one hidden working set (`status=published`) with set/version UI hidden (schema flatten later).

### Audience: SYSTEM vs COMPANY (owner rule, migration 196)

Independent of A/B/C, every domain stores `reference_domains.audience`:

- **system** — a list shipped and maintained by Maintafox that the program's logic depends on
  (`EQUIPMENT.STATUS`, `EQUIPMENT.CRITICALITY`, `DI.DISPOSITION`). Not listed in Données de
  référence, and every user-facing write (domain, sets, values, aliases, imports, merge, publish)
  is refused with `GATE_REF_SYSTEM_DOMAIN_READ_ONLY`, whatever the category. Pickers, links and
  ids are unchanged. Only seeders (migrations, `system_catalog_integrity`, module seeders) write it.
- **company** — a list that differs per plant; shown in Données de référence and governed by its
  A/B/C category as above (A = visible but read-only).

Domains created by users are always `company`; seeders declare the audience of the domains they
create. The module tables shown in the tree (OT types/statuses/priorities, article families, tax
categories) get their audience from `reference::audience::MODULE_CATALOGS`
(`list_reference_module_catalogs`); OT types and statuses are `system`.

---

### Units of measure (`MEASURE.UNITS`, migration 201)

One list for every module: article stock unit, supplier price unit, both units of a conversion, equipment meter unit, inspection measurement unit (all linked by `reference_values.id`). It replaces the fixed lookup `inventory.unit_of_measure` and the free-text unit fields (`reference::units`).

- A value carries its FR label (`label`), EN label (`metadata_json.label_en`), symbol (`metadata_json.symbol`), dimension (`semantic_tag`, a code of `MEASURE.DIMENSIONS`, checked on the server) and UN/CEFACT Rec 20 code (`external_code`) where one exists.
- **Standard units** (kg, m, L, h, bar, °C, pieces…) are protected by the protected-value guard: no edit, deactivation or recode.
- **Plant units** name a base standard unit of the same dimension (`parent_id`) and a factor > 0 (`metadata_json.base_factor`: 1 bag = 25 kg), required and checked on the server (`GATE_UNIT_BASE_REQUIRED` / `GATE_UNIT_BASE_INVALID`).
- Units created by migration 201 for unknown codes or texts are flagged `review_required` (draft validation warning, "To review" filter in the editor); editing one makes base and factor required and clears the flag.
- Conversions: the article's own conversion, then the plant unit's general factor, then a general row between standard units; units of different dimensions never convert (`GATE_UNIT_DIMENSION_MISMATCH`), and a missing factor is never taken as 1 (`GATE_UNIT_FACTOR_MISSING`).
- Receipts and issues may be entered in another unit (migration 204): converted to the stock unit with the same order (`unit_conversions::to_stock_quantity`), rounded half away from zero to 6 decimals; serial-tracked articles refuse a non-whole result (`GATE_UNIT_WHOLE_UNITS`). The receipt line and the movement keep the entered unit, quantity and factor; PO quantities, balances, lots and valuation stay in the stock unit.
- Merging units (`reference::migrations`): standard units are never a source; a unit with a factor only merges into a unit with the same base and factor (`GATE_UNIT_MERGE_CHANGES_QUANTITIES`); records, conversions, meters, checkpoints and checkpoint packages follow. Publishing a new version also moves the unit ids inside checkpoint packages.
- Sync: a row of a device older than migration 201 naming a unit by the sync id of the retired lookup, or by a unit text, is linked on apply to the unit with that code / the same synonym rule (`units::unit_for_legacy_sync_id`, `units::upgrade_legacy_unit_payload`).

---

## Behavioral matrix

| Action | A | B | C |
|--------|---|---|---|
| List values in forms | Published only | Working catalog | Published only |
| Create from ReferenceCombobox | Never | Yes → live | Never (or explicit propose-to-draft only) |
| Edit / deactivate in Manager on working/published | Never | Yes (soft-deactivate if in use) | Never on published; Yes on draft |
| Draft / publish / rollback UI | No | No | Yes |
| Integrity may upsert baseline | Yes | Optional seed once | Optional v1 seed |
| Product upgrade may change values | Yes | No | No |

### Permissions

- `ref.view` — read
- `ref.manage` — B live mutate; C draft mutate
- `ref.publish` — C validate / publish only

---

## Canonical domain mapping

| Domain | Category |
|--------|----------|
| `EQUIPMENT.STATUS` | **A** |
| `EQUIPMENT.CRITICALITY` | **A** |
| `EQUIPMENT.CLASS` | **C** — product classes seeded and locked; each plant adds its own classes (presses, boilers, …) by draft → publish |
| `DI.PRIORITY` | **A** |
| `DI.IMPACT_LEVEL` | **A** |
| `DI.REQUEST_TYPE` | **A** — closed intake taxonomy (repair default) |
| `EQUIPMENT.FAMILY` | **B** |
| `EQUIPMENT.SUBFAMILY` | **B** |
| `DI.SYMPTOM` | **B** |
| `DI.ORIGIN` | **B** — open catalog; create/update validate published codes (not the closed `DiOriginType` seed enum) |
| `PERSONNEL.SKILLS` | **B** |
| `WORK.DELAY_REASONS` | **B** — operational wait/hold causes (parts, permit, vendor, …) |
| `WORK.PART_UNUSED_REASON` | **B** — why a planned part was not consumed during execution |
| `WORK.FAILURE_MODES` | **C** |
| `PM.MAINTENANCE_TASK_LIST` | **C** |
| `MEASURE.UNITS` | **C** — units of measure (COMPANY). Standard units seeded and protected (`origin = system`); each plant unit names a base standard unit and a factor (see below) |
| `MEASURE.DIMENSIONS` | **A** — physical dimensions of the units (SYSTEM, hidden): the conversion engine depends on them |

### Adjacent (same philosophy, not necessarily `reference_domains` yet)

| Surface | Suggested category |
|---------|-------------------|
| WO statuses | **A** |
| WO priorities / urgency | **A** |
| WO types | **B** or **A** (product choice when unified) |
| Inventory article family / tax | **B** |

Legacy `lookup_*` is parallel debt; do not grow it for new features.

---

## Replaces (after migration)

| Legacy signal | Fate |
|---------------|------|
| `governance_level`: `system_seeded`, `tenant_managed`, `protected_analytical`, `erp_synced` | Replaced by A / B / C |
| Independent `is_extendable` | Derived from category (A=false; B/C=true for tenant mutate paths) |
| UI remap in `reference-domain-ui.ts` | Unnecessary once CRITICALITY/STATUS are stored as A |
| Dual create APIs without category gates | One write path per category |

---

## Related docs

- [REFERENCE_COMBOBOX.md](./REFERENCE_COMBOBOX.md) — form control standard
- [REFERENCE_GOVERNANCE_IMPLEMENTATION_PLAN.md](./REFERENCE_GOVERNANCE_IMPLEMENTATION_PLAN.md) — migration execution plan (includes Target smoke checklist)
- Cursor rule: `.cursor/rules/reference-combobox.mdc`

**Integrity:** After activation wipe, `ensure_system_reference_catalog_integrity` restores A/B/C catalogs. `WORK.FAILURE_MODES` baseline values are seeded without depending on `failure_codes`.

## Manager UI shell

Reference Manager uses one presentational shell — [`ReferenceValueTable`](../../src/components/lookups/ReferenceValueTable.tsx) — for chrome (header, toolbar, banners, loading, empty state, confirm dialogs, table tokens).

| Surface | Role |
|---------|------|
| [`ReferenceValueEditor`](../../src/components/lookups/ReferenceValueEditor.tsx) | Real domains: maps IPC `ReferenceGovernanceCapabilities` + `ref.manage` / `ref.publish` into the shell |
| [`adapters/*Host.tsx`](../../src/components/lookups/adapters/) | Synthetic domains (−101…−105): map inventory/WO IPC + `ref.manage` into the same shell; **no** governance IPC |

Business rules stay in governance (real domains) or existing synthetic services (WO/inventory). The shell does not invent Category A read-only behavior for synthetics.
