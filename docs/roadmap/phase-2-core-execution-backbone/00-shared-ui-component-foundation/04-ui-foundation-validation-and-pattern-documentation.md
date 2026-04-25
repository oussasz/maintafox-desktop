# Phase 2 · Sub-phase 00 · File 04
# UI Foundation Validation and Pattern Documentation

## Context and Purpose

Files 01–03 of this sub-phase installed the shared UI component library (Shadcn/ui),
data table engine (TanStack Table), chart primitives (D3.js), form infrastructure
(React Hook Form + Zod), and shell refinements (role-scoped sidebar, command palette,
notification wiring). This final file validates the entire UI foundation through
integration tests and produces a pattern documentation page that Phase 2 module
developers reference when building their screens.

**Gap addressed:** Ensures the Category B and Category E gap fixes are validated
end-to-end before Phase 2 modules begin consuming them.

## Architecture Rules Applied

- **Pattern page is dev-only.** The component showcase page is accessible only in
  development builds (`import.meta.env.DEV`). It is tree-shaken from production.
- **Tests cover the integration surface.** Each test verifies that a Shadcn component
  renders correctly with the Tailwind token system — confirming no CSS variable
  conflicts between Shadcn defaults and Maintafox tokens.
- **Accessibility baseline.** All Shadcn components are Radix-based and WCAG 2.1 AA
  compliant by default. Tests verify `role`, `aria-*` attributes are preserved.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src/pages/dev/ComponentShowcase.tsx` | Dev-only page showing all base components |
| `src/__tests__/ui-foundation.test.tsx` | Integration tests for Shadcn + Tailwind token compatibility |
| `src/__tests__/data-table.test.tsx` | DataTable rendering and pagination tests |
| `docs/UI_COMPONENT_PATTERNS.md` | Developer reference for form, table, and chart patterns |

## Prerequisites

- SP00-F01 through F03 complete: all components installed and configured
- Phase 1 test suite passing (108+ tests)

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Component Rendering Integration Tests | `ui-foundation.test.tsx`, `data-table.test.tsx` |
| S2 | Component Showcase Dev Page | `ComponentShowcase.tsx`, dev route |
| S3 | Pattern Documentation and Phase 2 Readiness Gate | `UI_COMPONENT_PATTERNS.md`, readiness checklist |

---

## Sprint S1 — Component Rendering Integration Tests

### AI Agent Prompt

```
You are a test engineer. Write Vitest integration tests that verify Shadcn components
render correctly with the Maintafox Tailwind token system.

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE src/__tests__/ui-foundation.test.tsx
────────────────────────────────────────────────────────────────────

Test each base component renders without errors:
- Button (default, primary, danger variants)
- Input with placeholder
- Label with htmlFor
- Dialog opens and closes
- DropdownMenu opens and shows items
- Tabs switches content panels
- Badge renders with text
- Card renders header and content

Each test uses `@testing-library/react` render() and screen queries.

────────────────────────────────────────────────────────────────────
STEP 2 — CREATE src/__tests__/data-table.test.tsx
────────────────────────────────────────────────────────────────────

Test the DataTable component:
- Renders correct number of rows
- Shows empty state when data is []
- Pagination shows correct page count
- Sorting toggles column order

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm test` passes — all tests (Phase 1 + new) pass
- At least 12 new test cases added
- Zero accessibility warnings in test output
```

---

### Supervisor Verification — Sprint S1

**V1 — All tests pass.**
Run `pnpm test`. Confirm zero failures.

---

## Sprint S2 — Component Showcase Dev Page

### AI Agent Prompt

```
Create a dev-only page that showcases all base components for visual review.

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE src/pages/dev/ComponentShowcase.tsx
────────────────────────────────────────────────────────────────────

A page that renders sections for:
- Buttons (all variants)
- Form fields (Input, Textarea, Select with FormField wrapper)
- Dialog and Sheet examples
- DataTable with sample data
- BarChart and LineChart with sample data
- Badge variants
- Card layout

The page is only accessible when `import.meta.env.DEV` is true.

────────────────────────────────────────────────────────────────────
STEP 2 — Add dev route
────────────────────────────────────────────────────────────────────

In router.tsx, add a conditional route:

```typescript
...(import.meta.env.DEV
  ? [{ path: "__dev/components", element: <ComponentShowcase /> }]
  : []),
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run dev` → navigate to `/__dev/components` → all components visible
- The page does not appear in production builds
- `pnpm run typecheck` passes
```

---

### Supervisor Verification — Sprint S2

**V1 — Showcase page renders.**
Run `pnpm run dev`. Navigate to `/__dev/components`. All component sections
should be visible with correct styling.

---

## Sprint S3 — Pattern Documentation and Phase 2 Readiness Gate

### AI Agent Prompt

```
Create the UI component pattern documentation and a readiness checklist.

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE docs/UI_COMPONENT_PATTERNS.md
────────────────────────────────────────────────────────────────────

A markdown reference that covers:
1. **Form pattern**: How to use `useZodForm()` + `FormField` + Shadcn inputs
2. **Table pattern**: How to define `ColumnDef[]` and render `DataTable`
3. **Dialog pattern**: How to use Dialog for create/edit forms
4. **Chart pattern**: How to use BarChart/LineChart with module data
5. **Permission gating**: How to use `PermissionGate` and sidebar filtering
6. **i18n in components**: How to use `useTranslation(namespace)` in module pages

Each pattern includes a complete code example.

────────────────────────────────────────────────────────────────────
STEP 2 — Phase 2 Readiness Checklist
────────────────────────────────────────────────────────────────────

At the end of the document, add a checklist:

```markdown
## Phase 2 UI Readiness Checklist

- [ ] Shadcn/ui: 12 base components generated and barrel-exported
- [ ] React Hook Form: useZodForm helper tested
- [ ] TanStack Table: DataTable with pagination/sorting tested
- [ ] D3.js: BarChart and LineChart render correctly
- [ ] Role-scoped sidebar: permission filtering works
- [ ] Command palette: ⌘K opens and navigates
- [ ] Notification bell: polling hook installed (backend pending SP07)
- [ ] User menu: logout, profile, settings links work
- [ ] All Phase 1 tests still pass
- [ ] TypeCheck: 0 errors | i18n:check: 0 errors
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `docs/UI_COMPONENT_PATTERNS.md` exists with all 6 patterns documented
- The readiness checklist has 10 items
- All items in the checklist are verifiable by the supervisor
```

---

### Supervisor Verification — Sprint S3

**V1 — Documentation exists and is complete.**
Open `docs/UI_COMPONENT_PATTERNS.md`. Confirm all 6 patterns are documented
with code examples.

**V2 — Readiness checklist passes.**
Walk through each of the 10 checklist items. All must pass before Phase 2
module work (SP01) begins.

**V3 — Full regression check.**
Run `pnpm test && pnpm run typecheck && pnpm run i18n:check`. All pass.

---

*End of Phase 2 · Sub-phase 00 · File 04*
