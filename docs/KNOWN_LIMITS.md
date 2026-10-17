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

Last updated: 2026-10-17 (step 5 in progress; planning steps A–E merged, LOTO readiness follow-up pending).

---

## Release actions (must be done at release)

| Limit | Impact | Handled in |
|---|---|---|
| The server's sync allowlist must accept every new synced record type of this version **before** any PC upgrades, or the server rejects those rows. New in this programme include the weekly schedule records (weekly schedules, events, baseline items) and the earlier finance, personnel and inspection types. | Data | Release |
| The built-in first-run "admin" account counts as an administrator while it is active, even if nobody uses it, so the last named administrator can be removed while it exists. Disable it once a named plant administrator exists. | Safety | Release |
| Planning windows saved with the old window dialog (before planning step C) lost their site and apply to the whole plant until a planner re-enters the site ("Site to re-enter"). A locked one blocks commitments everywhere over its dates until then. | Data | Release |
| Units, symptoms, LOTO energy/isolation texts that could not be matched during upgrade were kept as "to review" values; an administrator must review them (filter "à revoir"). | Data | Release |
| Placeholder FMECA rows created by the old Ishikawa promotion (S=O=D=5) are flagged "à revoir" and excluded from risk rankings until a person reviews them. | Data | Release |
| QR labels printed before this version must be reprinted: they carried a local database id, which identifies a different asset on another device. Scanning one is refused with "Label to reprint"; reprint from each asset record (no bulk reprint from the asset list yet). | Data | Release |

## Owner-approved items for Step 7 (validation)

| Limit | Impact | Handled in |
|---|---|---|
| A corrective work order cannot be flagged "needs shutdown"; only PM plans carry it, so corrective work is never checked against shutdown windows. Approved: add the flag and the check. | Safety | Step 7 |
| A status change arriving by sync from another PC is trusted as it was checked there (permits/LOTO, qualifications). Approved: re-validate on arrival; a refused change becomes a sync conflict for review. | Safety | Step 7 |
| Work-order and PM part lines have no "required / optional" flag, so every short planned part blocks the plan. | UX | Step 7 |
| Two first-start actions (licence reconciliation, creating the first administrator) work without a signed-in session (needed at first start, but sensitive). | Safety | Step 7 (with site-scoped roles, step 6) |
| About 60 semi-transparent colour styles that never rendered now do (design-system fix, checked by an automated test). Tints now appear on the lock overlay, licence details, error boxes, table hover/selection, KPI and module cards and import rows; they have not yet been looked at in the running app. | UX | Step 7 (visual check) |

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
| LOTO hand-over roles are free text. | Data | After pilot |
| Permits created before expiry became required (none set) are left unchanged: they cannot be activated or edited until someone sets an expiry (refused with "Permit … has no expiry"); permits already active keep running until closed. | Safety | Release (review open permits) |
| Planning's permit readiness check still uses the old rule (type code 'loto' needs an isolation) instead of the new 'requires energy isolation' flag. It gives the same result for the shipped LOTO type, but a plant type flagged later isn't checked in planning. | Safety | Planning follow-up PR right after step E (switches to the flag). |

## Preventive maintenance

| Limit | Impact | Handled in |
|---|---|---|
| A PM follow-up work order is deferred corrective (EN 13306) and keeps its PM link; follow-ups created before the change were re-tagged only where the link was unambiguous. | Data | Accepted |
| PM / request qualification gaps cannot be overridden before the work order exists (overrides are recorded on work orders). | UX | Accepted |
| The PM auto-pilot is set as a daily run time; a non-daily schedule set elsewhere is kept and shown as "custom", and can only be changed by switching back to daily. | UX | Accepted |
| The PM calendar loads every occurrence, not only the visible dates; large plans load slower. | UX | Step 7 |
| Parts on PM plans are chosen by article-code search. | UX | After pilot |
| Deferral, miss and cancel reasons are now a site reference list; reasons stored before keep their old upper-case codes and new ones are stored in lower case. Screens match either form; no stored reason is rewritten. | Data | Accepted |

## Stock and units of measure

| Limit | Impact | Handled in |
|---|---|---|
| Purchase-order lines are ordered in the stock unit; only receipts and issues take another unit. | UX | After pilot |
| Stock checks for planning use stock available over all locations (reservations netted); other jobs' planned-but-unreserved needs are not subtracted. | Data | After pilot |
| Part quantities on work-order / PM lines carry no unit; they are taken in stock units. | Data | After pilot |
| Cycle-count tolerance (5 units / 0.01) is shown pre-filled in the count dialog instead of coming from a site setting; the counter can change it. | Data | Step 7 |
| The stock-balance "low stock only" option (used by no screen today) still applies the old per-location rule, not the shared low-stock rule. | Data | Step 7 |
| The late purchase-order filter compares against the PC clock; the backend's late rule uses UTC, so an order can differ by a few hours around midnight. | Data | Step 7 |

## Planning and scheduling (steps A–E)

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
| Schedule compliance changed definition in step E: it is now measured against the frozen weekly baseline (the earlier "committed slots done on their day" figure is retired everywhere — planning, home, analytics, monthly report). Values before and after the upgrade are not comparable; periods before weekly baselines existed show "not measured". | Data | Release (tell users) |
| Planning KPIs (loading, compliance, break-ins) are measured only on frozen weeks; a site or week never frozen shows "not measured" with its reason, never a number. | Data | Accepted |
| A baseline job counts as done when the work order behind it is mechanically completed, ended or closed by the end of its week; a PM occurrence completed without a work order is not counted as done. | Data | After pilot |
| A baseline job without planned labour hours weighs its slot length in the KPIs. | Data | Accepted |
| A week unfrozen for editing keeps being measured on its last frozen baseline, flagged "reopened", until it is frozen again. | Data | Accepted |
| A job moved from one frozen week into another counts as displaced in both weeks. | Data | After pilot |
| Break-ins, displaced jobs and available hours use the site's crews of today, not the crews at freeze time (a crew moved to another site takes its history with it). | Data | After pilot |
| The planning page shows a fixed window of 8 weeks of KPIs (the current one and the 7 before it). | UX | After pilot |
| The home week load pools the current frozen week of every site; sites with different weeks are shown as one range (earliest start to latest end), with the sites not frozen counted apart. | UX | Accepted |

## Personnel, qualifications, shifts

| Limit | Impact | Handled in |
|---|---|---|
| Teams without a Classe horaire have 0 h capacity (never an assumed day); migrated teams carry a "Standard 8 h (migrated)" class flagged to review. | Data | Release (review) |
| A Classe horaire version can be edited in place only before it is in force; planned work orders keep their shift-window snapshot and show "Timetable changed since planning". | Data | Accepted |
| A member whose shift window is unknown counts in full on the days a slot touches. | Data | Accepted |
| Editing a person cannot clear a field once set (an emptied field is read as "no change"); skills and certifications are edited only in the Qualifications tab. | Data | Step 7 |
| Employment types and statuses, absence types and position categories are fixed lists in the program, not reference lists an administrator can edit. | Data | Step 7 |

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
| Site-scoped roles (step 6, `docs/SITE_SCOPED_ROLES.md`) are delivered stage by stage. Stage 1 (shared check, sign-in, user creation, rights per site for the screens) is done: a user may now hold roles on a site only, and an administrator chooses at creation where the role applies. Until a module is converted (requests, work orders and permits next; then PM, planning, equipment, inspections, personnel, finance, inventory; then analytics, reports and exports), it still counts plant-wide roles only, so a site-only user is refused in it — never shown another site's data. | Data | Step 6 (stages 2–4) |
| A site-only user sees a module in the menu when they hold its right on a site; a module not yet converted then refuses its screens with "permission required". | UX | Step 6 (stages 2–4) |
| Administrator roles are plant-wide only: giving one on a site (also as emergency access) is refused with a message. | Safety | Accepted |
| Role assignments arriving by sync from another PC are not re-checked on arrival (a site-limited administrator role could only come from a modified PC; it would not make anyone an administrator). Re-checking every synced change against its author's rights is the last stage of step 6, built with step 7's re-validation on arrival. | Safety | Step 6 (sync stage) / Step 7 |
| A picker that is empty because the user lacks the right to read its list says so only on the training and qualification screens; other modules' pickers have not been checked for this case yet. | UX | Step 6 (with site-scoped roles) |
| No separation-of-duties pairs are defined in the permission registry, so none are warned. | Safety | After pilot |
| The role editor warns before discarding unsaved changes when switching role or closing the app, but not when switching to another Administration tab. | UX | Step 7 |
| Settings shows its confirmations through its own message area, not the app-wide one. | UX | Step 7 |

## Home dashboard

| Limit | Impact | Handled in |
|---|---|---|
| Figures open their list pre-filtered, shown as a clearable chip, and the list counts exactly what the figure counted (open and overdue work orders, open requests and requests waiting 7+ days, articles to reorder); tests hold list total = figure. Overdue and waiting use the clock at the moment the list is loaded. Planning figures are not linked yet. | Data | Accepted |
| The work mix link (planned / reactive / emergency) finds the matching work-order types on the screen with the same nature rule the figures use; a work-order type with no nature set appears in no mix, in the figure or the list. | Data | Step 7 (check every type has a nature) |
| "My rounds" counts rounds assigned to me personally, not to my team; "My PM" means PM whose work order is mine. | Data | Accepted |
| The planning backlog panel reads at most 2,000 jobs, with a notice. | UX | Accepted |
| Very old equipment linked only through the legacy criticality list is not counted as critical. | Data | Step 7 |

## Analytics and finance

| Limit | Impact | Handled in |
|---|---|---|
| The cost tile: a work order settled before the period and reopened inside it shows as a negative cost in the current period (consistent with the Finance cockpit). | Data | Accepted |
| RAMS studies do not pull the mean corrective cost automatically (shown in Asset costs). | UX | After pilot |

## Assets

| Limit | Impact | Handled in |
|---|---|---|
| The equipment tree reads each branch from the database when opened. It follows active hierarchy links only; equipment with several active parents appears under each; "Expand all" opens only branches already loaded. | UX | Accepted |
| IoT and ERP "linked records" are yes/no flags, not counts. | UX | After pilot |
| The asset edit form no longer offers a retired status or an end-of-life date (retirement goes through Déclasser), but the server still accepts them on an identity update from another path. Server check being added. | Data | Step 5 |

## App frame, sync centre, documents, organisation, budget

| Limit | Impact | Handled in |
|---|---|---|
| The program's health check does not really query the database, so the status bar shows no database state (the old always-green dot was removed). | UX | Step 7 |
| There is no search across all record types; the top-bar box opens modules (Ctrl K). | UX | After pilot |
| Document categories are a fixed list in the program, not a reference list. | Data | Step 7 |
| An organisation unit's cost centre is free text, not picked from finance cost centres (needs a decision on which permission may set it). | Data | Owner decision |
| A node type cannot be placed under a node of the same type (the publish check treats it as a cycle), e.g. area inside area. | UX | Step 7 |
| Budget drill-down lists at most 2,000 lines and gives no total. | Data | Step 7 |
| The sync centre names about 40 record types; others show as "Other record" with their number. | UX | Accepted |
| The lock screen recognises a disabled PIN from the server's message text, not a code. | UX | Step 7 |
| In Reference data, when the publish-readiness check cannot load, the error box shows the most recent reference-data error, which may come from another action on the same screen. | UX | Step 7 |

## Activity feed and audit log

| Limit | Impact | Handled in |
|---|---|---|
| Event and action names arrive as English codes and are shown made readable ("status changed") with the translated module name, not as fully translated sentences. | UX | Step 7 |
| The organisation-unit filter of the activity feed takes a number, not a pick from the organisation tree. | UX | Step 7 |

## Step 5 (old-layout screens) — in progress

| Limit | Impact | Handled in |
|---|---|---|
| Work-order and request boards are read-only: each status change needs its own dialog (urgency, reason, permit checks). Purchasing boards allow submit/approve by drag, menu or keyboard through the same server step. | UX | Accepted |
| A board's "load more" shows only cards already loaded; the list's page notice covers the rest. | UX | Step 7 |
| Screens still old or mixed (in progress): training, analytics "Customise", asset import wizard, diagnostics page, permit types dialog (to move under Données de référence), permit hand-over form, and the remaining reliability tools (Ishikawa, decision log, rating scales). | UX | Step 5 |

## Modules delivered before this register

Inventory (issue desk, requisitions, valuation, lots, FX, approval limits), RAMS, PM governance,
analytics/SMRP, finance (cost flow, settlement, budget cockpit), personnel/qualifications and
inspections were delivered with their limits reported in chat and in some PR bodies only. Their
limits were not recorded systematically at the time. **Step 7** re-walks each of these modules in
the running app with the demo plant and adds every limit found here.
