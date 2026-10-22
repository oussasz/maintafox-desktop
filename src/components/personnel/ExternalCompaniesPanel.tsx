/**
 * Contractor companies: contract period, onboarding and insurance status, and the people each
 * company provides (contractor technicians are personnel records without a login).
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Building2, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import type { Tone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import {
  createExternalCompany,
  listExternalCompanies,
  listPersonnel,
} from "@/services/personnel-service";
import {
  INSURANCE_STATUSES,
  ONBOARDING_STATUSES,
  updateExternalCompany,
} from "@/services/workforce-service";
import type { ExternalCompany } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export const ONBOARDING_TONE: Record<string, Tone> = {
  pending: "info",
  active: "success",
  suspended: "danger",
  expired: "warning",
};
export const INSURANCE_TONE: Record<string, Tone> = {
  unknown: "attention",
  valid: "success",
  expired: "danger",
  not_required: "neutral",
};

interface Draft {
  id: number | null;
  name: string;
  service_domain: string;
  contract_start: string;
  contract_end: string;
  onboarding_status: string;
  insurance_status: string;
  notes: string;
  active: boolean;
}

function toDraft(c: ExternalCompany | null): Draft {
  return {
    id: c?.id ?? null,
    name: c?.name ?? "",
    service_domain: c?.service_domain ?? "",
    contract_start: c?.contract_start?.slice(0, 10) ?? "",
    contract_end: c?.contract_end?.slice(0, 10) ?? "",
    onboarding_status: c?.onboarding_status ?? "pending",
    insurance_status: c?.insurance_status ?? "unknown",
    notes: c?.notes ?? "",
    active: c ? c.is_active === 1 : true,
  };
}

export interface ExternalCompanyDialogProps {
  open: boolean;
  company: ExternalCompany | null;
  onOpenChange: (open: boolean) => void;
  onSaved: (company: ExternalCompany) => void;
}

/** Create or edit a contractor company (also used from the personnel creation form). */
export function ExternalCompanyDialog({
  open,
  company,
  onOpenChange,
  onSaved,
}: ExternalCompanyDialogProps) {
  const { t } = useTranslation("personnel");
  const [draft, setDraft] = useState<Draft>(toDraft(company));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) setDraft(toDraft(company));
  }, [open, company]);

  const datesValid =
    !draft.contract_start || !draft.contract_end || draft.contract_end >= draft.contract_start;
  const valid = draft.name.trim() !== "" && datesValid;

  const save = async () => {
    if (!valid) return;
    setSaving(true);
    try {
      let saved: ExternalCompany;
      if (draft.id == null) {
        saved = await createExternalCompany(
          draft.name.trim(),
          draft.service_domain.trim() || undefined,
          draft.contract_start || undefined,
          draft.contract_end || undefined,
          draft.notes.trim() || undefined,
        );
      } else {
        saved = (await updateExternalCompany({
          id: draft.id,
          name: draft.name,
          service_domain: draft.service_domain,
          contract_start: draft.contract_start || null,
          contract_end: draft.contract_end || null,
          onboarding_status: draft.onboarding_status,
          insurance_status: draft.insurance_status,
          notes: draft.notes,
          is_active: draft.active,
        })) as ExternalCompany;
      }
      onSaved(saved);
      onOpenChange(false);
    } catch {
      // Reported by the global toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !saving && onOpenChange(o)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{draft.id ? t("companies.editTitle") : t("companies.addTitle")}</DialogTitle>
          <DialogDescription>{t("companies.formDescription")}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 sm:grid-cols-2">
          <FormField name="co-name" label={t("companies.name")} required className="sm:col-span-2">
            <Input
              id="co-name"
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            />
          </FormField>
          <FormField name="co-domain" label={t("companies.domain")} className="sm:col-span-2">
            <Input
              id="co-domain"
              value={draft.service_domain}
              onChange={(e) => setDraft({ ...draft, service_domain: e.target.value })}
            />
          </FormField>
          <FormField name="co-start" label={t("companies.contractStart")}>
            <DateField
              id="co-start"
              value={draft.contract_start}
              onChange={(v) => setDraft({ ...draft, contract_start: v })}
            />
          </FormField>
          <FormField
            name="co-end"
            label={t("companies.contractEnd")}
            error={datesValid ? undefined : t("companies.endBeforeStart")}
          >
            <DateField
              id="co-end"
              value={draft.contract_end}
              onChange={(v) => setDraft({ ...draft, contract_end: v })}
            />
          </FormField>
          {draft.id != null ? (
            <>
              <FormField name="co-onboarding" label={t("companies.onboarding")}>
                <Select
                  value={draft.onboarding_status}
                  onValueChange={(v) => setDraft({ ...draft, onboarding_status: v })}
                >
                  <SelectTrigger id="co-onboarding">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {ONBOARDING_STATUSES.map((s) => (
                      <SelectItem key={s} value={s}>
                        {t(`companies.onboardingStatus.${s}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
              <FormField name="co-insurance" label={t("companies.insurance")}>
                <Select
                  value={draft.insurance_status}
                  onValueChange={(v) => setDraft({ ...draft, insurance_status: v })}
                >
                  <SelectTrigger id="co-insurance">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {INSURANCE_STATUSES.map((s) => (
                      <SelectItem key={s} value={s}>
                        {t(`companies.insuranceStatus.${s}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
            </>
          ) : null}
          <FormField name="co-notes" label={t("companies.notes")} className="sm:col-span-2">
            <Textarea
              id="co-notes"
              rows={2}
              value={draft.notes}
              onChange={(e) => setDraft({ ...draft, notes: e.target.value })}
            />
          </FormField>
          {draft.id != null ? (
            <label className="flex items-center gap-2 text-sm sm:col-span-2">
              <Switch
                checked={draft.active}
                onCheckedChange={(v) => setDraft({ ...draft, active: v })}
                aria-label={t("companies.active")}
              />
              {t("companies.active")}
            </label>
          ) : null}
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {t("action.cancel")}
          </Button>
          <Button className="h-11" onClick={() => void save()} disabled={!valid || saving}>
            {t("action.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ExternalCompaniesPanel() {
  const { t } = useTranslation("personnel");
  const { can } = usePermissions();
  const [rows, setRows] = useState<ExternalCompany[]>([]);
  const [peopleByCompany, setPeopleByCompany] = useState<Map<number, number>>(new Map());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [editing, setEditing] = useState<ExternalCompany | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [companies, contractors] = await Promise.all([
        listExternalCompanies(),
        listPersonnel({
          employment_origin: ["external"],
          employment_status: ["active"],
          limit: 2000,
          offset: 0,
        }),
      ]);
      const counts = new Map<number, number>();
      for (const p of contractors.items) {
        if (p.external_company_id != null)
          counts.set(p.external_company_id, (counts.get(p.external_company_id) ?? 0) + 1);
      }
      setRows(companies);
      setPeopleByCompany(counts);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const columns = useMemo<ColumnDef<ExternalCompany>[]>(
    () => [
      {
        accessorKey: "name",
        header: t("companies.name"),
        cell: ({ row }) =>
          can(P.PER_MANAGE) ? (
            <button
              type="button"
              className="text-left font-medium text-text-primary hover:underline"
              onClick={() => {
                setEditing(row.original);
                setDialogOpen(true);
              }}
            >
              {row.original.name}
            </button>
          ) : (
            <span className="font-medium">{row.original.name}</span>
          ),
      },
      {
        accessorKey: "service_domain",
        header: t("companies.domain"),
        cell: ({ row }) => row.original.service_domain ?? "—",
      },
      {
        id: "contract",
        header: t("companies.contract"),
        cell: ({ row }) =>
          [row.original.contract_start?.slice(0, 10), row.original.contract_end?.slice(0, 10)]
            .map((d) => d ?? "…")
            .join(" → "),
      },
      {
        accessorKey: "onboarding_status",
        header: t("companies.onboarding"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`companies.onboardingStatus.${row.original.onboarding_status}`)}
            tone={ONBOARDING_TONE[row.original.onboarding_status] ?? "neutral"}
            dot
          />
        ),
      },
      {
        accessorKey: "insurance_status",
        header: t("companies.insurance"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`companies.insuranceStatus.${row.original.insurance_status}`)}
            tone={INSURANCE_TONE[row.original.insurance_status] ?? "neutral"}
            dot
          />
        ),
      },
      {
        id: "people",
        header: t("companies.people"),
        cell: ({ row }) => (
          <span className="tabular-nums">{peopleByCompany.get(row.original.id) ?? 0}</span>
        ),
      },
    ],
    [t, can, peopleByCompany],
  );

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <p className="text-sm text-text-muted">{t("companies.intro")}</p>
        <PermissionGate permission={P.PER_MANAGE}>
          <Button
            className={mfLayout.primaryAction}
            onClick={() => {
              setEditing(null);
              setDialogOpen(true);
            }}
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("companies.add")}
          </Button>
        </PermissionGate>
      </div>
      {error && rows.length === 0 ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : !loading && rows.length === 0 ? (
        <EmptyState
          icon={<Building2 className="h-8 w-8" aria-hidden />}
          title={t("companies.empty")}
          description={t("companies.emptyHint")}
        />
      ) : (
        <DataTable
          columns={columns}
          data={rows}
          pageSize={20}
          isLoading={loading && rows.length === 0}
        />
      )}
      <ExternalCompanyDialog
        open={dialogOpen}
        company={editing}
        onOpenChange={setDialogOpen}
        onSaved={() => void load()}
      />
    </div>
  );
}
