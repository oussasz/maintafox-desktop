# Phase 2 · Sub-phase 00 · File 01
# Component Library Installation and Form Infrastructure

## Context and Purpose

Phase 1 delivered the complete engineering foundation: build tooling, Tauri shell,
database, authentication, i18n, and settings persistence. The UI during Phase 1 used
plain HTML elements styled with Tailwind CSS — sufficient for the login flow and shell
layout, but not for the data-heavy forms, tables, modals, and dialogs that Phase 2
modules require.

The PRD (§4.1) specifies the following UI stack:
- **Shadcn/ui + Radix UI** — accessible headless primitives with Tailwind styling
- **React Hook Form + Zod** — form state management with schema-based validation
- **TanStack Table 8.x** — headless table engine for data grids
- **D3.js 7.x** — data visualization for analytics and reliability modules

This file installs Shadcn/ui, configures the base component set, and establishes
the React Hook Form + Zod form patterns that all Phase 2 modules will use. Without
this foundation, every module team would independently install and configure these
libraries, leading to inconsistent patterns and duplicated effort.

**Gap addressed:** Category B from the Phase 1 gap analysis — the PRD specifies these
libraries but no roadmap step installs or configures them.

## Architecture Rules Applied

- **Shadcn/ui is copy-paste, not a dependency.** Components are generated into
  `src/components/ui/` and become project-owned code. They are styled using the
  existing Tailwind token system (colors, spacing, typography from SP02-F03).
- **Radix UI primitives are the runtime dependency.** Shadcn generates components
  that import from `@radix-ui/react-*` packages. These are the actual npm deps.
- **Form patterns use Zod for validation.** React Hook Form's `zodResolver` bridges
  form state to Zod schemas. The same schemas are reused for IPC response validation
  (established in SP04, SP05, SP06 service files).
- **Single source of truth for component exports.** All Shadcn components are
  re-exported from `src/components/ui/index.ts` so module imports are clean:
  `import { Button, Dialog, Input } from "@/components/ui"`.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| Shadcn/ui CLI init + components | Button, Input, Label, Textarea, Select, Dialog, Sheet, DropdownMenu, Card, Badge, Separator, Tabs |
| React Hook Form + Zod setup | `@hookform/resolvers/zod`, typed form helpers |
| `src/components/ui/index.ts` | Barrel export for all UI primitives |
| `src/lib/form-helpers.ts` | `useZodForm()` helper wrapping react-hook-form with zodResolver |
| `src/components/ui/FormField.tsx` | Reusable form field wrapper with label, error display, and description |

## Prerequisites

- SP02-F03 complete: Tailwind config with design tokens, `cn()` utility, globals.css
- SP01-F03 complete: `pnpm` workspace, TypeScript paths, Vite config
- Node/pnpm toolchain operational

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Shadcn/ui Installation and Base Components | CLI init, 12 components, barrel export |
| S2 | React Hook Form and Zod Form Infrastructure | hookform resolvers, `useZodForm`, `FormField` |
| S3 | Form Pattern Validation and Component Smoke Tests | Vitest tests for form helpers, component rendering |

---

## Sprint S1 — Shadcn/ui Installation and Base Components

### AI Agent Prompt

```
You are a TypeScript and React engineer. The Tailwind design token system is in place
(tailwind.config.ts, globals.css, cn() utility). Your task is to install Shadcn/ui and
generate the base component set required by Phase 2 modules.

────────────────────────────────────────────────────────────────────
STEP 1 — Initialize Shadcn/ui
────────────────────────────────────────────────────────────────────

Run the Shadcn/ui CLI init command with the following configuration:
- Style: "default"
- Base color: use existing (do NOT override tailwind.config.ts)
- CSS variables: yes (already in globals.css)
- Components path: src/components/ui
- Utils path: src/lib/utils (already exists with cn())

```bash
pnpm dlx shadcn@latest init
```

When prompted, confirm the path aliases match the existing tsconfig paths:
- `@/components` → `src/components`
- `@/lib` → `src/lib`

If the CLI creates a `components.json`, ensure it points to the existing paths.
Do NOT let the CLI overwrite `tailwind.config.ts` or `globals.css` — those already
have the Maintafox token system. If the CLI generates a new `utils.ts`, merge
the `cn()` function from the existing `src/lib/utils.ts`.

────────────────────────────────────────────────────────────────────
STEP 2 — Generate base components
────────────────────────────────────────────────────────────────────

Generate the 12 components required by Phase 2 modules:

```bash
pnpm dlx shadcn@latest add button input label textarea select \
  dialog sheet dropdown-menu card badge separator tabs
```

These components will be generated into `src/components/ui/`. After generation,
review each file and ensure:
1. The `cn()` import resolves to `@/lib/utils`
2. Tailwind classes use the existing token variables (e.g., `bg-primary` not
   hardcoded hex values)
3. No conflicting exports with the existing `ThemeProvider.tsx` in the same folder

────────────────────────────────────────────────────────────────────
STEP 3 — Create barrel export
────────────────────────────────────────────────────────────────────

Create `src/components/ui/index.ts` that re-exports all generated components:

```typescript
// src/components/ui/index.ts
// Barrel export for all UI primitives.
// Module code imports from "@/components/ui" — never from individual files.

export { Button, buttonVariants } from "./button";
export { Input } from "./input";
export { Label } from "./label";
export { Textarea } from "./textarea";
export {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./select";
export {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "./dialog";
export {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "./sheet";
export {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./dropdown-menu";
export { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "./card";
export { Badge, badgeVariants } from "./badge";
export { Separator } from "./separator";
export { Tabs, TabsContent, TabsList, TabsTrigger } from "./tabs";
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes with 0 errors
- `pnpm run dev` renders without console errors
- `import { Button, Dialog, Input } from "@/components/ui"` resolves correctly
- The Button component renders with the existing `bg-primary` token color
- No existing component (ThemeProvider, etc.) is broken by the new files
```

---

### Supervisor Verification — Sprint S1

**V1 — Components are generated.**
List `src/components/ui/`. Confirm you see files for all 12 components plus
`index.ts` and the existing `ThemeProvider.tsx`.

**V2 — TypeScript compiles.**
Run `pnpm run typecheck`. Zero errors.

**V3 — Existing tests still pass.**
Run `pnpm test`. All Phase 1 tests must pass — no regressions.

---

## Sprint S2 — React Hook Form and Zod Form Infrastructure

### AI Agent Prompt

```
You are a TypeScript and React engineer. Shadcn/ui components are installed. Your task
is to set up React Hook Form with Zod resolver and create reusable form helpers.

────────────────────────────────────────────────────────────────────
STEP 1 — Install React Hook Form and resolvers
────────────────────────────────────────────────────────────────────

```bash
pnpm add react-hook-form @hookform/resolvers
```

Zod is already installed (used by auth-service.ts and other service files).

────────────────────────────────────────────────────────────────────
STEP 2 — CREATE src/lib/form-helpers.ts
────────────────────────────────────────────────────────────────────

```typescript
/**
 * form-helpers.ts
 *
 * Typed wrapper around react-hook-form with Zod resolver.
 * All Phase 2 forms use this helper to ensure consistent
 * validation behavior and error display patterns.
 */

import { zodResolver } from "@hookform/resolvers/zod";
import {
  useForm,
  type DefaultValues,
  type FieldValues,
  type UseFormReturn,
} from "react-hook-form";
import type { ZodType } from "zod";

/**
 * Create a react-hook-form instance pre-configured with a Zod schema.
 *
 * Usage:
 * ```tsx
 * const schema = z.object({ name: z.string().min(1) });
 * type FormData = z.infer<typeof schema>;
 * const form = useZodForm(schema, { name: "" });
 * ```
 */
export function useZodForm<T extends FieldValues>(
  schema: ZodType<T>,
  defaultValues?: DefaultValues<T>,
): UseFormReturn<T> {
  return useForm<T>({
    resolver: zodResolver(schema),
    defaultValues,
    mode: "onBlur",
  });
}
```

────────────────────────────────────────────────────────────────────
STEP 3 — CREATE src/components/ui/FormField.tsx
────────────────────────────────────────────────────────────────────

```tsx
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Label } from "./label";

interface FormFieldProps {
  /** Unique field identifier (used for htmlFor and aria-describedby) */
  name: string;
  /** Field label text */
  label: string;
  /** Optional description shown below the label */
  description?: string;
  /** Error message string (from react-hook-form errors) */
  error?: string;
  /** Whether the field is required */
  required?: boolean;
  /** The form control (input, textarea, select, etc.) */
  children: ReactNode;
  /** Additional className for the wrapper */
  className?: string;
}

/**
 * Reusable form field wrapper. Provides consistent label, description,
 * and error display for all form controls.
 */
export function FormField({
  name,
  label,
  description,
  error,
  required,
  children,
  className,
}: FormFieldProps) {
  return (
    <div className={cn("space-y-1.5", className)}>
      <Label htmlFor={name} className={cn(error && "text-status-danger")}>
        {label}
        {required && <span className="text-status-danger ml-0.5">*</span>}
      </Label>
      {description && (
        <p
          id={`${name}-description`}
          className="text-xs text-text-muted"
        >
          {description}
        </p>
      )}
      {children}
      {error && (
        <p
          id={`${name}-error`}
          role="alert"
          className="text-xs text-status-danger"
        >
          {error}
        </p>
      )}
    </div>
  );
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes
- `useZodForm(schema, defaults)` returns a properly typed form instance
- `FormField` renders label, description, and error decorations correctly
- Zod resolver validates form data against schema on blur
```

---

### Supervisor Verification — Sprint S2

**V1 — Dependencies installed.**
Run `pnpm list react-hook-form @hookform/resolvers`. Both must be present.

**V2 — TypeScript types resolve.**
Run `pnpm run typecheck`. Confirm the `UseFormReturn<T>` type from react-hook-form
resolves through the Zod generic correctly.

---

## Sprint S3 — Form Pattern Validation and Component Smoke Tests

### AI Agent Prompt

```
You are a test engineer. The Shadcn components and form infrastructure are installed.
Write smoke tests to verify the components render and the form helper works.

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE src/lib/__tests__/form-helpers.test.ts
────────────────────────────────────────────────────────────────────

```typescript
import { renderHook, act } from "@testing-library/react";
import { z } from "zod";
import { describe, it, expect } from "vitest";

import { useZodForm } from "../form-helpers";

describe("useZodForm", () => {
  const schema = z.object({
    name: z.string().min(1, "Name is required"),
    email: z.string().email("Invalid email"),
  });

  it("initializes with default values", () => {
    const { result } = renderHook(() =>
      useZodForm(schema, { name: "", email: "" }),
    );
    expect(result.current.getValues()).toEqual({ name: "", email: "" });
  });

  it("reports validation errors for invalid data", async () => {
    const { result } = renderHook(() =>
      useZodForm(schema, { name: "", email: "not-an-email" }),
    );

    let isValid = true;
    await act(async () => {
      isValid = await result.current.trigger();
    });

    expect(isValid).toBe(false);
    expect(result.current.formState.errors.name?.message).toBe(
      "Name is required",
    );
    expect(result.current.formState.errors.email?.message).toBe(
      "Invalid email",
    );
  });

  it("passes validation for correct data", async () => {
    const { result } = renderHook(() =>
      useZodForm(schema, { name: "John", email: "john@example.com" }),
    );

    let isValid = false;
    await act(async () => {
      isValid = await result.current.trigger();
    });

    expect(isValid).toBe(true);
    expect(result.current.formState.errors).toEqual({});
  });
});
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm test` passes — all existing tests + new form helper tests pass
- The useZodForm helper is validated as producing correct validation errors
- Total test count increases from Phase 1 baseline (108) by at least 3
```

---

### Supervisor Verification — Sprint S3

**V1 — All tests pass.**
Run `pnpm test`. Confirm zero failures and the new `form-helpers.test.ts`
appears in the output.

**V2 — No regressions.**
Run `pnpm run typecheck && pnpm run i18n:check`. Both must exit 0.

---

*End of Phase 2 · Sub-phase 00 · File 01*
