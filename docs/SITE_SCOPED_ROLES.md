# Site-scoped roles (step 6) — design

Status: **approved** (owner, 2026-10-17: every proposal of §6 as written; O4 built last, together
with the Step 7 re-validation on arrival, its server part deployed only after the owner's separate
OK). Delivered stage by stage (§5); stage 1 = resolver, sign-in, user creation, rights per node.
This is the "transitive scope resolution" left for Phase 2 in `RBAC_CONTRACTS.md` §Scope Resolution.

## 1. Today

| Piece | Where | What it does |
|---|---|---|
| Role assignments | `user_scope_assignments` (`scope_type` tenant / entity / site / team / org_node, `scope_reference` = node id as text) | The admin dialog (`AssignRoleDialog` / `ScopeNodeSelect`) writes `tenant` (plant) or `org_node` + node id. Never `site` / `entity`. |
| Permission check | `auth/rbac.rs` `check_permission` (`:56`), `require_permission!` (`auth/mod.rs:76`) | All ~880 module calls pass `PermissionScope::Global`, which filters `scope_type = 'tenant'` (`rbac.rs:75-79`, `:204-208`). **This is why a site role grants nothing.** |
| Hierarchy-aware resolver | `rbac/resolver.rs` `effective_permissions_for_node` (`:215`), `user_has_permission_at_node` (`:279`), `rbac/scope_chain.rs` | Exists and is tested; **never called by modules**. |
| Login | `session_manager.rs:334` `resolve_tenant_scope_for_user`, `commands/auth.rs:191` | Refuses a user with no plant-wide assignment. `create_user` always adds one. So every user today is plant-wide for some role. |
| Frontend rights | `get_my_permissions` → `get_user_permissions` (`rbac.rs:294`) | Returns the union of **all** scopes (no filter): the UI shows actions the backend then refuses. |
| Already scope-aware | `notifications::inbox::visible_to`, `notifications::router::permission_holder_user_ids`, activity feed, audit log view | Notifications already filter by the reader's scoped assignments (subtree of `org_node_id`). |
| Sync arrival | `sync/materialize.rs` → `row_codec::apply_row` | No permission check; **the acting user is not known** at arrival (not in the envelope, `origin_machine_id` stays local). |

## 2. Rules (proposed)

1. **Plant-wide role** (`tenant`): grants everywhere, as today.
2. **Role on a node** (`org_node`, also `site` / `entity` if ever written): grants on that node **and every node below it** (its subtree). No guessing of "the site" from node-type flags: the admin picks the node, the subtree is the scope.
   - Why not `can_own_work`: the flag is set on several nested levels (root, zones, lines in the bootstrap; not on SITE in the demo seed), so "the site" would be ambiguous. The assigned node is not.
3. **Record anchor**: every scoped record resolves to one org node (its *anchor*, §4). A site-scoped user may act on / see a record only when its anchor is inside one of their granted subtrees **for that permission**.
4. **No anchor** (record without a derivable node): refused for site-only users, **visibly** — actions return a coded refusal `SCOPE_RECORD_WITHOUT_SITE`; lists return the visible rows plus counts `hidden_outside_scope` / `hidden_without_site` shown as a banner ("N records are not shown: outside your sites / without a site — ask a plant-wide user"). Never silent.
5. **Lists are filtered in SQL**, not after paging (counts and pages stay right).
6. **Creating a record** requires the permission at the new record's anchor (e.g. creating a DI on equipment of another site is refused).
7. **Moving a record** (changing its node / equipment) requires the permission at the old **and** new anchor.
8. **Analytics, KPIs, home, reports and exports** run on the user's scope for the module's view permission: a site user gets their subtrees only; an explicit org filter is intersected with it. Plant-wide figures are never shown to a site-only user.
9. **Administration** stays plant-wide: a site-scoped Administrator does **not** elevate (unchanged, `elevated_admin_sql`). Granting roles, settings, reference data, users remain plant-wide rights.
10. **Sync arrival**: a change made under a site role is re-validated on arrival against the actor's scope at the receiving PC; a refused change becomes a sync conflict for review (needs §6 O4).

## 3. One shared resolver — `auth::scope`

```text
enum AccessScope { Everywhere, Subtrees(BTreeSet<i64>), Nothing }

access_scope(db, cache, user, perm) -> AccessScope
    // tenant grant (or elevated admin) → Everywhere
    // else the nodes of in-force scoped assignments whose role carries `perm` → Subtrees
    // (built on rbac::resolver; cached per (user, perm) with the existing PermissionCache rules)

require_permission_at!(state, user, perm, anchor: Anchor)       // actions
    // Anchor::Node(id): Everywhere, or id's ancestor chain ∩ Subtrees ≠ ∅  (rbac::scope_chain)
    // Anchor::None: Everywhere only, else SCOPE_RECORD_WITHOUT_SITE
    // Anchor::Plant: Everywhere only (plant-wide objects: settings, reference, warehouses if O2=no)

scope_filter_sql(&AccessScope, anchor_expr: &str) -> (String, Vec<Value>)   // lists, analytics
    // Everywhere → "1=1"; Nothing → "0=1";
    // Subtrees → "<anchor_expr> IN (WITH RECURSIVE sub(id) AS (… roots … UNION ALL children) SELECT id FROM sub)"
    // plus a companion count query for the hidden-rows banner

anchor_of(db, RecordRef) -> Anchor      // one table, §4; used by actions and by sync arrival
```

`require_permission!(…, PermissionScope::Global)` stays for plant-wide objects; module record actions move to `require_permission_at!`. The frontend gets `get_my_access` = `{ everywhere: [perm], by_node: { node_id: [perm] } }` and `can(perm, nodeId?)`; `can(perm)` without a node means "anywhere" (to show a module), the backend is the authority.

## 4. Record anchors (module checklist)

| Module / record | Anchor (org node) | No anchor when | List to filter (cmd) | Action checks |
|---|---|---|---|---|
| Equipment | `installed_at_node_id` | node NULL | `list_assets`, `search_assets(_page)` (already subtree-capable) | create/move/decommission at node |
| Work orders | `entity_id` (copied from equipment at create) | entity NULL | `list_wo` (`WoListFilter`) | every WO action at `entity_id` |
| DI (requests) | `org_node_id` | `0` / missing node (create skips the check at 0 — fix) | `list_intervention_requests` **(today shows every site)** | review/approve/convert at node |
| PM plans | `asset_scope_type`: `equipment` → its node; `location` → that node; `family` / `criticality_group` → **multi-site** (O3) | family/criticality plans | `list_pm_plans` (filter ignored today) | edit at anchor; multi-site → plant-wide only |
| PM occurrences | `equipment_id` → node, else the plan's anchor | both missing | `list_pm_occurrences` | at anchor |
| Permits (+ isolations, LOTO) | `entity_id` (NOT NULL) | — | `list_work_permits`, `list_open_permits_report` | issue/activate/close at `entity_id`; isolations via permit |
| Planning candidates / commitments | the source record's anchor (WO / PM occ. / DI) | source without anchor | `list_schedule_candidates`, commitments | commit/break-in at source anchor |
| Weekly schedules | `entity_id` (the site) | — | `get_weekly_schedule(site, …)` | approve/freeze/unfreeze at site |
| Planning windows, capacity rules | `entity_id` (NULL = plant-wide) | NULL → plant-wide object | `list_planning_windows`, `list_capacity_rules` | NULL ones: plant-wide right |
| Teams | `primary_node_id` | NULL (crews without site) | `list_active_teams` (subtree-capable) | at node |
| Personnel (+ certifications, matrix) | `primary_entity_id` | NULL | `list_personnel`, qualification matrix | at node; **personal data stays behind `per.sensitiveview`** |
| Inspections (templates, rounds, results, anomalies) | template `org_scope_id`; checkpoints `asset_id` → node | `org_scope_id` NULL | rounds page, anomalies | at anchor |
| Inventory: warehouses, locations, balances, issues | **no org column** (O2) | always today | stock / balances / issues | O2 |
| Requisitions / purchase orders | demand WO's `entity_id` when `demand_source_type = 'WORK_ORDER'`, else warehouse (O2) | stock replenishment without warehouse anchor | requisitions, POs | at anchor |
| Finance | cost center `entity_id`; actuals via `work_order_id` / `equipment_id` | NULL | cost centers, actuals, settlements | at anchor; budgets plant-wide (O5) |
| Notifications | `org_node_id` (already scoped) | NULL → shown (keep) | inbox | — |
| Global (no scoping) | articles, suppliers, prices, units, FX, reference data, permit/certification types, positions, users, roles | — | — | plant-wide rights as today |

Cross-cutting surfaces:

| Surface | Change |
|---|---|
| Home (my work / supervision / planning) | supervision and planning panels take the user's scope; "my work" stays per user |
| Analytics executive dashboard, overview KPIs, workload/DI charts | scope intersected with the requested org; **org filter becomes subtree** (today exact node — O6) |
| Reports & exports (monthly, open WO, dashboard summary, workforce CSV, valuation, Gantt PDF, budget pack, ERP exports) | run on the requesting user's scope; scheduled runs re-check the owner's rights and scope at run time (today: no re-check) |
| Audit / admin exports | plant-wide admin rights, unchanged |
| Sync arrival | O4 |

## 5. Delivery order (after review)

1. `auth::scope` resolver + `require_permission_at!` + `scope_filter_sql` + `get_my_access` / `can(perm, node)`; login rule (O1); tests on the org tree (subtree, sibling, no anchor, plant-wide).
2. Modules in risk order: **DI, WO, permits/LOTO**, PM, planning, equipment, inspections, personnel, finance, inventory (after O2). Each: list SQL filter + hidden-rows banner, action checks at anchor, tests (site user sees own site, not sibling; no-anchor refused visibly; plant user unchanged).
3. Analytics, home, reports, exports, scheduled reports.
4. Sync arrival re-validation (after O4).

## 6. Needs an owner decision / changes stored data

| # | Question | Proposal | Stored data? |
|---|---|---|---|
| O1 | May a user have **only** site roles (today login needs a plant-wide one, and every user has one)? | Yes: login accepts any in-force assignment; `create_user` stops adding a plant-wide role when a site is chosen. Existing users keep their plant-wide role (nothing changes until an admin removes it). | No schema change; user creation behaviour changes |
| O2 | Warehouses have no site. | Add nullable `warehouses.entity_id` (synced; NULL = plant-wide store, visible to site users read-only for stock availability, writable by plant-wide users only). Admin sets it in the warehouse screen. | **Yes: new column, server sync allowlist** |
| O3 | PM plans by family / criticality group span sites. | Plan = plant-wide object (site users read it, cannot edit); its occurrences are scoped by equipment. | No |
| O4 | Sync re-validation needs to know **who** made a remote change; today nothing carries the actor. | Add `actor_user_sync_id` to the outbox envelope (wire + server pass-through), re-check `require_permission_at` on arrival, refusal → sync conflict. Ties in with the approved Step 7 item (permits/qualifications re-validated on arrival). | **Yes: sync envelope + server change** |
| O5 | Budgets / cost centers without a node. | Plant-wide objects (finance planners are plant-wide). | No |
| O6 | Analytics org filter is exact-node today. | Switch to subtree (a site includes its lines/zones). **Figures for a chosen node change** (they grow to include children). | No (figure definition change — tell users) |
| O7 | DI may be created with `org_node_id = 0`. | Refuse at create; existing 0 rows flagged "to review" (no site). | Data review of existing rows |
| O8 | Frontend `can(perm)` today returns site-scoped rights as if plant-wide. | `get_my_access` per node; module visibility = right held anywhere. | No |

## 7. Not in scope

- Site partitioning of sync (every PC still receives every site's rows; scoping is at read/act time).
- `work_orders.location_id` (meaning unclear; WO anchor is `entity_id`).
