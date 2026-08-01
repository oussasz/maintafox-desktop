# Maintafox — Launch Business Plan (V1 Core, 20 Days)

**Document owner:** Product & Commercial  
**Horizon:** 20 working days to a marketable, sellable version  
**Product name:** **Maintafox Core** (commercial edition)  
**Status:** Action plan — practical, non-technical  

---

## 1. Executive decision

You do **not** need a second product or a forked codebase. You need:

1. **One application** with a clearly defined **Core edition** customers can buy.
2. **Hidden modules** for everything not ready for sale (reliability, advanced analytics, permits, inspections, etc.).
3. **Central control from the VPS admin console** so each company sees only what their contract allows.
4. **A full edition visible only in your development environment** for internal work and demos of the future roadmap.

This plan sequences work to finish the **operational maintenance loop** first, then package and sell it.

---

## 2. What you are selling on Day 20

### Product promise (one sentence)

> *Maintafox Core lets industrial maintenance teams register assets, receive intervention requests, execute work orders, manage spare parts and teams, and keep a traceable history — locally, with or without connectivity.*

### The minimum credible workflow (your “golden path”)

```
Infrastructure → Equipment → Intervention Request (DI) → Work Order (OT)
        → Labor / parts / closure → History & traceability
```

If a pilot customer can run this loop reliably for one real piece of equipment, you have a sellable product — even if advanced reliability analytics are not visible.

### Commercial edition: **Maintafox Core v1.0**

| Included in Core | Purpose for the buyer |
|------------------|------------------------|
| **Infrastructure & sites** (organization tree) | Where work happens — plants, areas, lines |
| **Equipment registry** | Asset backbone linked to all work |
| **Intervention requests (DI)** | Intake, triage, approval |
| **Work orders (OT)** | Plan, assign, execute, verify, close |
| **Spare parts & stock** (catalog, locations, issue to WO) | Material traceability on jobs |
| **Team / personnel** (roster, skills basics, assignment) | Who does the work |
| **Users, roles & access** | Governance per site/role |
| **Reference data** (types, priorities, failure codes basics) | Consistent vocabulary |
| **Archive & activity history** (operational trace) | Evidence and auditability |
| **Settings, backup, license** | Safe operations and subscription control |
| **Operational dashboard** (counts, backlog, open OT — not RAMS) | Management visibility |

### Explicitly **not** in Core v1.0 (hidden, future editions)

| Hidden module | Why it waits |
|---------------|--------------|
| **Reliability / RAMS** (MTBF, FMECA, RCM, ISO 14224 engine) | Needs trustworthy WO close-out data first; scientific validation not complete |
| **Advanced analytics & reporting** | Depends on stable operational data |
| **Preventive maintenance (PM)** | Valuable upsell; not required for first corrective-maintenance sale |
| **Planning & scheduling engine** | Phase 3 scope |
| **Permits / LOTO** | Safety-critical; partial backend exists but not launch-ready for general sale |
| **Inspection rounds** | Feeds future RAMS; not needed for Core loop |
| **Training & certification** | Compliance upsell |
| **Budget & cost centers** | Finance upsell |
| **ERP / IoT connectors** | Enterprise tier |
| **Vendor admin console** (full platform ops) | Maintafox internal only |

---

## 3. How feature visibility works (business model)

You already designed the right architecture. Here is how to **use** it commercially — without building separate apps.

### Three environments, one product

| Environment | Who uses it | What they see | Update channel |
|-------------|-------------|---------------|----------------|
| **Development** | Maintafox engineering | **Full platform** — all modules visible for build and test | `internal` |
| **Pilot** | 3–5 design-partner plants | **Core + agreed extras** per contract | `pilot` |
| **Production** | Paying Core customers | **Core only** — advanced modules hidden | `stable` |

### Three levers you control from the VPS console (per company)

Think of each customer record as a **remote control** for their desktop app:

#### Lever 1 — **Edition / tier**

Examples: `core`, `professional`, `enterprise`, `development`.

- **Core** unlocks the module set in Section 2.
- **Development** unlocks everything (your internal tenants only).
- Higher tiers are for future upsell; do not sell them until modules are ready.

#### Lever 2 — **Feature flags** (module on/off)

Each module has a flag (examples aligned with your product: `eq`, `di`, `ot`, `inv`, `per`, `org`, `ram`, `pm`, `rep`, …).

- **Visible** = flag enabled in the customer’s signed license → menu appears, screens accessible.
- **Hidden** = flag disabled → menu item does not appear; user cannot reach the screen.
- Changes apply on next **license heartbeat** (or at login), so you can turn modules on/off without reinstalling the app.

#### Lever 3 — **Release channel**

- `internal` → your builds with experimental modules.
- `pilot` → validated Core + selected extras for design partners.
- `stable` → commercial Core builds only.

**Rule:** Production customers never receive `internal` channel updates.

### Per-company configuration (what you set in the console)

For each customer, document and configure:

| Field | Example (Core customer) | Example (Dev tenant) |
|-------|-------------------------|----------------------|
| Edition | `core` | `development` |
| Channel | `stable` | `internal` |
| Machine slots | 1–3 | 5 |
| Enabled modules | eq, di, ot, inv, per, org, ref, archive, settings | all modules |
| Disabled modules | ram, rep, pm, plan, ptw, ins, trn, fin, erp, iot | none |
| Support posture | standard | internal |

### What “hide and reveal” means for the user

- **Hidden** modules are invisible in the sidebar — not greyed out, not “coming soon.”
- **Revealed** modules appear when you enable them in the console (upsell, pilot extension, or edition upgrade).
- **Development** tenants always see the full menu so your team does not slow down building future versions.

### Operational rule for the next 20 days

> No customer-facing install may ship with advanced modules visible unless that module is on the Day-20 acceptance checklist for Core.

---

## 4. Honest gap assessment (current state vs. sellable)

This is based on the current codebase and roadmap gaps — what still blocks a credible sale.

### Ready or nearly ready

| Area | Status | Notes |
|------|--------|-------|
| Application shell, auth, license activation | Strong | Foundation in place |
| Organization / infrastructure designer | Usable | Core dependency met |
| Equipment registry | Usable | Linked to DI and OT |
| Intervention requests (DI) | Largely built | Needs end-to-end validation with OT |
| Work orders (OT) — structure | Largely built | List, detail, states, labor, parts exist |
| Inventory / spare parts — basics | Partially complete | Item master, stock, WO linkage started |
| Personnel / team | Usable | Roster, detail, assignment surfaces exist |
| Users & admin | Usable | Role-based access works |
| VPS licensing & entitlements | Built | Feature flags in license envelope; console customer management exists |
| Backup / settings baseline | Present | Validate before pilot |

### Must close before selling (critical gaps)

| Gap | Business risk if unfixed |
|-----|--------------------------|
| **WO execution loop not bulletproof** | Customer tries to assign/close a job and hits errors — instant churn |
| **DI → OT conversion not proven on real scenarios** | Product story breaks at the main value moment |
| **WO close-out incomplete** (verification, mandatory fields, attachments) | Jobs “close” without defensible evidence |
| **Advanced modules still visible in menu** | Customer opens Reliability or Permits, sees unfinished product — trust destroyed |
| **Feature flags not yet driving the menu** | You cannot safely ship Core until VPS flags actually hide modules |
| **Archive / activity trail incomplete** | “Traceability” promise weak for audits |
| **No documented golden-path test for pilots** | Every demo becomes improvisation |
| **Marketing says “full platform access”** | Legal and expectation mismatch with Core edition |

### Acceptable for v1 Core (can be thin but must work)

| Area | Acceptable v1 scope |
|------|---------------------|
| Dashboard | Open DI count, open OT count, overdue jobs — not RAMS charts |
| Notifications | Basic in-app; email can wait |
| Inventory | Catalog + stock + manual issue to OT; reorder automation can wait |
| Personnel | Roster + assign to OT; full training matrix can wait |
| PM / planning | Hidden entirely |
| Import | Excel/CSV template for equipment + personnel for pilot onboarding |

---

## 5. Roles during the 20 days

| Role | Responsibility |
|------|----------------|
| **Product owner (you)** | Scope decisions, pilot selection, accept/reject daily outcomes |
| **Build lead** | Module completion and visibility wiring |
| **QA / pilot champion** | Run golden-path scripts daily |
| **Commercial** | Update offer sheet, pricing, contract scope for Core |
| **VPS / ops** | Customer templates in console, channels, license keys |

If you are solo: alternate **build mornings** and **validation afternoons**, but never skip the daily golden-path check.

---

## 6. Twenty-day plan (one focus per day)

Each day ends with a **binary outcome**: done or not done. No partial credit.

### Week 1 — Lock the product box and stop the bleeding

| Day | Focus | End-of-day outcome |
|-----|-------|-------------------|
| **1** | **Define Core v1.0 scope document** | One-page SKU signed: included modules, hidden modules, golden path, out-of-scope list |
| **2** | **Configure VPS customer templates** | Three templates ready: Development (full), Pilot (core+), Production Core |
| **3** | **Hide all non-Core modules** | Reliability, analytics, PM, planning, permits, inspections, training, budget invisible for Core template |
| **4** | **Infrastructure & equipment validation** | Create site tree, register 10 assets, link to org nodes — no blockers |
| **5** | **Intervention request (DI) day** | Create, approve, reject, attach context — full intake works in FR |
| **6** | **DI → OT conversion day** | Approved DI becomes OT with correct equipment and priority |
| **7** | **Week 1 review & pilot script v1** | Written 30-minute demo script; fix top 3 blockers found in review |

### Week 2 — Make work orders trustworthy

| Day | Focus | End-of-day outcome |
|-----|-------|-------------------|
| **8** | **OT planning & assignment** | Plan job, assign technician, visible on board/list |
| **9** | **OT execution** | Start, pause, resume, labor time, tasks — field-realistic flow |
| **10** | **Parts on OT** | Reserve/issue spare part from stock on an open OT |
| **11** | **OT close-out** | Verification step, closure rules, closed job immutable |
| **12** | **Team & access** | Two roles (manager + technician) see correct menus and actions |
| **13** | **Archive & history** | Closed DI/OT findable; change history visible for dispute |
| **14** | **Week 2 review** | Golden path runs start-to-finish without workaround |

### Week 3 — Package for market

| Day | Focus | End-of-day outcome |
|-----|-------|-------------------|
| **15** | **Operational dashboard only** | Backlog widgets; remove or hide analytics that imply RAMS |
| **16** | **Onboarding kit** | Import template + 1-page admin setup guide + role matrix |
| **17** | **License & channel dry run** | Fresh machine: activate Core license, heartbeat, modules match console |
| **18** | **Backup / restore rehearsal** | Restore proven on test database; documented for support |
| **19** | **Pilot dress rehearsal** | Full demo recorded; second person follows script unaided |
| **20** | **Launch decision** | Go / no-go against checklist (Section 7); tag **Maintafox Core v1.0** |

---

## 7. Launch readiness checklist (Day 20 gate)

All must be **yes** for commercial sale:

### Product

- [ ] Golden path completes: **Infrastructure → Equipment → DI → OT → parts → close → history**
- [ ] No menu item visible for unfinished advanced modules (Core template)
- [ ] French UI consistent on Core screens (no obvious placeholders)
- [ ] Two roles tested: admin and field technician

### Control plane

- [ ] VPS console can enable/disable modules per customer without reinstall
- [ ] Development tenant shows full platform; production tenant shows Core only
- [ ] `stable` channel serves only Core-ready builds

### Commercial

- [ ] Offer sheet updated: **Core** scope, not “full platform”
- [ ] Pricing and contract reference the module list
- [ ] Pilot → paid conversion terms defined

### Operations

- [ ] Support contact and response commitment documented
- [ ] Backup/restore steps for customer admin
- [ ] Known limitations list published (what v1.0 does not do)

---

## 8. What to tell customers (positioning)

### Say this

- “Maintafox Core is a **local-first maintenance operations** system.”
- “You get **requests, work orders, assets, spare parts, and team management** with full traceability.”
- “Advanced reliability analytics are on the **roadmap** for a later edition.”
- “Pilot partners help shape **Professional** features; Core is production-ready for daily operations.”

### Do not say this

- “Complete CMMS with RAMS and FMECA.”
- “Full platform access” (unless Development/Pilot contract explicitly lists extras).
- “Enterprise integrations included.”

---

## 9. Edition roadmap (after Day 20)

| Edition | Timing | Adds |
|---------|--------|------|
| **Core v1.0** | Day 20 | Section 2 modules |
| **Professional v1.x** | +2–3 months | PM, planning, permits, inspections, richer analytics |
| **Enterprise v2.x** | +6+ months | RAMS, ERP, IoT, multi-site sync at scale |

Upsell path: enable flags in VPS console → customer sees new menus on next license refresh → invoice tier change.

---

## 10. Risk register and scope cuts

If time runs short, cut in this order (never cut the golden path):

1. ~~RAMS / reliability~~ — already out of scope  
2. ~~PM and planning~~ — hide  
3. Dashboard polish — minimal widgets only  
4. Notification email — in-app only  
5. Inventory automation (reorder rules) — manual stock moves only  
6. **Never cut:** DI, OT close-out, equipment link, license gating, module hiding  

---

## 11. Daily discipline (non-negotiable)

1. **Morning:** one module or gap from the calendar above.  
2. **Afternoon:** run the golden path on a clean dataset.  
3. **Evening:** log blockers in a single list; only three may carry to the next day.  
4. **No new features** outside Core scope until Day 20 gate passes.

---

## 12. Immediate next actions (before Day 1)

1. Print Section 2 (Core module list) and Section 3 (visibility model) — this is your contract with yourself.  
2. Create three VPS customer profiles: `Maintafox-Dev`, `Pilot-Template`, `Core-Production-Template`.  
3. Walk the app once as a Core user (mentally hide advanced menus) and note every screen that breaks the story.  
4. Update the early-adopter registration sheet: replace “accès intégral” with **“Maintafox Core + feuille de route Professional”** unless you intentionally sell extras to pilots.

---

*This plan aligns with `docs/PRD.md` delivery phases: Phase 2 (Core execution) is the commercial launch; Phase 5 (RAMS) is a later edition controlled by VPS feature flags and release channels.*
