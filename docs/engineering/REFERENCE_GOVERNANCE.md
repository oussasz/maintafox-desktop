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
| `EQUIPMENT.CLASS` | **A** |
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
