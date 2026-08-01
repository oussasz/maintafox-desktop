# UI Component Patterns

> Phase 2 developer reference — SP00-F04-S3
>
> This document describes the canonical patterns for building Phase 2 module
> screens. Every code example uses the **actual** helpers and components
> installed in SP00-F01 through F03. Import paths reference the barrel
> exports — never import from individual files.

---

## 1. Form Pattern

Use `useZodForm()` from `@/lib/form-helpers` for type-safe validation,
`FormField` from `@/components/ui/FormField` for consistent label/error
layout, and Shadcn inputs from `@/components/ui`.

```tsx
import { z } from "zod";
import { useZodForm } from "@/lib/form-helpers";
import { FormField } from "@/components/ui/FormField";
import { Button, Input, Textarea, Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui";

const schema = z.object({
  code:        z.string().min(1, "Code requis"),
  designation: z.string().min(3, "Minimum 3 caracteres"),
  category:    z.string().min(1, "Categorie requise"),
  notes:       z.string().optional(),
});

type FormData = z.infer<typeof schema>;

export function EquipmentForm({ onSubmit }: { onSubmit: (data: FormData) => void }) {
  const { register, handleSubmit, formState: { errors } } = useZodForm(schema, {
    code: "",
    designation: "",
    category: "",
    notes: "",
  });

  return (
    <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
      <FormField name="code" label="Code" required error={errors.code?.message}>
        <Input id="code" {...register("code")} placeholder="EQ-0001" />
      </FormField>

      <FormField name="designation" label="Designation" required error={errors.designation?.message}>
        <Input id="designation" {...register("designation")} />
      </FormField>

      <FormField name="category" label="Categorie" required error={errors.category?.message}>
        {/* For Select with react-hook-form, use Controller or register manually */}
        <Select>
          <SelectTrigger id="category">
            <SelectValue placeholder="Choisir..." />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="pompe">Pompe</SelectItem>
            <SelectItem value="moteur">Moteur</SelectItem>
            <SelectItem value="vanne">Vanne</SelectItem>
          </SelectContent>
        </Select>
      </FormField>

      <FormField name="notes" label="Notes" error={errors.notes?.message}>
        <Textarea id="notes" {...register("notes")} rows={3} />
      </FormField>

      <Button type="submit">Enregistrer</Button>
    </form>
  );
}
```

**Key rules:**
- `useZodForm(schema, defaults)` configures `mode: "onBlur"` by default.
- `FormField` renders label, optional description, error message, and `required` indicator.
- Error is displayed only when the string is truthy — pass `errors.field?.message`.

---

## 2. Table Pattern

Use `ColumnDef[]` from `@tanstack/react-table` and `DataTable` from
`@/components/data`.

```tsx
import type { ColumnDef } from "@tanstack/react-table";
import { DataTable } from "@/components/data";
import { Badge } from "@/components/ui";

interface Equipment {
  id: number;
  code: string;
  designation: string;
  status: "en_service" | "hors_service" | "en_attente";
}

const columns: ColumnDef<Equipment, unknown>[] = [
  { accessorKey: "code", header: "Code", enableSorting: true },
  { accessorKey: "designation", header: "Designation", enableSorting: true },
  {
    accessorKey: "status",
    header: "Statut",
    enableSorting: false,
    cell: ({ row }) => {
      const status = row.getValue<string>("status");
      const variant = status === "en_service" ? "default"
                    : status === "hors_service" ? "destructive"
                    : "secondary";
      return <Badge variant={variant}>{status}</Badge>;
    },
  },
];

export function EquipmentList({ data }: { data: Equipment[] }) {
  return (
    <DataTable
      columns={columns}
      data={data}
      searchable
      searchPlaceholder="Rechercher un equipement..."
      pageSize={10}
      onRowClick={(row) => console.log("Selected:", row.id)}
    />
  );
}
```

**Key rules:**
- `enableSorting` controls whether a column header is clickable.
- `DataTable` handles pagination, global search, sorting, and empty state internally.
- Use the `cell` render function for custom formatting (badges, links, icons).
- `onRowClick` receives the original data object (`TData`), not a TanStack row.
- `isLoading` + `skeletonRows` shows animated placeholders while data loads.

---

## 3. Dialog Pattern

Use `Dialog` from `@/components/ui` to wrap create/edit forms in a modal.

```tsx
import { useState } from "react";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  Input,
} from "@/components/ui";
import { FormField } from "@/components/ui/FormField";

export function CreateEquipmentDialog() {
  const [open, setOpen] = useState(false);

  const handleSave = () => {
    // validate + persist via IPC...
    setOpen(false);
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>Nouveau equipement</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Creer un equipement</DialogTitle>
          <DialogDescription>
            Renseignez les informations de base de l'equipement.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <FormField name="code" label="Code" required>
            <Input id="code" placeholder="EQ-0001" />
          </FormField>
          <FormField name="designation" label="Designation" required>
            <Input id="designation" />
          </FormField>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            Annuler
          </Button>
          <Button onClick={handleSave}>Enregistrer</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

**Key rules:**
- Control open state with `open` / `onOpenChange` for programmatic close after save.
- `DialogTrigger asChild` delegates the click to the child `Button`.
- Always include `DialogTitle` and `DialogDescription` for accessibility (WCAG).
- For side panels, use `Sheet` / `SheetContent` / `SheetTrigger` with the same pattern.

---

## 4. Chart Pattern

Use `BarChart` and `LineChart` from `@/components/charts`. D3 handles scales
and axes; React handles rendering.

```tsx
import { BarChart, type BarChartDatum } from "@/components/charts";
import { LineChart, type LineChartDatum } from "@/components/charts";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui";

// Bar chart: categorical data (e.g., OT count per month)
const barData: BarChartDatum[] = [
  { label: "Jan", value: 42 },
  { label: "Fev", value: 28 },
  { label: "Mar", value: 55 },
];

// Line chart: numerical/time-series data (e.g., MTBF over weeks)
const lineData: LineChartDatum[] = [
  { x: 1, y: 120 },
  { x: 2, y: 135 },
  { x: 3, y: 142 },
  { x: 4, y: 128 },
];

export function DashboardCharts() {
  return (
    <div className="grid gap-6 sm:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle>OT par mois</CardTitle>
        </CardHeader>
        <CardContent>
          {/* Charts fill their container — wrap in a div with a fixed height */}
          <div className="h-[250px]">
            <BarChart data={barData} />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>MTBF trend</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="h-[250px]">
            <LineChart data={lineData} showDots />
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
```

**Key rules:**
- Charts auto-size to their container via `useContainerSize`. Always provide a
  parent with an explicit height (`h-[250px]`, `h-64`, etc.).
- `BarChartDatum` = `{ label: string; value: number }`.
- `LineChartDatum` = `{ x: number | Date; y: number }`.
- Override colors via the `color` prop (defaults to the design-token primary).
- For explicit dimensions, pass `width` / `height` props (px).

---

## 5. Permission Gating

Use `PermissionGate` from `@/components/PermissionGate` for declarative
rendering, and `usePermissions()` from `@/hooks/use-permissions` for
imperative checks.

```tsx
import { PermissionGate } from "@/components/PermissionGate";
import { usePermissions } from "@/hooks/use-permissions";
import { Button } from "@/components/ui";

// ── Declarative: hide UI entirely if user lacks permission ──────

export function EquipmentActions() {
  return (
    <div className="flex gap-2">
      {/* Edit button: visible only if user has eq.manage */}
      <PermissionGate permission="eq.manage">
        <Button>Modifier</Button>
      </PermissionGate>

      {/* Delete button: show "not authorized" text as fallback */}
      <PermissionGate permission="eq.delete" fallback={<span className="text-text-muted text-sm">Non autorise</span>}>
        <Button variant="destructive">Supprimer</Button>
      </PermissionGate>
    </div>
  );
}

// ── Imperative: conditional logic inside a component ────────────

export function EquipmentPage() {
  const { can, isLoading } = usePermissions();

  if (isLoading) return null;

  const canCreate = can("eq.create");

  return (
    <div>
      <h1>Equipements</h1>
      {canCreate && <Button>Creer</Button>}
      {/* ... table, etc. */}
    </div>
  );
}
```

**Permission domains (from PRD §6.7):**
- `eq.*` — Equipment
- `di.*` — Intervention Requests
- `ot.*` — Work Orders
- `pm.*` — Preventive Maintenance
- `inv.*` — Inventory
- `adm.*` — Admin (users, settings)
- `arc.*` — Archive
- `doc.*` — Documentation
- `erp.*` — ERP Connector
- Full domain list: see `nav-registry.tsx` → `requiredPermission` field.

**Sidebar filtering:** The `Sidebar` component already calls `usePermissions().can()`
for each nav item's `requiredPermission`. No extra code is needed — items the user
cannot access are automatically hidden.

---

## 6. i18n in Components

Use `useTranslation(namespace)` from `react-i18next`. Namespaces are defined
in `src/i18n/namespaces.ts`.

```tsx
import { useTranslation } from "react-i18next";

// ── Eager namespace (available immediately) ─────────────────────

export function AppHeader() {
  const { t } = useTranslation("common");
  return <h1>{t("app.name")}</h1>;
}

// ── Module namespace (lazy-loaded on first route visit) ─────────

export function EquipmentPage() {
  const { t } = useTranslation("equipment");
  const { t: tc } = useTranslation("common"); // for shared labels

  return (
    <div>
      <h1>{t("page.title")}</h1>
      <p>{t("page.description")}</p>
      <button>{tc("action.save")}</button>
    </div>
  );
}

// ── Interpolation ───────────────────────────────────────────────

// In fr/equipment.json:
// { "detail.title": "Equipement {{code}}" }

export function EquipmentDetail({ code }: { code: string }) {
  const { t } = useTranslation("equipment");
  return <h2>{t("detail.title", { code })}</h2>;
}
```

**Namespace categories:**

| Category | Namespaces | Loading |
|----------|-----------|---------|
| Eager | `common`, `auth`, `errors`, `validation`, `formats`, `shell` | Bundled — available at frame 0 |
| Module | `equipment`, `di`, `ot`, `org`, `personnel`, `reference`, `inventory`, `pm`, `planning`, `permits`, `inspections`, `training`, `reliability`, `budget`, `reports`, `archive`, `notifications`, `documentation`, `iot`, `erp`, `activity`, `users`, `settings`, `diagnostics`, `configuration` | Lazy-loaded via `i18next-resources-to-backend` |

**Key rules:**
- French is the primary locale (`fr`). English (`en`) is the fallback.
- Eager namespaces live in `src/i18n/fr/` and `src/i18n/en/`.
- Module namespaces live in `src/i18n/locale-data/{lng}/{ns}.json`.
- Never hardcode user-facing strings — always use `t()`.
- Missing keys show as `[namespace.key]` in dev mode (debug logging enabled).

---

## Phase 2 UI Readiness Checklist

- [ ] Shadcn/ui: 14 base components generated and barrel-exported
- [ ] React Hook Form: useZodForm helper tested
- [ ] TanStack Table: DataTable with pagination/sorting tested
- [ ] D3.js: BarChart and LineChart render correctly
- [ ] Role-scoped sidebar: permission filtering works
- [ ] Command palette: Ctrl+K opens and navigates
- [ ] Notification bell: polling hook installed (backend pending SP07)
- [ ] User menu: logout, profile, settings links work
- [ ] All Phase 1 tests still pass
- [ ] TypeCheck: 0 errors | i18n:check: 0 errors
