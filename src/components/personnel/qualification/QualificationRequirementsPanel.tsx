/**
 * What jobs require of the people doing them: by work-order type, by permit type, by LOTO role
 * (and, read-only here, the ones set on a single work order). Each requirement asks for a
 * certification or a skill at a minimum level, from every person or from at least one of the crew.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { ListChecks, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { EntityFormDialog, EntityFormFooter, EntityFormSection } from "@/components/entity-form";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { type PermitTypeRecord, listPermitTypes } from "@/services/permit-service";
import {
  LOTO_ROLES,
  type QualificationRequirement,
  type RequirementTargetKind,
  listQualificationRequirements,
  upsertQualificationRequirement,
} from "@/services/qualification-gate-service";
import { listWorkOrderTypes } from "@/services/wo-service";
import type {
  CertificationType,
  PersonnelSkillReferenceValue,
  WorkOrderTypeOption,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { SKILL_LEVELS } from "./PersonQualificationSheet";

type EditableKind = Exclude<RequirementTargetKind, "work_order">;
const EDITABLE_KINDS: EditableKind[] = ["work_order_type", "permit_type", "loto_role"];

interface Draft {
  target_kind: EditableKind;
  target_code: string;
  what: "certification" | "skill";
  certification_type_id: string;
  skill_reference_value_id: string;
  level: string;
  coverage: "each" | "any";
  notes: string;
}

const EMPTY: Draft = {
  target_kind: "work_order_type",
  target_code: "",
  what: "certification",
  certification_type_id: "",
  skill_reference_value_id: "",
  level: "3",
  coverage: "each",
  notes: "",
};

export interface QualificationRequirementsPanelProps {
  certTypes: CertificationType[];
  skills: PersonnelSkillReferenceValue[];
}

export function QualificationRequirementsPanel({
  certTypes,
  skills,
}: QualificationRequirementsPanelProps) {
  const { t } = useTranslation("training");
  const { can } = usePermissions();
  /** Retiring a requirement is a safety change: certification right and a fresh step-up. */
  const canRetire = can(P.TRN_CERTIFY);
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [rows, setRows] = useState<QualificationRequirement[]>([]);
  const [woTypes, setWoTypes] = useState<WorkOrderTypeOption[]>([]);
  const [permitTypes, setPermitTypes] = useState<PermitTypeRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [kindFilter, setKindFilter] = useState<RequirementTargetKind | "all">("all");
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [retire, setRetire] = useState<QualificationRequirement | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [req, wt, pt] = await Promise.all([
        listQualificationRequirements({}),
        listWorkOrderTypes(),
        listPermitTypes(),
      ]);
      setRows(req);
      setWoTypes(wt.filter((w) => w.is_active));
      setPermitTypes(pt);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const targetLabel = useCallback(
    (r: QualificationRequirement): string => {
      switch (r.target_kind) {
        case "work_order":
          return [r.work_order_code, r.target_label].filter(Boolean).join(" — ");
        case "loto_role":
          return t(`lotoRole.${r.target_code}`, { defaultValue: r.target_code ?? "" });
        default:
          return r.target_label ?? r.target_code ?? "";
      }
    },
    [t],
  );

  const visible = useMemo(
    () => (kindFilter === "all" ? rows : rows.filter((r) => r.target_kind === kindFilter)),
    [rows, kindFilter],
  );

  const columns = useMemo<ColumnDef<QualificationRequirement>[]>(
    () => [
      {
        id: "target",
        header: t("col.appliesTo"),
        cell: ({ row }) => (
          <div>
            <span className="block text-xs text-text-muted">
              {t(`targetKind.${row.original.target_kind}`)}
            </span>
            <span className="font-medium text-text-primary">{targetLabel(row.original)}</span>
          </div>
        ),
      },
      {
        id: "what",
        header: t("col.required"),
        cell: ({ row }) =>
          row.original.certification_type_id != null
            ? (row.original.certification_name ?? row.original.certification_code)
            : t("requirements.skillAtLevel", {
                skill: row.original.skill_label ?? row.original.skill_code,
                level: t(`level.${row.original.min_proficiency_level ?? 1}`),
              }),
      },
      {
        accessorKey: "coverage",
        header: t("col.coverage"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`coverage.${row.original.coverage}`)}
            tone={row.original.coverage === "each" ? "progress" : "info"}
          />
        ),
      },
      { accessorKey: "notes", header: t("col.notes"), cell: ({ row }) => row.original.notes ?? "" },
      {
        id: "actions",
        header: () => <span className="sr-only">{t("col.actions")}</span>,
        cell: ({ row }) =>
          canRetire ? (
            <Button size="sm" variant="ghost" onClick={() => setRetire(row.original)}>
              {t("requirements.retire")}
            </Button>
          ) : null,
      },
    ],
    [t, targetLabel, canRetire],
  );

  const targetOptions = (kind: EditableKind): { value: string; label: string }[] => {
    if (kind === "work_order_type") return woTypes.map((w) => ({ value: w.code, label: w.label }));
    if (kind === "permit_type") return permitTypes.map((p) => ({ value: p.code, label: p.name }));
    return LOTO_ROLES.map((r) => ({ value: r, label: t(`lotoRole.${r}`) }));
  };

  const draftValid =
    draft != null &&
    draft.target_code !== "" &&
    (draft.what === "certification"
      ? draft.certification_type_id !== ""
      : draft.skill_reference_value_id !== "");

  const save = async () => {
    if (!draft || !draftValid) return;
    setSaving(true);
    try {
      await upsertQualificationRequirement({
        target_kind: draft.target_kind,
        target_code: draft.target_code,
        certification_type_id:
          draft.what === "certification" ? Number(draft.certification_type_id) : null,
        skill_reference_value_id:
          draft.what === "skill" ? Number(draft.skill_reference_value_id) : null,
        min_proficiency_level: draft.what === "skill" ? Number(draft.level) : null,
        coverage: draft.target_kind === "loto_role" ? "each" : draft.coverage,
        notes: draft.notes.trim() || null,
      });
      setDraft(null);
      await load();
    } catch {
      // Reported by the global toast; the form stays open.
    } finally {
      setSaving(false);
    }
  };

  const confirmRetire = async () => {
    if (!retire) return;
    await withStepUp(() =>
      upsertQualificationRequirement({
        id: retire.id,
        expected_row_version: retire.row_version,
        target_kind: retire.target_kind,
        work_order_id: retire.work_order_id,
        target_code: retire.target_code,
        certification_type_id: retire.certification_type_id,
        skill_reference_value_id: retire.skill_reference_value_id,
        min_proficiency_level: retire.min_proficiency_level,
        coverage: retire.coverage,
        notes: retire.notes,
        is_active: false,
      }),
    );
    setRetire(null);
    await load();
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <label htmlFor="req-kind-filter" className="text-sm text-text-muted">
            {t("col.appliesTo")}
          </label>
          <Select
            value={kindFilter}
            onValueChange={(v) => setKindFilter(v as RequirementTargetKind | "all")}
          >
            <SelectTrigger id="req-kind-filter" className="w-56">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("requirements.allTargets")}</SelectItem>
              {(["work_order_type", "permit_type", "loto_role", "work_order"] as const).map((k) => (
                <SelectItem key={k} value={k}>
                  {t(`targetKind.${k}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <PermissionGate permission={P.TRN_MANAGE}>
          <Button className={mfLayout.primaryAction} onClick={() => setDraft({ ...EMPTY })}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("requirements.add")}
          </Button>
        </PermissionGate>
      </div>
      <p className="text-sm text-text-muted">{t("requirements.intro")}</p>

      {error && rows.length === 0 ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : !loading && visible.length === 0 ? (
        <EmptyState
          icon={<ListChecks className="h-8 w-8" aria-hidden />}
          title={t("requirements.empty")}
          description={t("requirements.emptyHint")}
        />
      ) : (
        <DataTable
          columns={columns}
          data={visible}
          pageSize={25}
          isLoading={loading && rows.length === 0}
        />
      )}

      <EntityFormDialog
        open={draft != null}
        onOpenChange={(o) => !o && !saving && setDraft(null)}
        title={t("requirements.addTitle")}
        description={t("requirements.formDescription")}
        footer={
          <EntityFormFooter
            cancelLabel={t("action.cancel")}
            primaryLabel={t("action.save")}
            onCancel={() => setDraft(null)}
            cancelDisabled={saving}
            primaryType="button"
            onPrimary={() => void save()}
            primaryDisabled={!draftValid}
            primaryLoading={saving}
          />
        }
      >
        {draft ? (
          <EntityFormSection title={t("requirements.formSection")}>
            <div className="grid gap-3 sm:grid-cols-2">
              <FormField name="req-kind" label={t("col.appliesTo")} required>
                <Select
                  value={draft.target_kind}
                  onValueChange={(v) =>
                    setDraft({ ...draft, target_kind: v as EditableKind, target_code: "" })
                  }
                >
                  <SelectTrigger id="req-kind">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {EDITABLE_KINDS.map((k) => (
                      <SelectItem key={k} value={k}>
                        {t(`targetKind.${k}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
              <FormField name="req-target" label={t(`targetKind.${draft.target_kind}`)} required>
                <Select
                  value={draft.target_code}
                  onValueChange={(v) => setDraft({ ...draft, target_code: v })}
                >
                  <SelectTrigger id="req-target">
                    <SelectValue placeholder={t("requirements.chooseTarget")} />
                  </SelectTrigger>
                  <SelectContent>
                    {targetOptions(draft.target_kind).map((o) => (
                      <SelectItem key={o.value} value={o.value}>
                        {o.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
              <FormField name="req-what" label={t("requirements.what")} required>
                <Select
                  value={draft.what}
                  onValueChange={(v) => setDraft({ ...draft, what: v as Draft["what"] })}
                >
                  <SelectTrigger id="req-what">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="certification">
                      {t("requirements.whatCertification")}
                    </SelectItem>
                    <SelectItem value="skill">{t("requirements.whatSkill")}</SelectItem>
                  </SelectContent>
                </Select>
              </FormField>
              {draft.what === "certification" ? (
                <FormField name="req-cert" label={t("col.certification")} required>
                  <Select
                    value={draft.certification_type_id}
                    onValueChange={(v) => setDraft({ ...draft, certification_type_id: v })}
                  >
                    <SelectTrigger id="req-cert">
                      <SelectValue placeholder={t("sheet.chooseCertification")} />
                    </SelectTrigger>
                    <SelectContent>
                      {certTypes
                        .filter((c) => c.is_active)
                        .map((c) => (
                          <SelectItem key={c.id} value={String(c.id)}>
                            {c.name}
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                </FormField>
              ) : (
                <>
                  <FormField name="req-skill" label={t("col.skill")} required>
                    <Select
                      value={draft.skill_reference_value_id}
                      onValueChange={(v) => setDraft({ ...draft, skill_reference_value_id: v })}
                    >
                      <SelectTrigger id="req-skill">
                        <SelectValue placeholder={t("sheet.chooseSkill")} />
                      </SelectTrigger>
                      <SelectContent>
                        {skills.map((s) => (
                          <SelectItem key={s.id} value={String(s.id)}>
                            {s.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </FormField>
                  <FormField name="req-level" label={t("col.minLevel")} required>
                    <Select
                      value={draft.level}
                      onValueChange={(v) => setDraft({ ...draft, level: v })}
                    >
                      <SelectTrigger id="req-level">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {SKILL_LEVELS.map((l) => (
                          <SelectItem key={l} value={String(l)}>
                            {t(`level.${l}`)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </FormField>
                </>
              )}
              {draft.target_kind !== "loto_role" ? (
                <FormField
                  name="req-coverage"
                  label={t("col.coverage")}
                  required
                  className="sm:col-span-2"
                >
                  <Select
                    value={draft.coverage}
                    onValueChange={(v) => setDraft({ ...draft, coverage: v as Draft["coverage"] })}
                  >
                    <SelectTrigger id="req-coverage">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="each">{t("coverage.eachLong")}</SelectItem>
                      <SelectItem value="any">{t("coverage.anyLong")}</SelectItem>
                    </SelectContent>
                  </Select>
                </FormField>
              ) : null}
              <FormField name="req-notes" label={t("col.notes")} className="sm:col-span-2">
                <Textarea
                  id="req-notes"
                  rows={2}
                  value={draft.notes}
                  onChange={(e) => setDraft({ ...draft, notes: e.target.value })}
                />
              </FormField>
            </div>
          </EntityFormSection>
        ) : null}
      </EntityFormDialog>

      <ConfirmDialog
        open={retire != null}
        onOpenChange={(o) => !o && setRetire(null)}
        description={t("requirements.retireConfirm", { target: retire ? targetLabel(retire) : "" })}
        confirmLabel={t("requirements.retire")}
        destructive
        onConfirm={confirmRetire}
      />
      {StepUpDialogElement}
    </div>
  );
}
