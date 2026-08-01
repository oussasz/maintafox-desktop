# Spare parts & inventory — tester feedback checklist

Legend: **Done** · **Partial** · **Open** · **N/A / unclear**

_Last synced with codebase: inventory topology CRUD, procurement UX, WO valuation, cycle-count upsert refresh._

---

## A — Master data (warehouses, families, articles)

| Status | Item | Notes |
|--------|------|--------|
| **Done** | A1 | **Warehouses & locations:** tab **“Warehouses & locations”** on Inventory; CRUD via IPC (`create`/`update` warehouse and stock location). Edit uses modals; prefer **deactivate** (active flag) over delete when referenced. |
| **Done** | A2 | Families work. |
| **Done** | A3 | Article creation works. |
| **Done** | A4 | Article editing works. |

---

## B — Stock balances tab

| Status | Item | Notes |
|--------|------|--------|
| **Done** | B1–B4 | **`list_stock_balances`** returns **synthetic zero-qty rows** for active articles without a `stock_balances` row yet (preferred location, else first default active bin). Re-test filters and “low stock only”. |

---

## C — Procurement (demand → requisition, PO, goods receipt)

| Status | Item | Notes |
|--------|------|--------|
| **Done** | C1–C2 | **Labels** on demand → requisition fields (article, location, requested qty, source type/ids, reservation id, reason). |
| **Done** | C3 | **List + Kanban** toggle for **requisitions** (columns by status); list + selector retained for actions. |
| **Open** | C4–C5 | **PO:** dedicated **list + kanban** and **detail sheet** on row click (DI/OT parity) — not implemented; PO flow still select + inline actions. |
| **Done** | C6 | **“Receive goods”** opens a **dialog** to post GR (PO, line, location, received/rejected qty) — same posting path as inline PO section. |
| **Open** | C7 | **Empty lists** — if requisitions/POs still empty, confirm **data** (no reqs/POs yet) vs bug; IPC list commands exist. Re-test after creating requisitions/POs in-app. |

---

## D — Repairables

| Status | Item | Notes |
|--------|------|--------|
| **Done** | D1 | **Labels** on repairable form (article, source/return location, qty, reason). |
| **Open** | D2 | **List + kanban** for repairable **orders** (WO/DI-style) — not added; procurement tab kanban applies to **requisitions** only. |
| **Done** | D3 | **Stock for selected article:** loads **`list_inventory_stock_balances`** filtered by article and shows on-hand / reserved / available under Repairables when an article is selected. |

---

## E — Work orders (parts, reservations, status)

| Status | Item | Notes |
|--------|------|--------|
| **Done** | E1–E2 | **Unit cost:** IPC **`evaluate_inventory_unit_cost`** (valuation policies: standard, moving avg, last receipt, etc.). Planning panel **auto-fills unit cost** when article + stock location are set (depends on seeded **`inventory_valuation_policies`** and stock/GR data). |
| **Done** | E3 | **Reservation id** shown on **persisted** part lines (“Stock reservation ID: …”) when present. |
| **Partial** | E4 | **Empty org groups:** in-app **hint** when no org tree nodes are loaded — create/publish nodes in Organization designer. **No demo org seed** in this track. |
| **Partial** | E5 | Reservations appear when parts are saved with article + location and reservation succeeds; **execution** view unchanged here — re-test full WO lifecycle. |
| **Open** | E6 | **“Still empty”** — clarify target list/panel if issue persists. |
| **Open** | E7 | Still depends on assignment/planning data (**E4**); retest after org + WO data exist. |

---

## F — Inventory controls (cycle count, etc.)

| Status | Item | Notes |
|--------|------|--------|
| **Done** | F1 | Tester marked as done. |
| **Done** | F2 | **Labels** on cycle-count session creation and count-line fields (warehouse, scope location, critical threshold, article, location, counted qty, variance reason, reviewer note). |
| **Done** | F3 | **Upsert line** refreshes **`listInventoryCountLines`** for the session only (no full panel `loadAll`), so new lines should appear without a full-page-style reload. |
| **Open** | F4–F7 | Still **unclear / TBD** — need clarification or reproduction steps. |

---

## Summary

### Corrected (in codebase)

- **A1** — Warehouse & stock location CRUD (Rust + IPC + **Warehouses & locations** tab).
- **B** — Synthetic zero rows on stock balances until first movement.
- **C (partial)** — Requisition **labels**, **list/kanban** for requisitions, **Receive goods** dialog.
- **D (partial)** — Repairable form **labels**, **per-article stock balances** snippet.
- **E (partial)** — WO part **valuation-based unit cost**, **reservation id** on lines, **org empty-state hint**.
- **F (partial)** — Controls **labels**, **targeted refresh** after count line upsert.

### Remaining / follow-up

- **C4–C5** — PO **kanban** + **detail drawer** like DI/OT.
- **C7** — Confirm empty procurement lists are data-related after creating reqs/POs.
- **D2** — Repairable orders **list/kanban** (if still required).
- **E4–E7** — Org **seed or demo** data (optional product decision); clarify **E6** if needed.
- **F4–F7** — Clarify tester intent.

### Quick re-test priority

1. **Inventory → Warehouses & locations** — create WH + bin, then use them on articles and stock.  
2. **Inventory → Stock balances** — zeros for new articles; filters.  
3. **Procurement tab** — labels, list/kanban, **Receive goods** dialog, repair article **balances**.  
4. **WO planning** — part line: pick article + bin → **unit cost** fills when valuation policy applies; **reservation id** after save.  
5. **Controls** — upsert count line → line list updates without losing session selection.
