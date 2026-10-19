/**
 * Catalogue of certifications / authorisations the plant tracks (electrical habilitations,
 * LOTO authority, confined space, work at height, welding, lifting equipment, first aid…):
 * family, validity and how early renewal is flagged.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Award, Plus } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { EntityFormDialog, EntityFormFooter, EntityFormSection } from "@/components/entity-form";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { mfLayout } from "@/design-system/tokens";
import { upsertCertificationType } from "@/services/qualification-service";
import type { CertificationType } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export const CERTIFICATION_CATEGORIES = [
  "electrical",
  "loto",
  "confined_space",
  "work_at_height",
  "welding",
  "lifting",
  "first_aid",
  "hot_work",
  "other",
] as const;

interface Draft {
  id: number | null;
  row_version: number | null;
  code: string;
  name: string;
  category: string;
  validity: string;
  lead: string;
  description: string;
  active: boolean;
}

function toDraft(c: CertificationType | null): Draft {
  return {
    id: c?.id ?? null,
    row_version: c?.row_version ?? null,
    code: c?.code ?? "",
    name: c?.name ?? "",
    category: c?.category ?? "other",
    validity: c?.default_validity_months != null ? String(c.default_validity_months) : "",
    lead: c?.renewal_lead_days != null ? String(c.renewal_lead_days) : "",
    description: c?.description ?? "",
    active: c?.is_active ?? true,
  };
}

function positiveOrNull(v: string): number | null | "invalid" {
  if (v.trim() === "") return null;
  const n = Number(v);
  return Number.isInteger(n) && n > 0 ? n : "invalid";
}

export interface CertificationCatalogPanelProps {
  certTypes: CertificationType[];
  loading: boolean;
  onChanged: () => void;
}

export function CertificationCatalogPanel({
  certTypes,
  loading,
  onChanged,
}: CertificationCatalogPanelProps) {
  const { t } = useTranslation("training");
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);

  const validity = draft ? positiveOrNull(draft.validity) : null;
  const lead = draft ? positiveOrNull(draft.lead) : null;
  const valid =
    draft != null &&
    draft.code.trim() !== "" &&
    draft.name.trim() !== "" &&
    validity !== "invalid" &&
    lead !== "invalid";

  const save = async () => {
    if (!draft || !valid) return;
    setSaving(true);
    try {
      await upsertCertificationType({
        id: draft.id,
        expected_row_version: draft.row_version,
        code: draft.code.trim(),
        name: draft.name.trim(),
        category: draft.category,
        default_validity_months: validity,
        renewal_lead_days: lead,
        description: draft.description.trim() || null,
        is_active: draft.active,
      });
      setDraft(null);
      onChanged();
    } catch {
      // Reported by the global toast; the form stays open.
    } finally {
      setSaving(false);
    }
  };

  const columns = useMemo<ColumnDef<CertificationType>[]>(
    () => [
      {
        accessorKey: "name",
        header: t("col.certification"),
        cell: ({ row }) => (
          <button
            type="button"
            className="text-left font-medium text-text-primary hover:underline"
            onClick={() => setDraft(toDraft(row.original))}
          >
            {row.original.name}
            <span className="block text-xs font-normal text-text-muted">{row.original.code}</span>
          </button>
        ),
      },
      {
        accessorKey: "category",
        header: t("col.family"),
        cell: ({ row }) =>
          t(`category.${row.original.category}`, { defaultValue: row.original.category }),
      },
      {
        accessorKey: "default_validity_months",
        header: t("col.validity"),
        cell: ({ row }) =>
          row.original.default_validity_months != null
            ? t("catalog.months", { count: row.original.default_validity_months })
            : t("catalog.noExpiry"),
      },
      {
        accessorKey: "renewal_lead_days",
        header: t("col.renewalNotice"),
        cell: ({ row }) =>
          row.original.renewal_lead_days != null
            ? t("catalog.days", { count: row.original.renewal_lead_days })
            : t("catalog.defaultNotice"),
      },
      {
        accessorKey: "is_active",
        header: t("col.status"),
        cell: ({ row }) => (
          <StatusChip
            label={row.original.is_active ? t("catalog.active") : t("catalog.inactive")}
            tone={row.original.is_active ? "success" : "neutral"}
            dot
          />
        ),
      },
    ],
    [t],
  );

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <p className="text-sm text-text-muted">{t("catalog.intro")}</p>
        <PermissionGate permission={P.TRN_MANAGE}>
          <Button className={mfLayout.primaryAction} onClick={() => setDraft(toDraft(null))}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("catalog.add")}
          </Button>
        </PermissionGate>
      </div>
      {!loading && certTypes.length === 0 ? (
        <EmptyState icon={<Award className="h-8 w-8" aria-hidden />} title={t("catalog.empty")} />
      ) : (
        <DataTable
          columns={columns}
          data={certTypes}
          pageSize={25}
          isLoading={loading && certTypes.length === 0}
        />
      )}

      <EntityFormDialog
        open={draft != null}
        onOpenChange={(o) => !o && !saving && setDraft(null)}
        title={draft?.id ? t("catalog.editTitle") : t("catalog.addTitle")}
        description={t("catalog.formDescription")}
        footer={
          <EntityFormFooter
            cancelLabel={t("action.cancel")}
            primaryLabel={t("action.save")}
            onCancel={() => setDraft(null)}
            cancelDisabled={saving}
            primaryType="button"
            onPrimary={() => void save()}
            primaryDisabled={!valid}
            primaryLoading={saving}
          />
        }
      >
        {draft ? (
          <EntityFormSection title={t("catalog.formSection")}>
            <div className="grid gap-3 sm:grid-cols-2">
              <FormField name="ct-code" label={t("col.code")} required>
                <Input
                  id="ct-code"
                  value={draft.code}
                  onChange={(e) => setDraft({ ...draft, code: e.target.value })}
                />
              </FormField>
              <FormField name="ct-family" label={t("col.family")} required>
                <Select
                  value={draft.category}
                  onValueChange={(v) => setDraft({ ...draft, category: v })}
                >
                  <SelectTrigger id="ct-family">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {CERTIFICATION_CATEGORIES.map((c) => (
                      <SelectItem key={c} value={c}>
                        {t(`category.${c}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
              <FormField name="ct-name" label={t("col.name")} required className="sm:col-span-2">
                <Input
                  id="ct-name"
                  value={draft.name}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
              </FormField>
              <FormField
                name="ct-validity"
                label={t("col.validityMonths")}
                description={t("catalog.validityHint")}
                error={validity === "invalid" ? t("catalog.positiveNumber") : undefined}
              >
                <Input
                  id="ct-validity"
                  inputMode="numeric"
                  value={draft.validity}
                  onChange={(e) => setDraft({ ...draft, validity: e.target.value })}
                />
              </FormField>
              <FormField
                name="ct-lead"
                label={t("col.renewalNoticeDays")}
                description={t("catalog.leadHint")}
                error={lead === "invalid" ? t("catalog.positiveNumber") : undefined}
              >
                <Input
                  id="ct-lead"
                  inputMode="numeric"
                  value={draft.lead}
                  onChange={(e) => setDraft({ ...draft, lead: e.target.value })}
                />
              </FormField>
              <FormField name="ct-desc" label={t("col.description")} className="sm:col-span-2">
                <Textarea
                  id="ct-desc"
                  rows={2}
                  value={draft.description}
                  onChange={(e) => setDraft({ ...draft, description: e.target.value })}
                />
              </FormField>
              <label className="flex items-center gap-2 text-sm sm:col-span-2">
                <Switch
                  checked={draft.active}
                  onCheckedChange={(v) => setDraft({ ...draft, active: v })}
                  aria-label={t("catalog.active")}
                />
                {t("catalog.active")}
              </label>
            </div>
          </EntityFormSection>
        ) : null}
      </EntityFormDialog>
    </div>
  );
}
