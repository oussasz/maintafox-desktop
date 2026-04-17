// src/pages/dev/ComponentShowcase.tsx
// Phase 2 · SP00-F04 · S2 — Dev-only component showcase page.
// Accessible at /__dev/components in development builds only.
// Tree-shaken from production via import.meta.env.DEV gating in router.tsx.

import type { ColumnDef } from "@tanstack/react-table";
import { useState } from "react";

import { BarChart, type BarChartDatum } from "@/components/charts/BarChart";
import { LineChart, type LineChartDatum } from "@/components/charts/LineChart";
import { DataTable } from "@/components/data";
import {
  Badge,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
  Textarea,
} from "@/components/ui";
import { FormField } from "@/components/ui/FormField";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

// ─── Sample data ──────────────────────────────────────────────────

interface SampleEquipment {
  id: number;
  code: string;
  designation: string;
  status: string;
}

const sampleTableData: SampleEquipment[] = Array.from({ length: 25 }, (_, i) => ({
  id: i + 1,
  code: `EQ-${String(i + 1).padStart(4, "0")}`,
  designation: `Equipement ${i + 1}`,
  status: i % 3 === 0 ? "En service" : i % 3 === 1 ? "Hors service" : "En attente",
}));

const sampleColumns: ColumnDef<SampleEquipment, unknown>[] = [
  { accessorKey: "id", header: "ID", enableSorting: true },
  { accessorKey: "code", header: "Code", enableSorting: true },
  { accessorKey: "designation", header: "Designation", enableSorting: true },
  { accessorKey: "status", header: "Statut", enableSorting: false },
];

const barChartData: BarChartDatum[] = [
  { label: "Jan", value: 42 },
  { label: "Fev", value: 28 },
  { label: "Mar", value: 55 },
  { label: "Avr", value: 31 },
  { label: "Mai", value: 68 },
  { label: "Jun", value: 47 },
];

const lineChartData: LineChartDatum[] = [
  { x: 1, y: 20 },
  { x: 2, y: 45 },
  { x: 3, y: 32 },
  { x: 4, y: 67 },
  { x: 5, y: 54 },
  { x: 6, y: 78 },
  { x: 7, y: 62 },
  { x: 8, y: 89 },
];

// ─── Section wrapper ──────────────────────────────────────────────

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-4">
      <h2 className="text-xl font-semibold tracking-tight">{title}</h2>
      <Separator />
      {children}
    </section>
  );
}

// ─── Page ─────────────────────────────────────────────────────────

export function ComponentShowcase() {
  const [formError, setFormError] = useState("");

  return (
    <div className="mx-auto max-w-5xl space-y-10 p-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Component Showcase</h1>
        <p className="mt-1 text-sm text-text-muted">
          Dev-only page — UI foundation primitives for Phase 2 module development.
        </p>
      </div>

      {/* ── Buttons ────────────────────────────────────────────── */}
      <Section title="Buttons">
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="default">Default</Button>
          <Button variant="destructive">Destructive</Button>
          <Button variant="outline">Outline</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="link">Link</Button>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Button size="sm">Small</Button>
          <Button size="default">Default</Button>
          <Button size="lg">Large</Button>
          <Button size="icon" aria-label="Icon button">
            <span className="text-lg">+</span>
          </Button>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Button disabled>Disabled</Button>
        </div>
      </Section>

      {/* ── Form Fields ────────────────────────────────────────── */}
      <Section title="Form Fields">
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField name="demo-input" label="Input" description="Standard text input">
            <Input id="demo-input" placeholder="Saisir une valeur..." />
          </FormField>

          <FormField name="demo-textarea" label="Textarea" description="Multi-line input">
            <Textarea id="demo-textarea" placeholder="Description..." rows={3} />
          </FormField>

          <FormField name="demo-select" label="Select" description="Dropdown selection">
            <Select>
              <SelectTrigger id="demo-select">
                <SelectValue placeholder="Choisir..." />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="option1">Option 1</SelectItem>
                <SelectItem value="option2">Option 2</SelectItem>
                <SelectItem value="option3">Option 3</SelectItem>
              </SelectContent>
            </Select>
          </FormField>

          <FormField name="demo-error" label="Field with error" error={formError || ""} required>
            <Input
              id="demo-error"
              placeholder="Type to clear error..."
              onChange={(e) => setFormError(e.target.value ? "" : "Ce champ est requis")}
              onFocus={() => {
                if (!formError) setFormError("Ce champ est requis");
              }}
            />
          </FormField>

          <div className="space-y-1.5">
            <Label htmlFor="demo-bare-input">Bare Label + Input</Label>
            <Input id="demo-bare-input" placeholder="Without FormField wrapper" />
          </div>
        </div>
      </Section>

      {/* ── Dialog & Sheet ─────────────────────────────────────── */}
      <Section title="Dialog & Sheet">
        <div className="flex flex-wrap gap-3">
          <Dialog>
            <DialogTrigger asChild>
              <Button variant="outline">Open Dialog</Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Sample Dialog</DialogTitle>
                <DialogDescription>
                  This is a modal dialog for create/edit operations.
                </DialogDescription>
              </DialogHeader>
              <div className="space-y-3 py-4">
                <FormField name="dialog-name" label="Nom" required>
                  <Input id="dialog-name" placeholder="Nom de l'equipement..." />
                </FormField>
              </div>
              <DialogFooter>
                <Button variant="outline">Annuler</Button>
                <Button>Enregistrer</Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          <Sheet>
            <SheetTrigger asChild>
              <Button variant="outline">Open Sheet</Button>
            </SheetTrigger>
            <SheetContent>
              <SheetHeader>
                <SheetTitle>Detail Panel</SheetTitle>
                <SheetDescription>Side panel for record details or filters.</SheetDescription>
              </SheetHeader>
              <div className="mt-6 space-y-3">
                <FormField name="sheet-field" label="Champ" description="Example field in sheet">
                  <Input id="sheet-field" placeholder="Valeur..." />
                </FormField>
              </div>
            </SheetContent>
          </Sheet>
        </div>
      </Section>

      {/* ── Tabs ───────────────────────────────────────────────── */}
      <Section title="Tabs">
        <Tabs defaultValue="tab1">
          <TabsList>
            <TabsTrigger value="tab1">General</TabsTrigger>
            <TabsTrigger value="tab2">Technique</TabsTrigger>
            <TabsTrigger value="tab3">Historique</TabsTrigger>
          </TabsList>
          <TabsContent value="tab1" className="rounded-md border p-4">
            <p className="text-sm text-text-muted">Contenu de l'onglet General.</p>
          </TabsContent>
          <TabsContent value="tab2" className="rounded-md border p-4">
            <p className="text-sm text-text-muted">Contenu de l'onglet Technique.</p>
          </TabsContent>
          <TabsContent value="tab3" className="rounded-md border p-4">
            <p className="text-sm text-text-muted">Contenu de l'onglet Historique.</p>
          </TabsContent>
        </Tabs>
      </Section>

      {/* ── DataTable ──────────────────────────────────────────── */}
      <Section title="DataTable">
        <DataTable
          columns={sampleColumns}
          data={sampleTableData}
          searchable
          searchPlaceholder="Rechercher un equipement..."
          pageSize={5}
        />
      </Section>

      {/* ── Charts ─────────────────────────────────────────────── */}
      <Section title="Charts">
        <div className="grid gap-6 sm:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">BarChart — OT par mois</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="h-[250px]">
                <BarChart data={barChartData} />
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">LineChart — MTBF trend</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="h-[250px]">
                <LineChart data={lineChartData} showDots />
              </div>
            </CardContent>
          </Card>
        </div>
      </Section>

      {/* ── Badges ─────────────────────────────────────────────── */}
      <Section title="Badges">
        <div className="flex flex-wrap items-center gap-3">
          <Badge variant="default">Default</Badge>
          <Badge variant="secondary">Secondary</Badge>
          <Badge variant="destructive">Destructive</Badge>
          <Badge variant="outline">Outline</Badge>
        </div>
      </Section>

      {/* ── Cards ──────────────────────────────────────────────── */}
      <Section title="Cards">
        <div className="grid gap-4 sm:grid-cols-3">
          <Card>
            <CardHeader>
              <CardTitle>Equipements</CardTitle>
              <CardDescription>Total des actifs enregistres</CardDescription>
            </CardHeader>
            <CardContent>
              <p className="text-3xl font-bold">247</p>
            </CardContent>
            <CardFooter className="text-xs text-text-muted">Mis a jour il y a 5 min</CardFooter>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>OT en cours</CardTitle>
              <CardDescription>Ordres de travail actifs</CardDescription>
            </CardHeader>
            <CardContent>
              <p className="text-3xl font-bold">18</p>
            </CardContent>
            <CardFooter className="text-xs text-text-muted">3 en retard</CardFooter>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>MTBF</CardTitle>
              <CardDescription>Temps moyen entre pannes</CardDescription>
            </CardHeader>
            <CardContent>
              <p className="text-3xl font-bold">142h</p>
            </CardContent>
            <CardFooter className="text-xs text-text-muted">+12% vs mois precedent</CardFooter>
          </Card>
        </div>
      </Section>
    </div>
  );
}
