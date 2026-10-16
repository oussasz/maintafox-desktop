# Known limits register

One place for every known limit of the modules delivered in the UX/standards programme
(branch `claude/ux-unify-1af493`). Each MODULE REPORT's "Not done / known limits" section is
added here in the same commit as the work it describes.

**Impact:** **Safety** (could let unsafe work proceed or hide a hazard) · **Data** (a figure or
record can be wrong, incomplete or need review) · **UX** (slower or less convenient, no wrong data).

**Handled in:** **Step 7** = hands-on validation pass and its fixes (before the test installer) ·
**After pilot** = planned improvement after first plant use · **Owner decision** = waiting for the
plant owner · **Release** = must be done when the version is released · **Accepted** = accepted by
design, documented.

Last updated: 2026-10-17 (step 5 in progress, planning step E in progress).

---

## Release actions (must be done at release)

| Limit | Impact | Handled in |
|---|---|---|
| The server's sync allowlist must accept every new synced record type of this version **before** any PC upgrades, or the server rejects those rows. New in this programme include the weekly schedule records (weekly schedules, events, baseline items) and the earlier finance, personnel and inspection types. | Data | Release |
| The built-in first-run "admin" account counts as an administrator while it is active, even if nobody uses it, so the last named administrator can be removed while it exists. Disable it once a named plant administrator exists. | Safety | Release |
| Planning windows saved with the old window dialog (before planning step C) lost their site and apply to the whole plant until a planner re-enters the site ("Site to re-enter"). A locked one blocks commitments everywhere over its dates until then. | Data | Release |
| Units, symptoms, LOTO energy/isolation texts that could not be matched during upgrade were kept as "to review" values; an administrator must review them (filter "à revoir"). | Data | Release |
| Placeholder FMECA rows created by the old Ishikawa promotion (S=O=D=5) are flagged "à revoir" and excluded from risk rankings until a person reviews them. | Data | Release |

## Owner-approved items for Step 7 (validation)

| Limit | Impact | Handled in |
|---|---|---|
| A corrective work order cannot be flagged "needs shutdown"; only PM plans carry it, so corrective work is never checked against shutdown windows. Approved: add the flag and the check. | Safety | Step 7 |
| A status change arriving by sync from another PC is trusted as it was checked there (permits/LOTO, qualifications). Approved: re-validate on arrival; a refused change becomes a sync conflict for review. | Safety | Step 7 |
| Editing a permit can still link it to a work order that already has an open permit (creation refuses it). The start gate checks every live permit, so it is not a hole at execution. | Safety | Step 7 |
| Work-order and PM part lines have no "required / optional" flag, so every short planned part blocks the plan. | UX | Step 7 |
| Two first-start actions (licence reconciliation, creating the first administrator) work without a signed-in session (needed at first start, but sensitive). | Safety | Step 7 (with site-scoped roles, step 6) |

## Work orders and requests (standards step: urgency, EN 13306 natures, ISO 14224 symptoms)

| Limit | Impact | Handled in |
|---|---|---|
| Devices on the previous version must upgrade together with this one: they would still send the old urgency and type references. | Data | Release |
| The 4 h estimated duration once used for DI planning candidates was removed; DIs are "not estimated" until converted and estimated. | UX | Accepted |
| Planned labour per technician on a work order does not exist (labour lines hold actuals only), so planning counts only the named assignee per commitment ("+N crew members not named"). Naming every crew member is proposed for later. | Data | Owner decision |

## Requests (DI) screens

| Limit | Impact | Handled in |
|---|---|---|
| No personal saved views; the work queues stand in for them (would need a new synced table). | UX | After pilot |
| The asset picker is a search with the site path shown, not a tree browser. | UX | Step 5 (asset picker batch) |
| The review history does not show who took each decision (the status-history tab does). | UX | After pilot |
| Request lists are not limited by site: anyone with request view sees every site's requests. | Data | Step 6 (site-scoped roles) |

## Permits and LOTO

| Limit | Impact | Handled in |
|---|---|---|
| A permit that expires during the work is only warned in planning and by the expiry notification; start is refused only for an already expired permit. | Safety | Accepted |
| Requests (DI) have no permit field; permit readiness is checked for work orders and PM only. | Safety | Step 7 |
| Some permit lifecycle refusals (draft → review, close…) are still plain English sentences; the start and activation refusals are coded and translated. | UX | Step 5 |
| LOTO hand-over roles are free text. | Data | After pilot |

## Preventive maintenance

| Limit | Impact | Handled in |
|---|---|---|
| A PM follow-up work order is deferred corrective (EN 13306) and keeps its PM link; follow-ups created before the change were re-tagged only where the link was unambiguous. | Data | Accepted |
| PM / request qualification gaps cannot be overridden before the work order exists (overrides are recorded on work orders). | UX | Accepted |

## Stock and units of measure

| Limit | Impact | Handled in |
|---|---|---|
| Purchase-order lines are ordered in the stock unit; only receipts and issues take another unit. | UX | After pilot |
| Stock checks for planning use stock available over all locations (reservations netted); other jobs' planned-but-unreserved needs are not subtracted. | Data | After pilot |
| Part quantities on work-order / PM lines carry no unit; they are taken in stock units. | Data | After pilot |

## Planning and scheduling (steps A–D; E in progress)

| Limit | Impact | Handled in |
|---|---|---|
| Shutdown windows are a planning rule only: nothing at work-order start checks that the equipment is actually stopped. | Safety | Accepted |
| A job with no known location shows "location unknown: site windows not checked". | Safety | Accepted (warned) |
| Certification and skill validity are judged on the PC's local calendar day (no site time zone). | Data | After pilot |
| A site with a single person holding the planning-confirm right cannot unfreeze a week (four-eyes rule). | UX | Accepted |
| Week boundaries and the automatic freeze use local time; the freeze runs only while a PC is open and catches up afterwards. | Data | Accepted |
| A week that spans a change of the site's weekend rule lasts 6–13 days. | Data | Accepted |
| Crews without a planning site (no organisation unit able to own work above them) appear on no weekly schedule; a banner lists them. | Data | Release (setup) |
| The suggested-technician skill score on backlog cards is a simple skill-code match. | UX | After pilot |
| When sites have different work weeks and a planner has no home site, the home planning panel says so instead of showing a week. | UX | Accepted |

## Personnel, qualifications, shifts

| Limit | Impact | Handled in |
|---|---|---|
| Teams without a Classe horaire have 0 h capacity (never an assumed day); migrated teams carry a "Standard 8 h (migrated)" class flagged to review. | Data | Release (review) |
| A Classe horaire version can be edited in place only before it is in force; planned work orders keep their shift-window snapshot and show "Timetable changed since planning". | Data | Accepted |
| A member whose shift window is unknown counts in full on the days a slot touches. | Data | Accepted |

## Notifications

| Limit | Impact | Handled in |
|---|---|---|
| Finance period-close and procurement-approval alerts have no generator yet. | UX | After pilot |
| E-mail goes only through the existing channel; no new channel. | UX | Accepted |
| Escalation runs per PC on the notifications that PC holds; an acknowledgement on another PC stops it once that state syncs. | UX | Accepted |

## Administration, security, settings, currency

| Limit | Impact | Handled in |
|---|---|---|
| A password change arriving by sync does not end sessions started up to 2 minutes before it (clock-skew tolerance); local resets end them at once. | Safety | Accepted |
| Printouts with an invalid stored document header print without a letterhead, with no warning on the print path. | UX | Step 5 |
| Roles limited to a site grant nothing in the modules yet (only plant-wide roles count); approved as its own step. | Data | Step 6 |
| No separation-of-duties pairs are defined in the permission registry, so none are warned. | Safety | After pilot |

## Home dashboard

| Limit | Impact | Handled in |
|---|---|---|
| Work-order, PM and stock lists cannot yet be opened pre-filtered (overdue, assignee, urgency); tiles open the module and the home page lists the exact records. | UX | Step 5 |
| "My rounds" counts rounds assigned to me personally, not to my team; "My PM" means PM whose work order is mine. | Data | Accepted |
| The planning backlog panel reads at most 2,000 jobs, with a notice. | UX | Accepted |
| Very old equipment linked only through the legacy criticality list is not counted as critical. | Data | Step 7 |

## Analytics and finance

| Limit | Impact | Handled in |
|---|---|---|
| The cost tile: a work order settled before the period and reopened inside it shows as a negative cost in the current period (consistent with the Finance cockpit). | Data | Accepted |
| RAMS studies do not pull the mean corrective cost automatically (shown in Asset costs). | UX | After pilot |

## Step 5 (old-layout screens) — in progress

| Limit | Impact | Handled in |
|---|---|---|
| Moving cards on boards (drag or keyboard) is built but no board allows moves yet; status changes go through their dialogs. Being decided per board. | UX | Step 5 |
| About 79 screens/panels remain old or mixed; inventory in the programme notes (old-screens-inventory). | UX | Step 5 |

## Modules delivered before this register

Inventory (issue desk, requisitions, valuation, lots, FX, approval limits), RAMS, PM governance,
analytics/SMRP, finance (cost flow, settlement, budget cockpit), personnel/qualifications and
inspections were delivered with their limits reported in chat and in some PR bodies only. Their
limits were not recorded systematically at the time. **Step 7** re-walks each of these modules in
the running app with the demo plant and adds every limit found here.
