# Reference Governance — Implementation Plan

**Status:** Phase 1 complete · PR2 Manager complete · **PR3 Reference Experience Unification complete** (`Target` active)  
**Depends on:** [REFERENCE_GOVERNANCE.md](./REFERENCE_GOVERNANCE.md)  
**Goal:** Migrate Maintafox from the inconsistent draft-vs-operational split to the A/B/C governance model with one write path per category.

### Phase 1 landed

- Module [`governance.rs`](../../src-tauri/src/reference/governance.rs) is the unique permission decision engine.
- Column `governance_category` on `reference_domains` (migration `m20260807_000123`).
- Value create/update/deactivate/move, operational create, draft-set create, publish, and analytical protection call sites route through governance asserts.
- Capabilities DTO + IPC `get_reference_governance_capabilities` / `_by_code`.
- Unit tests: `governance_tests.rs`.

### PR2 / Phase 3 Manager landed

- Reference Manager loads a capability snapshot and only renders allowed actions (no `is_extendable` / `governance_level` / UI remap gates).
- Surfaces wired: DomainBrowserPanel, ReferenceValueEditor, PublishReadinessPanel, ReferenceAliasPanel, Import.

### PR3 / Reference Experience Unification landed

- `ACTIVE_ENFORCEMENT_PHASE = Target` — B live CRUD on published; B draft/publish forbidden; C unchanged; A read-only.
- `ReferenceCombobox` creates only when `can_operational_create` (+ `ref.manage`); **`allowOperationalCreate` removed**.
- `valueMode: "code" | "id"`; empty-state primary create CTA for B.
- DiCreateForm origin / symptom / priority / impact use Combobox; `DiLookupManagerDialog` removed.
- Export remains `ref.view` only (not capability-gated).

**Remaining follow-ups:** schema drop of legacy `governance_level` / `is_extendable` columns; other form migrations beyond Skills.

### Phase 5.1 — FAILURE_MODES wipe seed (landed)

- [`system_catalog_integrity.rs`](../../src-tauri/src/reference/system_catalog_integrity.rs) seeds a controlled-catalog baseline for `WORK.FAILURE_MODES` **independent of** `failure_codes` (kept as additive merge).
- Regression: `system_catalog_integrity_tests.rs` — empty `failure_codes` + cleared values → integrity restores ≥7 published active modes.

### Target smoke checklist (manual)

After activation wipe or on a Target build, verify:

1. **A** (CLASS / CRITICALITY / STATUS / DI.PRIORITY): no Add/Edit/Delete/Draft/Publish in Manager; Combobox no create.
2. **B FAMILY published**: Add/Edit/Deactivate work; **no** “Nouveau jeu”; no published read-only draft banner.
3. **B Combobox** (Asset famille / DI symptôme): empty list shows create CTA with `ref.manage`; create selects new value.
4. **C FAILURE_MODES published**: no mutate; draft create available; publish with `ref.publish`.
5. **FAILURE_MODES not empty** after wipe: ≥ baseline modes (VIBRATION, …) in Manager / reliability lists.
6. Export still works with `ref.view` only (if UI/API exercised).

---

## Outcomes

1. `reference_domains` store a single `governance_category` ∈ `system_catalog` | `operational_dictionary` | `controlled_catalog` (A/B/C).
2. Backend enforces category rules; UI never offers illegal actions.
3. Manager and ReferenceCombobox share the same B live-CRUD API.
4. C retains draft → validate → publish; B has no draft/publish chrome.
5. Domains remapped per the accepted philosophy table.
6. Docs and Cursor rules match the new model.

---

## Phase 0 — Inventory lock (read-only checks)

Confirm current codes and call sites before mutating schema:

- Domains: integrity + migrations (`EQUIPMENT.*`, `DI.*`, `PERSONNEL.SKILLS`, `WORK.FAILURE_MODES`, `PM.MAINTENANCE_TASK_LIST`)
- Write paths: [`values.rs`](../../src-tauri/src/reference/values.rs) `create_value` / `create_operational_value` / update / deactivate
- Publish: [`sets.rs`](../../src-tauri/src/reference/sets.rs), [`publish.rs`](../../src-tauri/src/reference/publish.rs)
- UI: [`ReferenceValueEditor.tsx`](../../src/components/lookups/ReferenceValueEditor.tsx), [`DomainBrowserPanel.tsx`](../../src/components/lookups/DomainBrowserPanel.tsx), [`ReferenceCombobox.tsx`](../../src/components/reference/ReferenceCombobox.tsx)
- Remap helper: [`reference-domain-ui.ts`](../../src/lib/reference-domain-ui.ts)

---

## Phase 1 — Schema & domain remapping

### 1.1 Migration

Add `governance_category` (TEXT NOT NULL) on `reference_domains`, backfill:

| Legacy | New category |
|--------|----------------|
| `system_seeded` + EQUIPMENT.CLASS/CRITICALITY/STATUS | `system_catalog` |
| `protected_analytical` (DI.PRIORITY, DI.IMPACT_LEVEL) | `system_catalog` |
| `tenant_managed` FAMILY/SUBFAMILY/SYMPTOM/ORIGIN/SKILLS | `operational_dictionary` |
| `WORK.FAILURE_MODES` (was system_seeded) | `controlled_catalog` |
| `PM.MAINTENANCE_TASK_LIST` | `controlled_catalog` |

Then:

- Stop writing `governance_level` / treat as deprecated mirror **or** migrate column rename in a follow-up migration (prefer: new column → cutover reads → drop old column in later migration).
- Set `is_extendable` from category for compatibility during transition: A → 0; B/C → 1.
- Ensure every B/C domain has exactly one working **published** set after integrity (B: only that set shown; C: published + optional draft).

### 1.2 Integrity updates

Update [`system_catalog_integrity.rs`](../../src-tauri/src/reference/system_catalog_integrity.rs), [`taxonomy_reference.rs`](../../src-tauri/src/assets/taxonomy_reference.rs):

- Create/upsert domains with `governance_category`.
- **A:** upsert baseline values; tenant cannot mutate.
- **B:** seed baseline once if empty; thereafter tenant-owned (do not wipe tenant rows on every startup).
- **C:** ensure domain + published v1 baseline for `WORK.FAILURE_MODES` **without** depending solely on empty `failure_codes` after wipe (fix empty-after-activation gap).
- `PM.MAINTENANCE_TASK_LIST`: create as C when first used; drafts clone-from-published when introducing version UX.

### 1.3 IPC / shared types

- Extend `ReferenceDomain` in [`shared/ipc-types.ts`](../../shared/ipc-types.ts) + Rust DTO with `governance_category`.
- Map French/EN badges: Système / Dictionnaire opérationnel / Catalogue contrôlé.

---

## Phase 2 — Backend write paths (one API per category)

### 2.1 Category A

- Reject all value create/update/deactivate/move from tenant IPCs.
- List published only.

### 2.2 Category B — live dictionary service

Unify Manager + Combobox on one path (rename/clarify existing operational API):

- `create` / `update` / `deactivate` against the **working published set** only.
- Guards: category == `operational_dictionary`; label/code rules; parent rules; soft-deactivate when in use (no hard delete of referenced rows).
- **Forbid** draft set creation and publish IPCs for B domains (or no-op with clear error).
- Remove need for `assert_set_is_draft` on B writes.

Suggested shape (names illustrative):

- Keep IPC `create_operational_reference_value` as B-create **or** rename to `create_dictionary_value` and add `update_dictionary_value` / `deactivate_dictionary_value`.
- Manager must call these for B, **not** draft `create_reference_value`.

### 2.3 Category C — governed versioning only

- Value mutate **only** when set status is `draft`.
- Publish / validate / impact remain; gate with `ref.publish`.
- Forms list **published** values only; Combobox create disabled (unless explicit propose-to-draft is built later).
- ~~**Clone-from-published** when creating a new draft (required for usable C UX).~~ — **done** (`create_draft_set` clones values/aliases/hierarchy in one txn; bootstrap empty only when no published set).
- ~~**Discard draft**~~ — **done** (`discard_draft_set` hard-delete + Manager CTA; `can_discard_draft_set`).
- Failure Modes: baseline published v1; edits go through draft → publish.

### 2.4 Delete dual peer semantics

| Old | New |
|-----|-----|
| `create_reference_value` on any draft | C drafts only (and never B/A) |
| `create_operational_reference_value` as bypass | **Definition** of B create |
| `is_extendable` independent checks | Derive from category |

---

## Phase 3 — Reference Manager UI — **COMPLETE (PR2)**

Files: `ReferenceValueEditor.tsx`, `DomainBrowserPanel.tsx`, `PublishReadinessPanel.tsx`, `reference-manager-store.ts`, `use-reference-capabilities.ts`, `reference-governance-ui.ts`.

Manager consumes `get_reference_governance_capabilities` and combines with `ref.manage` / `ref.publish`. Under **Target** (active):

| Category | Tree / badges | Value editor | Publish chrome |
|----------|---------------|--------------|----------------|
| A | Lock + “Système” | Read-only | Hide |
| B | “Dictionnaire” | Live mutate on published working set | Hide (no draft create) |
| C | “Catalogue contrôlé” | Mutate draft only; published → banner + draft CTA | Show on drafts if ready + `ref.publish` |

**Removed UI gates:** `is_extendable`, `governance_level` remap / `normalizeReferenceDomainForUi` permission side effects, `isReferenceDomainProtected` as mutate gate (analytical badge uses `requires_analytical_protection`).

---

## Phase 4 — Reference Experience Unification (PR3) — **COMPLETE**

Files: [`reference-types.ts`](../../src/components/reference/reference-types.ts), [`ReferenceCombobox.tsx`](../../src/components/reference/ReferenceCombobox.tsx), [`ReferenceCreateModal.tsx`](../../src/components/reference/ReferenceCreateModal.tsx), [`DiCreateForm.tsx`](../../src/components/di/DiCreateForm.tsx).

- Create eligibility from `can_operational_create` (capabilities IPC by domain code); **`allowOperationalCreate` deleted**.
- A/C: never show create CTA; B: operational create + empty-state primary CTA.
- `valueMode: "code" | "id"` (DI symptom uses id).
- DiCreateForm: origin / symptom / priority / impact on Combobox.
- `ACTIVE_ENFORCEMENT_PHASE = Target` (architecture test: Manager needed no structural rewrite).
- Docs + Cursor rule updated; Export documented as view-only.

---

## Phase 5 — Data fixes & cleanup

1. ~~**FAILURE_MODES** after activation wipe: seed controlled-catalog baseline independently of empty `failure_codes`.~~ — **done**.
2. ~~Delete orphan DiLookupManagerDialog~~ — **done in PR3**.
3. Do not grow `lookup_*` for new reference fields.
4. ~~Align demo/RAMS seeds so FAILURE_MODES is not inserted as `tenant_managed` / extendable drift.~~ — **done** (`demo_seeder` + RAMS prerequisites write `governance_category`; FAILURE_MODES / CLASS = system_seeded non-extendable).
5. ~~Hide draft/version chrome for B when historical sets exist.~~ — **done** (`visibleSetsForDomain` + Manager auto-select published; breadcrumb omits vN/status for B).
6. ~~Skills form migration~~ — **done** (Profile → `ReferenceCombobox` `personnel.skills` / `valueMode=id`; PM loads published codes). Remaining form migrations still tracked as needed.
7. ~~C clone-from-published + discard draft~~ — **done**.

---

## Phase 6 — Validation

| Scenario | Expect |
|----------|--------|
| A domain in Manager | No add/edit/delete; Combobox no create |
| B Family create from Combobox | Immediate select; same value editable/deactivatable in Manager |
| B Manager edit on working catalog | Succeeds (no “draft only” error) |
| C Failure Modes | Cannot edit published; draft clone → edit → validate → publish |
| User without `ref.manage` | Dropdown works; no create |
| User without `ref.publish` | Cannot publish C |
| Activation wipe | A/B/C baselines restored; FAILURE_MODES not empty |
| `cargo check` + TS typecheck | Clean for touched modules |

---

## Out of scope (this implementation cut)

- Flattening B to domain→values without sets (B2)
- Unifying `lookup_*` and WO synthetics into `reference_domains`
- Cloud multi-tenant `tenant_id` / sync conflict strategy
- Combobox “propose to draft” for C
- French marketing copy polish beyond badge keys

---

## Suggested PR sequence

1. **Schema + backfill + integrity category** — done (Phase 1)
2. **Backend governance engine + capabilities IPC** — done
3. **Manager category chrome (PR2)** — done
4. **Reference Experience Unification (PR3)** — **done** (Combobox + Target + DI forms)
5. **FAILURE_MODES seed fix + remaining form migrations** — remaining

---

## Definition of done

- [ ] Philosophy doc still accurate vs code
- [ ] Every production domain has exactly one `governance_category`
- [ ] No UI path offers mutate that backend rejects for that category
- [ ] B create/edit/deactivate identical from Combobox and Manager
- [ ] C publish path only for controlled catalogs
- [ ] Cursor rule + REFERENCE_COMBOBOX.md updated
