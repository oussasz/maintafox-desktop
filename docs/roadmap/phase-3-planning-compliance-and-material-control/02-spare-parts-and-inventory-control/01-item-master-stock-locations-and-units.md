# Item Master Stock Locations And Units

**PRD:** §6.8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Master schema baseline** — Keep `articles`, `article_families`, `warehouses`, `stock_locations`, `stock_balances` with unique codes, FK integrity, and row-version fields where needed.
- **Reference-governed master data** — Enforce units, criticality, tax category, and optional procurement category from governed SP03/SP13 domains (no free-text master fields for governed dimensions).
- **Reference Data UX parity** — Manage VAT/TVA tax categories in `Données de référence` using the same governance UX pattern as stock article families (domain-tree entry, version badge, inline CRUD/deactivate).
- **Identity and lifecycle controls** — Add `is_active`, `created_at`, `updated_at`, and soft-deactivation rules for item families, locations, and articles used by active records.
- **Location topology rules** — Support warehouse default bins, active/inactive bin status, and uniqueness (`warehouse_id + code`) to prevent duplicate storage points.
- **Item master contracts** — Add validation for min/max/reorder relationships, stocking type, and preferred warehouse/location hints.
- **Inventory domain services** — Keep all item-master mutations in `src-tauri/src/inventory/` service/query layer with transactional guards (no direct UI-side assumptions).
- **Permissioned command boundary** — Expose read/write commands in `src-tauri/src/commands/inventory.rs` with `inv.view` vs `inv.manage` enforcement for every operation.
- **IPC and type safety** — Maintain synchronized Rust DTOs and `shared/ipc-types.ts` contracts, with Zod decode/validation in `src/services/inventory-service.ts`.
- **UI completeness** — Keep `src/pages/InventoryPage.tsx` item-master forms fully labeled, validated, and bound to real DB lists (families, units, criticality, locations).
- **Data quality acceptance checks** — Validate duplicate code rejection, stale row-version protection, inactive-family protections, and reference-domain mismatch rejection.

---

*Completion: 2026-04-14, verifier: Codex (Cursor agent), `cargo check` (pass in `src-tauri/`), `pnpm typecheck` (pass at workspace root).*
