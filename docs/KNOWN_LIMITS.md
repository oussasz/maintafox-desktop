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
| New synced record types: the per-site rules (site settings and their change history) must be in the server sync allowlist before PCs upgrade. The desktop copy of the allowlist already has them; the server's original must be updated at release. | Data | Release |
| Per-site self-approval rule: the sync server must accept site rules and their change history only from users who hold the settings-administration right. The desktop checks that every change carries a complete, consistent, latest audit trail, but cannot prove who really sent it, so a device with valid credentials could otherwise forge one. | Safety | Release (sync server) |
| A per-site self-approval rule received from another PC is applied only together with its matching audit record; until that record arrives it waits (shown as a deferred row in the sync status). A rule whose record never comes, including a forged one, waits indefinitely and is never applied; it does not become a conflict by itself. | Safety | Accepted |
| Photos and attachments are the one kind of history that sync may delete (so a removal on one PC applies everywhere): the sync server must accept such a delete only from a user allowed to remove that attachment (its work order, request or equipment right, or the person who added it). Otherwise a paired PC with valid credentials could erase work-order, request or equipment photos everywhere. Inspection evidence and all other history cannot be deleted by sync. | Safety | Release (sync server) |
| The licence server does not sign the activation details it returns (tenant, licence, PC, demo data, expiry); only the entitlement envelope is signed. The desktop therefore cannot prove a licence change came from the server, so a change of plant, licence or PC needs a signed-in administrator once the plant has one (and demo data always does). The server must sign the activation details (or include them in the signed envelope) so the desktop can refuse unsigned ones. | Safety | Release (licence server) |
| Personnel and document list values found in existing data but not in the new lists are kept exactly as stored, added to their list and flagged "à revoir"; an administrator must review them (filter "à revoir"). People whose stored employment status is not a known status are listed for review. | Data | Release |
| QR labels printed before this version must be reprinted: they carried a local database id, which identifies a different asset on another device. Scanning one is refused with "Label to reprint"; reprint from each asset record (no bulk reprint from the asset list yet). | Data | Release |
| Permit hand-over roles come from the plant's list "Rôles de passation de permis" (PERMIT.HANDOVER_ROLES, Données de référence), shipped empty: until an administrator fills it, no hand-over can be recorded, so no permit can be handed back. Roles typed as free text before this version are kept exactly as stored, added to the list and flagged "à revoir". Hand-overs recorded before this version have no signer (nobody is invented). | Data | Release (fill the list, review flagged roles) |

## Owner-approved items for Step 7 (validation)

| Limit | Impact | Handled in |
|---|---|---|
| A corrective work order can be flagged "needs shutdown" (planning panel, with confirmation) and is checked against shutdown windows like PM plans. The badge shows on the work-order list, board, detail and the planning backlog cards. | UX | Accepted |
| A permit, isolation, suspension, hand-over log, work-order start or qualification override changed on another PC is checked again on arrival with the same rules as here (never stricter): activation (time, expiry, isolations verified by a second person, no lock removed), close, hand-back, suspension, the two-person rule, the start gate at the start time (including a permit of the rule's type for a rule-flagged work order), override reason / person / context / no self-override / approver right. A refused change is a sync conflict for review (sync centre, coded and explained in FR/EN), with nothing written. | Safety | Accepted |
| Not re-checked on arrival: the qualifications of the people booked or acting on the work day, and the permissions and step-up of the person at the time of the action; these stay enforced on the PC where the action is taken (certificates may reach this PC later or differently). | Safety | Accepted |
| Rows that arrive together are applied together: a permit's change with the isolations, suspensions and hand-over logs it was judged on, a work order's start with its permits, waits and status changes. Each goes through the normal checks of any received row (an older version, or a row changed here and not yet sent, is never applied over this PC's copy); if one is refused, none is written and the permit or work order is a conflict; if one is still waiting, the whole group waits. | Safety | Accepted |
| An isolation received by sync is fitted and verified before its permit protects the work (exact times, as on this PC); none is added and nobody's name on one changes while the permit protects the work; it never moves to another permit. | Safety | Accepted |
| Deletes received by sync: a permit, its isolations and suspensions only while the permit is a draft (the LOTO history of every other permit is kept); a permit type or a qualification requirement never (edit or deactivate them). | Safety | Accepted |
| A work order received "in progress" is judged at the moment it started, resumed or was reopened, against its permits as they were then; times in the future, or a resume later than the work order row itself, are refused as inconsistent. A work order reopened to in progress has no recorded reopening time and is judged at its last start or resume. | Safety | Accepted |
| A work order's plant-rule permit flag cannot be removed or changed by a received row; a received clearing needs a reason, a named clearer, the permit-issue right of that clearer on this PC (tenant-wide), and a clearing time not in the future and not after the work started. The clearing (who, when, why) is judged in full whenever any part of it is new, even while the flag is still set. When two PCs each cleared the same work order offline, a received clearing that differs from the one recorded here and is complete (who, when and why all present) is judged in full like a new one and replaces it when valid, so both PCs converge; a received row that removes the recorded clearing or any part of it (who, when or why missing) is a conflict (WO_PERMIT_RULE_CLEAR_INVALID) (this PC never does it either: the clearing is written once, and the general edit only sets the flag again, or back to "not required" after a clearing, keeping the clearing). A flag set again with its clearing kept is applied. "Before the work started" is read from the actual start: a work order in progress without an actual start is not caught by that test, and a clearing already held here is not judged again when a later row moves the actual start before it (the clear action itself refuses a work order past "ready"). A clearing already recorded is judged again when a received row sets the flag back to "not required", including that its clearer still holds the right here (fails closed). | Safety | Accepted |
| A received override approver or rule-flag clearer must hold the right on this PC when the row arrives (fails closed). While rows that could give that person the right are still waiting in the inbox, the row waits instead of becoming a conflict; it is never applied without the right. Only these rows hold it: a plant-wide, non-emergency assignment of that person to a role that carries the right, or a permission row for that right of a role they hold plant-wide (here, or by such a waiting assignment, even if the role itself is still waiting). Site-scoped or emergency assignments, other people, other roles and other permissions never hold it. Such a row that never arrives keeps it waiting (shown as deferred, no age shown yet). A right given by a still-waiting account row (administrator or active flag) or by an assignment not yet in force is not waited for: conflict at once (fails closed; accepting the remote version once it arrived re-runs the check). The wait check scans the waiting assignment and permission rows once per judged row, per sweep (bounded; slow only on a first synchronisation with many such rows). An approver demoted since makes it a conflict to confirm. | Safety | Accepted |
| A permit hand-over log received from another PC without a signer is accepted (PCs before this version record none). A log that names a signer or a receiver counts only once that person is on this PC (it waits until then). A log whose receiver is the person who signed it is refused (sync conflict "hand-over received by the person who signed it", nothing written) and never counts for handing a permit back. All PCs upgrade together. Because a log without a signer is accepted, a modified PC can still send one and have a permit handed back without a named signer: until every PC of the plant runs this version, the signer cannot be required on received logs. | Safety | Release (once all PCs are upgraded, refuse received logs without a signer) |
| A permit's "needs energy isolation" setting is read from this PC, as the local activation does: a type made stricter here and not yet on the other PC makes a permit activated there under the old setting a conflict, for review. A qualification requirement deactivated on another PC is applied as received (the right and step-up of the person who did it are not re-checked here). | Safety | Accepted |
| A received row that waits for something not yet on this PC (a person, a list value, an isolation, the rows that could give a right) is shown as deferred, naming what it waits for, and waits as long as that row does not come; it never becomes a conflict by itself and has no age alert. | Data | After pilot |
| Work-order and PM part lines are marked required or optional (existing lines became required, today's behaviour; every new line must be answered, nothing preselected); only a short required part blocks the plan, a short optional part warns. While the work order is in draft or planning, a saved planned line with no recorded use can be switched between required and optional or cancelled (kept with the status "Annulée", its reservation released, audited; it leaves totals, availability and readiness; adding the part again makes a new line). After planning, a planned line is settled during execution with Used / Not used. | UX | Accepted |
| Two first-start actions (licence reconciliation, creating the first administrator) work without a signed-in session (needed at first start, but sensitive). | Safety | Step 7 (with site-scoped roles, step 6) |
| About 60 semi-transparent colour styles that never rendered now do (design-system fix, checked by an automated test). Tints now appear on the lock overlay, licence details, error boxes, table hover/selection, KPI and module cards and import rows; they have not yet been looked at in the running app. | UX | Step 7 (visual check) |

## Work orders and requests (standards step: urgency, EN 13306 natures, ISO 14224 symptoms)

**Glossary (owner decision 2026-10-17): two named meanings of an open work order.**
"Open (work to do)" = not yet mechanically completed: home, backlog, work-order lists.
"Not yet closed" = not closed and not cancelled, including work done but awaiting closure: monthly report, analytics "as of".
Each figure says which one it shows, and its list link applies the same rule.

| Limit | Impact | Handled in |
|---|---|---|
| Every "open work order" count uses one of the two shared rules and its figure shows the rule name, with a list link using the same rule (SMRP backlog included). | Data | Accepted |
| Devices on the previous version must upgrade together with this one: they would still send the old urgency and type references, and an edit from an old PC turns a part line marked optional back to required (the safe direction), and an old PC does not know cancelled part lines: it would count them as planned until it is upgraded. | Data | Release |
| The 4 h estimated duration once used for DI planning candidates was removed; DIs are "not estimated" until converted and estimated. | UX | Accepted |
| Planned labour per technician on a work order does not exist (labour lines hold actuals only), so planning books duration × crew size and counts unnamed crew members in the crew load. Naming every crew member is not planned for the pilot (decision 2026-10-19; after the pilot if planners ask). | Data | Accepted |
| A work-order type without a maintenance nature is listed in Données de référence with the number of work orders using it; a holder of the reference-data manage right gives it one of the six natures, once, with a reason (audited). Until then its work orders count in no planned / reactive / emergency share. | Data | Accepted |
| A nature tag is read the same way by every screen and report: spaces, tabs and line breaks around it are ignored. A tag with another invisible character (for example a no-break space) is read as no nature by all of them and appears on the review list. Stored tags are not rewritten (owner decision: leave stored data as is). | Data | Accepted |

## Requests (DI) screens

| Limit | Impact | Handled in |
|---|---|---|
| No personal saved views; the work queues stand in for them (decision 2026-10-19: enough for the pilot; a new synced table only if pilot users ask). | UX | Accepted |
| The asset picker is a search with the site path shown, not a tree browser; the full tree is on the equipment page (decision 2026-10-19). | UX | Accepted |
| Requests are now limited by site (step 6, stage 2): a user whose role is on a site sees, counts and acts on the requests of that site and of the lines/zones below it only; a plant-wide user sees everything as before. The list says how many requests it leaves out and why ("outside your sites", "without a site"); these counts cover all requests, whatever the filters or search, so a search never tells a site user what another site's requests contain. The home page, the dashboard and the request reports and exports are not converted yet: they still need a plant-wide role. | Data | Step 6 (stage 3: home, analytics, reports) |
| Old requests recorded without a site (site left empty before a site was required) are flagged "To review: no site". Only plant-wide users see them and can act on them; no screen can attach them to a site yet (editing a request does not change its site or equipment), and nothing is rewritten in the stored data. | Data | After pilot (data review) |
| A new request must have a site, taken from its equipment's location: a request without one is refused for everyone, from the request screen and from inspection and PM follow-ups alike. A site user cannot create a request on another site, nor on equipment installed on another site. | Data | Accepted |
| A site user's "My requests" shows their requests on the sites where they hold the request view right; a request they raised on a site they no longer hold is counted in the "not shown" note instead. | UX | Accepted |
| Converting a request to a work order now also needs the work-order creation right at the request's site (the work order is created there). The default Administrator and Supervisor roles have it; a custom role that can convert but not create work orders is refused, and the convert button is not shown. | Safety | Accepted |
| The audit log of all requests shows the requests where the user holds request administration; events not tied to a request are shown to plant-wide users only. | Data | Accepted |
| Request SLA rules stay plant-wide settings: only a user with a plant-wide role reads or changes them. | UX | Accepted |

## Permits and LOTO

| Limit | Impact | Handled in |
|---|---|---|
| A permit that expires during the work is only warned in planning and by the expiry notification; start is refused only for an already expired permit. | Safety | Accepted |
| Requests (DI) carry no permit; at conversion, the new work order is flagged "permit required" when a plant rule in Données de référence (equipment class and/or work type → permit type) matches, and the work order cannot start or resume until a linked permit of the rule's permit type is active and ready (a permit of another type does not count). With no rule configured nothing is flagged and the conversion says so. Clearing a rule's flag needs the permit-issue right and a reason (audited); rules that cannot be applied are marked in Données de référence and a conversion that skipped them shows a warning that stays until closed. | Safety | Accepted (owner decision) |
| The refusal "a permit of type … is required" names the permit type by its stored name, which has one language (as entered in the permit types), so a French screen can show an English type name. | UX | After pilot |
| If a permit type named by a permit rule is later removed, a work order already flagged by that rule cannot start (no permit can be of a type that no longer exists) until someone with the permit-issue right clears the flag with a reason. This fails closed by design. | Safety | Accepted |
| Hand-over roles adopted from old free text (flagged "à revoir") stay active: they can be picked for new hand-overs before an administrator reviews them, typos included (same as the personnel lists). | Data | Owner decision |
| A free-text hand-over role received from a PC that has not upgraded is added to the list (flagged "à revoir") only at the next start of this PC, not when it arrives. If the plant later adds the same role code on another PC, the two can clash on that code (same as the personnel lists). | Data | After pilot |
| Hand-over receiver search: "%" and "_" in the search act as wildcards, and an upper-case accented letter does not find the lower-case one ("É" does not find "é"). A person without a display name is listed and shown under their login name. | UX | After pilot |
| The upgrade step that creates the hand-over roles list (migration 235) uses the app's own list code. If that code later needs a column added by a later upgrade step, an upgrade from a version before 235 would fail; an upgrade test from a pre-235 database covers today's code. | Data | Release (upgrade test from an old database) |
| Permits created before expiry became required (none set) are left unchanged: they cannot be activated or edited until someone sets an expiry (refused with "Permit … has no expiry"); permits already active keep running until closed. | Safety | Release (review open permits) |

## Preventive maintenance

| Limit | Impact | Handled in |
|---|---|---|
| A PM follow-up work order is deferred corrective (EN 13306) and keeps its PM link; follow-ups created before the change were re-tagged only where the link was unambiguous. | Data | Accepted |
| PM / request qualification gaps cannot be overridden before the work order exists (overrides are recorded on work orders). | UX | Accepted |
| The PM auto-pilot is set as a daily run time; a non-daily schedule set elsewhere is kept and shown as "custom", and can only be changed by switching back to daily. | UX | Accepted |
| Parts on PM plans are chosen by article-code search (same search as work orders; shows the available quantity and unit). | UX | Accepted |
| Deferral, miss and cancel reasons are now a site reference list; reasons stored before keep their old upper-case codes and new ones are stored in lower case. Screens match either form; no stored reason is rewritten. | Data | Accepted |

## Stock and units of measure

| Limit | Impact | Handled in |
|---|---|---|
| Purchase-order lines are ordered in the stock unit; only receipts and issues take another unit. | UX | After pilot |
| Stock checks for planning use stock available over all locations (reservations netted); other jobs' planned-but-unreserved needs are not subtracted. | Data | After pilot |
| Part quantities on work-order / PM lines are always in the article's stock unit (shown next to the quantity on the work-order planning, execution and print screens; PM part lines do not show it yet); no other unit can be entered. | Data | After pilot |
| Cycle-count and reconciliation tolerances are tenant settings (Settings › Inventory), seeded with the former values (5 units / 0.01) and checked server-side; each count session keeps the tolerance it was opened with. | Data | Accepted |

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
| A new person's origin (internal / external) starts as "internal"; the person can change it before saving. Owner decision 2026-10-18: "Keep 'internal'". Every list value from Données de référence (employment type and status, position category, absence type, document category) starts empty and must be chosen. | UX | Accepted (owner decision) |
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
| Site-scoped roles (step 6, `docs/SITE_SCOPED_ROLES.md`) are delivered stage by stage. Stage 1 (shared check, sign-in, user creation, rights per site for the screens) is done: a user may now hold roles on a site only, and an administrator chooses at creation where the role applies. Stage 2 has converted the requests (DI) module. Until a module is converted (work orders and permits next; then PM, planning, equipment, inspections, personnel, finance, inventory; then analytics, reports and exports), it still counts plant-wide roles only, so a site-only user is refused in it — never shown another site's data. | Data | Step 6 (stages 2–4) |
| A site-only user sees a module in the menu when they hold its right on a site; a module not yet converted then refuses its screens with "permission required". | UX | Step 6 (stages 2–4) |
| When a user's roles change, or the screen moves to another site, buttons that depend on site rights are shown as not allowed for a moment while the rights reload (never with the previous site's rights). | UX | Accepted |
| Administrator roles are plant-wide only: giving one on a site (also as emergency access) is refused with a message. | Safety | Accepted |
| Role assignments arriving by sync from another PC are not re-checked on arrival (a site-limited administrator role could only come from a modified PC; it would not make anyone an administrator). Re-checking every synced change against its author's rights is the last stage of step 6, built with step 7's re-validation on arrival. | Safety | Step 6 (sync stage) / Step 7 |
| A picker or filter whose list the user may not read now says which right is needed and never requests the list (all modules audited). The access rules themselves are unchanged. | UX | Accepted |
| Choosing an unfreeze or break-in approver on the planning page reads the user list, which needs the user-administration right; planners without it see a note instead of the list, so they cannot name an approver where one is required. Decide in the roles step whether planners may read approver names (planning-scoped list). | UX | Step 6 (roles) |
| The weekly schedule is per site and the site list comes from the organisation tree: a planner without the organisation view right cannot use it (the screen says why). | UX | Step 6 (roles) |
| Creating or moving an asset needs a location from the organisation tree, which needs the organisation view right: a user allowed to manage assets but not to view the organisation cannot create one (the screen says why). Decide in the roles step whether asset managers may read the location list. | UX | Step 6 (roles) |
| No separation-of-duties pairs are defined in the permission registry, so none are warned. | Safety | After pilot |

## Home dashboard

| Limit | Impact | Handled in |
|---|---|---|
| Figures open their list pre-filtered, shown as a clearable chip, and the list counts exactly what the figure counted (open and overdue work orders, open requests and requests waiting 7+ days, articles to reorder); tests hold list total = figure. Overdue and waiting use the clock at the moment the list is loaded. Planning figures on home open the planning page, not pre-filtered. | Data | Accepted |
| The work mix link (planned / reactive / emergency) finds the matching work-order types on the screen with the same nature rule the figures use; a work-order type with no nature set appears in no mix, in the figure or the list; the types concerned are listed in Données de référence for review. | Data | Release (a plant's review list must be empty before go-live) |
| "My rounds" counts rounds assigned to me personally, not to my team; "My PM" means PM whose work order is mine. | Data | Accepted |
| The planning backlog panel reads at most 2,000 jobs, with a notice. | UX | Accepted |
| Equipment whose criticality comes only from the old criticality list is read through the standard correspondence (CRITIQUE→A, IMPORTANT→B, STANDARD→C, NON-CRITIQUE→D) and flagged "à revoir"; a plant's own old code is read as no criticality until a planner sets one. Nothing stored is rewritten. | Data | Release (review flagged equipment) |

## Reliability decisions (four-eyes, owner decision 2026-10-17)

| Limit | Impact | Handled in |
|---|---|---|
| A decision's site is the planning site of its equipment. A decision on equipment with no such site, or a study spanning several sites, always needs a second approver (the refusal says why). | Safety | Accepted |
| A self-approval recorded before this rule that arrives by sync on a new PC becomes a conflict to review (no plant data yet). | Data | Accepted |
| A site's self-approval rule can be switched off but not deleted; every change is audited with who, when, before/after and reason. | UX | Accepted |
| A record received by sync that waits for something not yet received (a decision whose study or site is missing, a site rule whose audit trail has not arrived) keeps waiting; it never becomes a conflict by itself. The safe rule (four-eyes) stays in force meanwhile; the sync status shows these as deferred. | Data | After pilot |
| Retirement and recommission rules are checked on the PC where the action is taken; such an event received by sync from another PC is applied as recorded there, not re-checked. | Data | Accepted |
| A person whose stored employment status is unknown keeps being read as before (not active, neither ended nor inactive) until reviewed. | Data | Accepted |
| The asset identity edit refuses a retired status or end-of-life date from any path; retirement goes only through Déclasser. | Data | Accepted |

## Analytics and finance

| Limit | Impact | Handled in |
|---|---|---|
| The cost tile: a work order settled before the period and reopened inside it shows as a negative cost in the current period (consistent with the Finance cockpit). | Data | Accepted |

## Assets

| Limit | Impact | Handled in |
|---|---|---|
| The equipment tree reads each branch from the database when opened. It follows active hierarchy links only; equipment with several active parents appears under each; "Expand all" opens only branches already loaded. | UX | Accepted |
| IoT and ERP "linked records" are yes/no flags, not counts. | UX | After pilot |

## App frame, sync centre, documents, organisation, budget

| Limit | Impact | Handled in |
|---|---|---|
| There is no search across all record types; the top-bar box opens modules (Ctrl K). | UX | After pilot |
| An organisation unit's cost centre is free text, not picked from finance cost centres (needs a decision on which permission may set it). | Data | Owner decision |
| The sync centre names about 40 record types; others show as "Other record" with their number. | UX | Accepted |

## In progress (program side done, screens being wired)

| Limit | Impact | Handled in |
|---|---|---|
| Personnel and document list screens follow Données de référence: an edit there shows at once; a change received by sync shows when a screen opens and its copy is more than 5 minutes old. | UX | Accepted |
| A planner without the personnel view right sees absence codes (not names) on the planning board, coloured only as critical or plain. | UX | Step 6 (roles) |
| A person's skills and certifications are edited only in the Qualifications tab, not in the person form (which points there). | UX | Accepted |

## Activity feed and audit log

| Limit | Impact | Handled in |
|---|---|---|
| Activity events are shown with translated names (every event code the program writes today is covered); event names added to the program later show in a readable form until their translation is added. | UX | Accepted |

## Step 5 (old-layout screens) — in progress

| Limit | Impact | Handled in |
|---|---|---|
| Work-order and request boards are read-only: each status change needs its own dialog (urgency, reason, permit checks). Purchasing boards allow submit/approve by drag, menu or keyboard through the same server step. | UX | Accepted |
| Work-order and request boards load each lane from the server with its own count and "load more"; opening a board sends one request per lane (7 or 6, in parallel). | UX | Accepted |
| Screens still old or mixed (in progress): the training certification-catalogue and qualification-requirements dialogs, asset import wizard, diagnostics page, permit types dialog (to move under Données de référence), permit hand-over form, and the remaining reliability tools (Ishikawa, decision log, rating scales). | UX | Step 5 |

## Modules delivered before this register

Inventory (issue desk, requisitions, valuation, lots, FX, approval limits), RAMS, PM governance,
analytics/SMRP, finance (cost flow, settlement, budget cockpit), personnel/qualifications and
inspections were delivered with their limits reported in chat and in some PR bodies only. Their
limits were not recorded systematically at the time. **Step 7** re-walks each of these modules in
the running app with the demo plant and adds every limit found here.
