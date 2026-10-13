# Availability Tab — Functional Specification

**Status:** Product lock (approved — ResourceTimeline era)  
**Audience:** Planners, supervisors, maintenance managers  
**Related:** `docs/engineering/RESOURCE_TIMELINE.md`

This document is the **to-be** contract for the Availability tab.

---

## Locked product decisions

1. **Not HR.** Scope is maintenance planning support only. No employment-type, origin, status, hire, or directory CRUD on this surface.
2. **Single question.** Every element must help answer: *Can I assign this technician to maintenance work during this planning horizon?*
3. **Not a scheduler.** No WO assignment, drag-and-drop scheduling, or workload balancing from this tab. ResourceTimeline is **visibility only**.
4. **Blocks stay here.** Leave / Medical / Restriction / Training blocks are created and managed here as **contextual** actions (not a permanent top form).
5. **Primary view = ResourceTimeline (people × time).** Chronology with zoom. The People × Days matrix is **removed**.
6. **Skills/certs:** warning badges only (e.g. qualification expired). No management UI.
7. **Filters:** Entity, Team, Person, Block type, Critical only, Horizon. No HR filters / no search-as-directory.
8. **Analytics:** compact operational summary only; detailed reports remain on Analytics.
9. **Detail chrome:** block/person inspection uses **split pane** (`moduleWorkspaceSplit`), same family as Directory/Equipment — not a centered Dialog for inspection. Create/edit forms remain dialogs.
10. **Header views:** secondary views use **icon-only** `mfLayout.viewToggleGroup` in the page header (WO/DI pattern).
11. **Delivery:** implement against this spec + ResourceTimeline DS primitive.

---

## 1. Purpose and non-goals

### Purpose

Expose **who is available, partially available, blocked, or critically constrained** over a planning horizon so the user can decide assignability before committing work elsewhere (Planning / Work Orders).

### Primary user question

> Can I assign this technician to maintenance work during this planning horizon?

### Non-goals (explicit)

- HR master data, directory list/cards, import/export personnel
- Work order assignment / reschedule from this surface
- Training/certification program management
- Team composition / succession editing (Teams module)
- Heavy analytics, CSV workforce reports (Analytics tab)
- People × Days matrix (superseded by ResourceTimeline)

---

## 2. Actors and permissions

| Actor | Typical need |
|-------|----------------|
| Planner | Scan crew coverage; avoid assigning blocked techs |
| Supervisor | See who is out / restricted this week |
| Maintenance manager | Spot critical constraints and coverage risk |

**Permissions:**

- View chronology / summary: `per.view` (`list_availability_calendar`, block lists)
- Create / edit / end blocks: `per.manage`
- Hide or disable mutating actions when unauthorized

---

## 3. Core concepts

| Concept | Definition |
|---------|------------|
| **Horizon** | Inclusive date range from `RESOURCE_TIMELINE_HORIZON_PRESETS` or custom from–to |
| **Person row** | Active technician in scope (ResourceTimeline group) |
| **Block item** | Interval bar on the chronology (leave/medical/restriction/training) |
| **Day aggregate** | Backend calendar row used for summary KPIs (not a matrix UI) |
| **Critical block** | Safety-relevant constraint; medical/restriction always critical |
| **Coverage risk** | Days in horizon where free headcount in filtered scope is zero |

Blocks are **inputs**. ResourceTimeline is the **decision surface**.

---

## 4. Information architecture

```text
Availability tab
├── Page header
│   Create block · Refresh · Icon view toggles (chronology | list)
├── Context / filter bar
│   Horizon · Entity · Team · Person · Block type · Critical only
├── Operational summary (compact)
├── Split workspace
│   ├── Primary: ResourceTimeline (people × time, zoom)
│   └── AvailabilityInspectorWorkspace (assignability / constraints for the selected person or block)
└── Contextual dialogs
    Create / edit block
```

**Inspector vs Directory:** The right-hand `AvailabilityInspectorWorkspace` answers only *can I assign this person over this horizon / what blocks them?* It is not the Directory person master (`PersonnelDetailWorkspace`). Open directory via the explicit deep-link action when HR/identity detail is needed; do not clone Directory tabs into the Availability pane.

---

## 5. Filters (normative)

| Filter | Behavior |
|--------|----------|
| **Horizon** | Required. Presets from DS tokens (7 / 14 / 28) + custom from–to. |
| **Entity** | Org/site scope. |
| **Team** | Restrict to one crew; empty = all in entity scope. |
| **Person** | Restrict to one person. |
| **Block type** | People with ≥1 matching block in horizon. |
| **Critical only** | People with critical constraints in horizon. |
| **Refresh** | Reloads calendar + blocks. |

**Removed:** search, position, employment type, origin, employment status, matrix view.

---

## 6. Operational summary

1. **Available technicians** — majority of horizon days Free.
2. **Blocked technicians** — ≥1 fully Blocked or Critical day.
3. **Critical blocks** — person-days with critical constraint.
4. **Coverage risk** — days with zero free headcount.

---

## 7. Primary visualization — ResourceTimeline

- Renderer: Design System `ResourceTimeline` only.
- Colors: semantic tokens (`free` / `partial` / `blocked` / `critical` / `rest` / block types).
- Zoom: primitive chrome (in / out / fit-to-horizon).
- Click person/group or block → split detail workspace.
- No drag-assign of WOs.

### Secondary view

- **List:** compact read-only day/person list for scan/export-friendly reading — header icon toggle.

---

## 8. Availability blocks (contextual)

### Types

Leave · Medical · Restriction · Training

### Rules

- Create / edit / end only with `per.manage`
- Medical and Restriction: always critical
- Hard overlap of capacity-zeroing blocks: reject on save

### UX placement

- Toolbar “Create block”, detail workspace actions
- Form in **dialog**; inspection in **split pane**

---

## 9. Warning badges (skills / certifications)

Display on person row / detail only. No cert management on this tab.

---

## 10. Module boundaries

| Belongs here | Belongs elsewhere |
|--------------|-------------------|
| Horizon coverage & block chronology | WO assign / Planning board |
| Create/edit availability blocks | Personnel master CRUD |
| Compact coverage summary | Analytics reports / CSV |
| Cert **warning** badges | Cert lifecycle management |
| Team as **filter** | Team structure editing |

---

## 11. Acceptance criteria

1. Planner selects horizon + filters and sees ResourceTimeline without directory HR chrome.
2. Bar/state colors answer assignability without opening WOs.
3. Create block is contextual and does not dominate the page.
4. Critical medical/restriction behavior remains enforced.
5. No control assigns work or reschedules jobs.
6. Summary shows only the four operational counts.
7. View toggles are icon-only in the page header (WO/DI pattern).
8. Unauthorized users cannot mutate blocks.
9. Matrix is not available as a view.

---

## 12. Color / zoom standards

See `docs/engineering/RESOURCE_TIMELINE.md` and `.cursor/rules/resource-timeline-horizon-colors.mdc`.
