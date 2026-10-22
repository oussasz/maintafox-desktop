/**
 * One person's qualifications: certifications / authorisations with expiry (record, renew,
 * verify) and skills with a level 1–5 (record, validate, remove). Used from the qualification
 * matrix and from the person's sheet in the personnel register.
 */
import { Award, Plus, ShieldCheck, Trash2, Wrench } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ListAccessNote } from "@/components/ui/list-access-note";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useCanReadList, usePermissionsLoading } from "@/lib/list-access";
import { listPersonnelSkillReferenceValues } from "@/services/personnel-service";
import {
  listCertificationTypes,
  listPersonnelCertifications,
  upsertPersonnelCertification,
} from "@/services/qualification-service";
import {
  type PersonnelSkillRow,
  listPersonnelSkills,
  removePersonnelSkill,
  upsertPersonnelSkill,
} from "@/services/workforce-service";
import { isStepUpRequiredError } from "@/utils/errors";
import type {
  CertificationType,
  PersonnelCertification,
  PersonnelSkillReferenceValue,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { addMonths, CERT_READINESS_TONE, todayIso } from "./qualification-tones";

export const SKILL_LEVELS = [1, 2, 3, 4, 5] as const;

interface CertDraft {
  certification_type_id: string;
  issued_at: string;
  expires_at: string;
  issuing_body: string;
  certificate_ref: string;
  verified: boolean;
}

const EMPTY_CERT: CertDraft = {
  certification_type_id: "",
  issued_at: "",
  expires_at: "",
  issuing_body: "",
  certificate_ref: "",
  verified: true,
};

interface SkillDraft {
  reference_value_id: string;
  level: string;
  valid_to: string;
  validate: boolean;
}

const EMPTY_SKILL: SkillDraft = {
  reference_value_id: "",
  level: "3",
  valid_to: "",
  validate: false,
};

function isCancelled(err: unknown): boolean {
  return err instanceof Error && err.message === "Step-up cancelled by user";
}

export interface PersonQualificationSheetProps {
  personnelId: number;
  /** Pre-selects a certification type in the "record" form (renewal from the expiry list). */
  focusCertificationTypeId?: number | null;
  /** Called after any change, so the caller can refresh its own view (matrix row…). */
  onChanged?: () => void;
}

export function PersonQualificationSheet({
  personnelId,
  focusCertificationTypeId,
  onChanged,
}: PersonQualificationSheetProps) {
  const { t } = useTranslation("training");
  const { can } = usePermissions();
  // Certifications and their types are training lists (trn.view); skills are personnel data.
  // Each part loads on its own, so one refused or failed list does not empty the other.
  const canReadCerts = useCanReadList("certificationTypes");
  const permissionsLoading = usePermissionsLoading();
  // Recording or verifying a certification also needs the certification list (trn.view).
  const canCertify = can(P.TRN_CERTIFY) && canReadCerts;
  // Validating a skill is `upsert_personnel_skill` (per.manage + trn.certify): no trn.view.
  const canManageSkills = can(P.PER_MANAGE);
  const canValidateSkills = canManageSkills && can(P.TRN_CERTIFY);
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [certs, setCerts] = useState<PersonnelCertification[]>([]);
  const [skills, setSkills] = useState<PersonnelSkillRow[]>([]);
  const [certTypes, setCertTypes] = useState<CertificationType[]>([]);
  const [skillCatalogue, setSkillCatalogue] = useState<PersonnelSkillReferenceValue[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [certError, setCertError] = useState<unknown>(null);

  const [certDraft, setCertDraft] = useState<CertDraft | null>(null);
  const [skillDraft, setSkillDraft] = useState<SkillDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<PersonnelSkillRow | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    setCertError(null);
    const [skillPart, certPart] = await Promise.allSettled([
      Promise.all([listPersonnelSkills(personnelId), listPersonnelSkillReferenceValues()]),
      canReadCerts
        ? Promise.all([
            listPersonnelCertifications({ personnel_id: personnelId, limit: 500 }),
            listCertificationTypes(),
          ])
        : Promise.resolve(null),
    ]);
    if (skillPart.status === "fulfilled") {
      setSkills(skillPart.value[0]);
      setSkillCatalogue(skillPart.value[1]);
    } else {
      setLoadError(skillPart.reason);
    }
    if (certPart.status === "fulfilled") {
      setCerts(certPart.value?.[0] ?? []);
      setCertTypes((certPart.value?.[1] ?? []).filter((x) => x.is_active));
    } else {
      setCertError(certPart.reason);
    }
    setLoading(false);
  }, [personnelId, canReadCerts]);

  useEffect(() => {
    // Rights not known yet: nothing is asked before it is known which lists may be read.
    if (!permissionsLoading) void load();
  }, [load, permissionsLoading]);

  useEffect(() => {
    if (focusCertificationTypeId != null && canCertify) {
      setCertDraft({
        ...EMPTY_CERT,
        certification_type_id: String(focusCertificationTypeId),
        issued_at: todayIso(),
      });
    }
  }, [focusCertificationTypeId, canCertify]);

  const typeById = useMemo(() => new Map(certTypes.map((c) => [c.id, c])), [certTypes]);

  /** Latest certificate per type (renewals keep their history below it). */
  const certRows = useMemo(
    () =>
      [...certs].sort(
        (a, b) =>
          (a.certification_type_name ?? "").localeCompare(b.certification_type_name ?? "") ||
          (b.expires_at ?? "9999").localeCompare(a.expires_at ?? "9999"),
      ),
    [certs],
  );

  const setIssued = (issued: string) => {
    setCertDraft((d) => {
      if (!d) return d;
      const type = typeById.get(Number(d.certification_type_id));
      const months = type?.default_validity_months ?? null;
      const expires = issued && months ? (addMonths(issued, months) ?? d.expires_at) : d.expires_at;
      return { ...d, issued_at: issued, expires_at: expires };
    });
  };

  const setCertType = (value: string) => {
    setCertDraft((d) => {
      if (!d) return d;
      const months = typeById.get(Number(value))?.default_validity_months ?? null;
      const expires = d.issued_at && months ? (addMonths(d.issued_at, months) ?? "") : d.expires_at;
      return { ...d, certification_type_id: value, expires_at: expires };
    });
  };

  const certDraftValid =
    certDraft != null &&
    certDraft.certification_type_id !== "" &&
    certDraft.issued_at !== "" &&
    (certDraft.expires_at === "" || certDraft.expires_at >= certDraft.issued_at);

  const saveCert = async () => {
    if (!certDraft || !certDraftValid) return;
    setSaving(true);
    try {
      await withStepUp(() =>
        upsertPersonnelCertification({
          id: null,
          personnel_id: personnelId,
          certification_type_id: Number(certDraft.certification_type_id),
          issued_at: certDraft.issued_at,
          expires_at: certDraft.expires_at || null,
          issuing_body: certDraft.issuing_body.trim() || null,
          certificate_ref: certDraft.certificate_ref.trim() || null,
          verification_status: certDraft.verified ? "verified" : "pending",
          expected_row_version: null,
        }),
      );
      setCertDraft(null);
      await load();
      onChanged?.();
    } catch (err) {
      // Failures are reported by the global toast; a cancelled step-up is not a failure.
      if (!isCancelled(err) && !isStepUpRequiredError(err))
        console.warn("certification save failed", err);
    } finally {
      setSaving(false);
    }
  };

  const verifyCert = async (c: PersonnelCertification) => {
    setSaving(true);
    try {
      await withStepUp(() =>
        upsertPersonnelCertification({
          id: c.id,
          personnel_id: c.personnel_id,
          certification_type_id: c.certification_type_id,
          issued_at: c.issued_at,
          expires_at: c.expires_at,
          issuing_body: c.issuing_body,
          certificate_ref: c.certificate_ref,
          verification_status: "verified",
          expected_row_version: c.row_version,
        }),
      );
      await load();
      onChanged?.();
    } catch (err) {
      if (!isCancelled(err)) console.warn("certification verify failed", err);
    } finally {
      setSaving(false);
    }
  };

  const saveSkill = async () => {
    if (!skillDraft || skillDraft.reference_value_id === "") return;
    setSaving(true);
    try {
      const rows = await upsertPersonnelSkill({
        personnel_id: personnelId,
        reference_value_id: Number(skillDraft.reference_value_id),
        proficiency_level: Number(skillDraft.level),
        valid_to: skillDraft.valid_to || null,
        validate: skillDraft.validate,
      });
      setSkills(rows);
      setSkillDraft(null);
      onChanged?.();
    } catch (err) {
      console.warn("skill save failed", err);
    } finally {
      setSaving(false);
    }
  };

  const validateSkill = async (s: PersonnelSkillRow) => {
    setSaving(true);
    try {
      setSkills(
        await upsertPersonnelSkill({
          personnel_id: personnelId,
          reference_value_id: s.reference_value_id,
          proficiency_level: s.proficiency_level,
          valid_to: s.valid_to,
          is_primary: s.is_primary,
          validate: true,
        }),
      );
      onChanged?.();
    } catch (err) {
      console.warn("skill validation failed", err);
    } finally {
      setSaving(false);
    }
  };

  const confirmRemoveSkill = async () => {
    if (!removeTarget) return;
    setSkills(await removePersonnelSkill(personnelId, removeTarget.reference_value_id));
    setRemoveTarget(null);
    onChanged?.();
  };

  if ((loading || permissionsLoading) && certs.length === 0 && skills.length === 0) {
    return <LoadingState />;
  }

  return (
    <div className="space-y-6">
      {StepUpDialogElement}

      {/* ── Certifications ─────────────────────────────────────────── */}
      <section aria-labelledby={`certs-${personnelId}`} className="space-y-2">
        <div className="flex items-center justify-between gap-2">
          <h3
            id={`certs-${personnelId}`}
            className="flex items-center gap-1.5 text-sm font-semibold text-text-primary"
          >
            <Award className="h-4 w-4" aria-hidden />
            {t("sheet.certifications")}
          </h3>
          {canCertify && certDraft == null ? (
            <Button
              size="sm"
              variant="outline"
              className="gap-1"
              onClick={() => setCertDraft({ ...EMPTY_CERT, issued_at: todayIso() })}
            >
              <Plus className="h-3.5 w-3.5" aria-hidden />
              {t("sheet.recordCertification")}
            </Button>
          ) : null}
        </div>

        {!canReadCerts ? (
          <ListAccessNote list="certificationTypes" />
        ) : certError ? (
          <ErrorState error={certError} onRetry={() => void load()} />
        ) : certRows.length === 0 ? (
          <p className="text-sm text-text-muted">{t("sheet.noCertifications")}</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                <th className="py-1.5 pr-2 font-medium">{t("col.certification")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("col.issued")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("col.expires")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("col.issuingBody")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("col.status")}</th>
                <th className="py-1.5 font-medium">
                  <span className="sr-only">{t("col.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {certRows.map((c) => (
                <tr key={c.id} className="border-b border-surface-border/60">
                  <td className="py-1.5 pr-2">
                    {c.certification_type_name ?? c.certification_type_code}
                  </td>
                  <td className="py-1.5 pr-2 tabular-nums">{c.issued_at?.slice(0, 10) ?? "—"}</td>
                  <td className="py-1.5 pr-2 tabular-nums">
                    {c.expires_at?.slice(0, 10) ?? t("sheet.noExpiry")}
                  </td>
                  <td className="py-1.5 pr-2">{c.issuing_body ?? "—"}</td>
                  <td className="py-1.5 pr-2">
                    <StatusChip
                      label={t(`certStatus.${c.readiness_status}`, {
                        defaultValue: t("certStatus.pending"),
                      })}
                      tone={toneOf(CERT_READINESS_TONE, c.readiness_status)}
                      dot
                    />
                  </td>
                  <td className="py-1.5 text-right">
                    {canCertify && c.verification_status === "pending" ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={saving}
                        onClick={() => void verifyCert(c)}
                      >
                        <ShieldCheck className="mr-1 h-3.5 w-3.5" aria-hidden />
                        {t("sheet.verify")}
                      </Button>
                    ) : null}
                    {canCertify && c.verification_status === "verified" ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() =>
                          setCertDraft({
                            ...EMPTY_CERT,
                            certification_type_id: String(c.certification_type_id),
                            issuing_body: c.issuing_body ?? "",
                            issued_at: todayIso(),
                            expires_at: (() => {
                              const months = typeById.get(
                                c.certification_type_id,
                              )?.default_validity_months;
                              return months ? (addMonths(todayIso(), months) ?? "") : "";
                            })(),
                          })
                        }
                      >
                        {t("sheet.renew")}
                      </Button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        {certDraft ? (
          <div className="grid gap-3 rounded-md border border-surface-border bg-surface-1 p-3 sm:grid-cols-2">
            <FormField
              name={`cert-type-${personnelId}`}
              label={t("col.certification")}
              required
              className="sm:col-span-2"
            >
              <Select value={certDraft.certification_type_id} onValueChange={setCertType}>
                <SelectTrigger id={`cert-type-${personnelId}`}>
                  <SelectValue placeholder={t("sheet.chooseCertification")} />
                </SelectTrigger>
                <SelectContent>
                  {certTypes.map((ct) => (
                    <SelectItem key={ct.id} value={String(ct.id)}>
                      {ct.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
            <FormField name={`cert-issued-${personnelId}`} label={t("col.issued")} required>
              <Input
                id={`cert-issued-${personnelId}`}
                type="date"
                value={certDraft.issued_at}
                onChange={(e) => setIssued(e.target.value)}
              />
            </FormField>
            <FormField
              name={`cert-expires-${personnelId}`}
              label={t("col.expires")}
              description={t("sheet.expiryHint")}
              error={
                certDraft.expires_at !== "" && certDraft.expires_at < certDraft.issued_at
                  ? t("sheet.expiryBeforeIssue")
                  : undefined
              }
            >
              <Input
                id={`cert-expires-${personnelId}`}
                type="date"
                value={certDraft.expires_at}
                onChange={(e) => setCertDraft({ ...certDraft, expires_at: e.target.value })}
              />
            </FormField>
            <FormField name={`cert-body-${personnelId}`} label={t("col.issuingBody")}>
              <Input
                id={`cert-body-${personnelId}`}
                value={certDraft.issuing_body}
                onChange={(e) => setCertDraft({ ...certDraft, issuing_body: e.target.value })}
              />
            </FormField>
            <FormField name={`cert-ref-${personnelId}`} label={t("col.certificateRef")}>
              <Input
                id={`cert-ref-${personnelId}`}
                value={certDraft.certificate_ref}
                onChange={(e) => setCertDraft({ ...certDraft, certificate_ref: e.target.value })}
              />
            </FormField>
            <label className="flex items-center gap-2 text-sm sm:col-span-2">
              <Switch
                checked={certDraft.verified}
                onCheckedChange={(v) => setCertDraft({ ...certDraft, verified: v })}
                aria-label={t("sheet.verifiedNow")}
              />
              {t("sheet.verifiedNow")}
            </label>
            <div className="flex justify-end gap-2 sm:col-span-2">
              <Button variant="ghost" onClick={() => setCertDraft(null)} disabled={saving}>
                {t("action.cancel")}
              </Button>
              <Button onClick={() => void saveCert()} disabled={!certDraftValid || saving}>
                {t("action.save")}
              </Button>
            </div>
          </div>
        ) : null}
      </section>

      {/* ── Skills ─────────────────────────────────────────────────── */}
      <section aria-labelledby={`skills-${personnelId}`} className="space-y-2">
        <div className="flex items-center justify-between gap-2">
          <h3
            id={`skills-${personnelId}`}
            className="flex items-center gap-1.5 text-sm font-semibold text-text-primary"
          >
            <Wrench className="h-4 w-4" aria-hidden />
            {t("sheet.skills")}
          </h3>
          {canManageSkills && skillDraft == null ? (
            <Button
              size="sm"
              variant="outline"
              className="gap-1"
              onClick={() => setSkillDraft({ ...EMPTY_SKILL })}
            >
              <Plus className="h-3.5 w-3.5" aria-hidden />
              {t("sheet.recordSkill")}
            </Button>
          ) : null}
        </div>
        {loadError ? (
          <ErrorState error={loadError} onRetry={() => void load()} />
        ) : skills.length === 0 ? (
          <p className="text-sm text-text-muted">{t("sheet.noSkills")}</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                <th className="py-1.5 pr-2 font-medium">{t("col.skill")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("col.level")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("col.validTo")}</th>
                <th className="py-1.5 pr-2 font-medium">{t("col.status")}</th>
                <th className="py-1.5 font-medium">
                  <span className="sr-only">{t("col.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {skills.map((s) => {
                const unvalidated = !s.is_validated;
                return (
                  <tr key={s.id} className="border-b border-surface-border/60">
                    <td className="py-1.5 pr-2">{s.skill_label}</td>
                    <td className="py-1.5 pr-2">
                      {t(`level.${s.proficiency_level}`, {
                        defaultValue: String(s.proficiency_level),
                      })}
                    </td>
                    <td className="py-1.5 pr-2 tabular-nums">{s.valid_to ?? "—"}</td>
                    <td className="py-1.5 pr-2">
                      {s.is_validated ? (
                        <StatusChip
                          label={t("skillStatus.validated")}
                          tone="success"
                          dot
                          {...(s.verified_by_name
                            ? {
                                title: t("skillStatus.validatedBy", {
                                  name: s.verified_by_name,
                                  date: s.verified_at?.slice(0, 10) ?? "",
                                }),
                              }
                            : {})}
                        />
                      ) : unvalidated ? (
                        <StatusChip label={t("skillStatus.unvalidated")} tone="neutral" dot />
                      ) : (
                        <StatusChip label={t("skillStatus.recorded")} tone="info" dot />
                      )}
                    </td>
                    <td className="py-1.5 text-right">
                      {canValidateSkills && !s.is_validated ? (
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={saving}
                          onClick={() => void validateSkill(s)}
                        >
                          {t("sheet.validate")}
                        </Button>
                      ) : null}
                      {canManageSkills ? (
                        <Button
                          size="sm"
                          variant="ghost"
                          aria-label={t("sheet.removeSkill", { skill: s.skill_label })}
                          onClick={() => setRemoveTarget(s)}
                        >
                          <Trash2 className="h-3.5 w-3.5" aria-hidden />
                        </Button>
                      ) : null}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}

        {skillDraft ? (
          <div className="grid gap-3 rounded-md border border-surface-border bg-surface-1 p-3 sm:grid-cols-3">
            <FormField name={`skill-${personnelId}`} label={t("col.skill")} required>
              <Select
                value={skillDraft.reference_value_id}
                onValueChange={(v) => setSkillDraft({ ...skillDraft, reference_value_id: v })}
              >
                <SelectTrigger id={`skill-${personnelId}`}>
                  <SelectValue placeholder={t("sheet.chooseSkill")} />
                </SelectTrigger>
                <SelectContent>
                  {skillCatalogue.map((sk) => (
                    <SelectItem key={sk.id} value={String(sk.id)}>
                      {sk.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
            <FormField name={`skill-level-${personnelId}`} label={t("col.level")} required>
              <Select
                value={skillDraft.level}
                onValueChange={(v) => setSkillDraft({ ...skillDraft, level: v })}
              >
                <SelectTrigger id={`skill-level-${personnelId}`}>
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
            <FormField name={`skill-valid-${personnelId}`} label={t("col.validTo")}>
              <Input
                id={`skill-valid-${personnelId}`}
                type="date"
                value={skillDraft.valid_to}
                onChange={(e) => setSkillDraft({ ...skillDraft, valid_to: e.target.value })}
              />
            </FormField>
            {canValidateSkills ? (
              <label className="flex items-center gap-2 text-sm sm:col-span-3">
                <Switch
                  checked={skillDraft.validate}
                  onCheckedChange={(v) => setSkillDraft({ ...skillDraft, validate: v })}
                  aria-label={t("sheet.validateNow")}
                />
                {t("sheet.validateNow")}
              </label>
            ) : null}
            <div className="flex justify-end gap-2 sm:col-span-3">
              <Button variant="ghost" onClick={() => setSkillDraft(null)} disabled={saving}>
                {t("action.cancel")}
              </Button>
              <Button
                onClick={() => void saveSkill()}
                disabled={skillDraft.reference_value_id === "" || saving}
              >
                {t("action.save")}
              </Button>
            </div>
          </div>
        ) : null}
      </section>

      <ConfirmDialog
        open={removeTarget != null}
        onOpenChange={(o) => {
          if (!o) setRemoveTarget(null);
        }}
        description={t("sheet.removeSkillConfirm", { skill: removeTarget?.skill_label ?? "" })}
        confirmLabel={t("sheet.removeSkillAction")}
        destructive
        onConfirm={confirmRemoveSkill}
      />
    </div>
  );
}

export interface PersonQualificationDialogProps extends PersonQualificationSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  personName: string;
}

export function PersonQualificationDialog({
  open,
  onOpenChange,
  personName,
  ...sheet
}: PersonQualificationDialogProps) {
  const { t } = useTranslation("training");
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[90vh] w-full max-w-3xl flex-col gap-0 p-0">
        <DialogHeader className="px-6 pt-5 pb-3">
          <DialogTitle>{personName}</DialogTitle>
          <DialogDescription>{t("sheet.description")}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-6">
          {open ? <PersonQualificationSheet {...sheet} /> : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}
