/**
 * Work-permit types editor — hosted in Données de référence (catalog `PERMIT.TYPES`), the one
 * place they are managed. Saves go through `upsert_permit_type` (permit issuer; administrator +
 * step-up + a reason of at least ISOLATION_REASON_MIN characters to stop requiring an energy
 * isolation; the LOTO type keeps it: without `adm.settings` the box of a type that requires it is
 * shown but disabled). The
 * three-person lock-removal setting is shown to everyone and changed only by an administrator
 * (`adm.settings`), with a reason of at least THREE_PERSON_REASON_MIN characters and a step-up.
 */
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, FormField, Input, Textarea } from "@/components/ui";
import { Checkbox } from "@/components/ui/checkbox";
import { Switch } from "@/components/ui/switch";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { appErrorReason } from "@/lib/app-error";
import { type PermitTypeRecord, upsertPermitType } from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

import { serverTextLength, serverTrim } from "./permit-rules";
import { usePermitTypeName } from "./permit-type-name";

/**
 * Shortest reason the server accepts for a three-person change: `MIN_REASON_CHARS` in
 * `src-tauri/src/settings/catalog.rs` (counted trimmed, in code points; a test pins both values).
 */
export const THREE_PERSON_REASON_MIN = 10;

/**
 * Shortest reason the server accepts to stop requiring an energy isolation: also
 * `MIN_REASON_CHARS` (counted trimmed, in code points; a test pins both values).
 */
export const ISOLATION_REASON_MIN = 10;

/** The step-up dialog was closed without confirming: nothing to report. */
function isStepUpCancelled(err: unknown): boolean {
  return err instanceof Error && err.message === "Step-up cancelled by user";
}

interface PermitTypesEditorProps {
  types: PermitTypeRecord[];
  /** Permit issuer right: without it the list is read-only. */
  canEdit: boolean;
  onSaved: () => void;
}

interface TypeDraft {
  id: number | null;
  code: string;
  name: string;
  description: string;
  hse: boolean;
  operations: boolean;
  atmospheric: boolean;
  /** Needs a verified energy isolation before it starts (LOTO rule). */
  energyIsolation: boolean;
  /** The saved value: switching it off needs a reason (administrator + step-up on the server). */
  savedEnergyIsolation: boolean;
  /** Shipped LOTO type: the rule cannot be switched off. */
  lotoLocked: boolean;
  disableReason: string;
  /** Third person confirms lock removal (the isolation's verifier may not). */
  threePerson: boolean;
  /** The saved value (false for a new type): any change needs a reason and an administrator. */
  savedThreePerson: boolean;
  threePersonReason: string;
  maxHours: string;
  ppeJson: string | null;
  rulesJson: string | null;
}

const EMPTY: TypeDraft = {
  id: null,
  code: "",
  name: "",
  description: "",
  hse: false,
  operations: false,
  atmospheric: false,
  energyIsolation: false,
  savedEnergyIsolation: false,
  lotoLocked: false,
  disableReason: "",
  threePerson: false,
  savedThreePerson: false,
  threePersonReason: "",
  maxHours: "",
  ppeJson: null,
  rulesJson: null,
};

function toDraft(ty: PermitTypeRecord): TypeDraft {
  return {
    id: ty.id,
    code: ty.code,
    name: ty.name,
    description: ty.description,
    hse: ty.requires_hse_approval,
    operations: ty.requires_operations_approval,
    atmospheric: ty.requires_atmospheric_test,
    energyIsolation: ty.requires_energy_isolation,
    savedEnergyIsolation: ty.requires_energy_isolation,
    lotoLocked: ty.code.trim().toLowerCase() === "loto",
    disableReason: "",
    threePerson: ty.requires_three_person_removal,
    savedThreePerson: ty.requires_three_person_removal,
    threePersonReason: "",
    maxHours: ty.max_duration_hours == null ? "" : String(ty.max_duration_hours),
    ppeJson: ty.mandatory_ppe_ids_json,
    rulesJson: ty.mandatory_control_rules_json,
  };
}

export function PermitTypesEditor({ types, canEdit, onSaved }: PermitTypesEditorProps) {
  const { t } = useTranslation("permits");
  const typeName = usePermitTypeName();
  const [draft, setDraft] = useState<TypeDraft>(EMPTY);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const { can } = usePermissions();
  const canChangeThreePerson = can(P.ADM_SETTINGS);
  // Stopping to require an energy isolation needs the settings right (`upsert_permit_type`);
  // requiring it is a plain edit, so the box is disabled only on a saved type that requires it.
  const isolationAdminOnly = !can(P.ADM_SETTINGS) && draft.id != null && draft.savedEnergyIsolation;

  const changingThreePerson = draft.threePerson !== draft.savedThreePerson;
  const threePersonReason = serverTrim(draft.threePersonReason);
  const threePersonReasonShort =
    serverTextLength(draft.threePersonReason) < THREE_PERSON_REASON_MIN;

  const relaxing = draft.id != null && draft.savedEnergyIsolation && !draft.energyIsolation;
  const disableReason = serverTrim(draft.disableReason);
  const disableReasonShort = serverTextLength(draft.disableReason) < ISOLATION_REASON_MIN;
  const maxHoursValue = draft.maxHours.trim() === "" ? null : Number(draft.maxHours);
  const maxHoursInvalid =
    maxHoursValue != null && (!Number.isFinite(maxHoursValue) || maxHoursValue <= 0);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!draft.code.trim() || !draft.name.trim() || maxHoursInvalid) return;
    if (relaxing && disableReasonShort) return;
    if (changingThreePerson && threePersonReasonShort) return;
    setBusy(true);
    setError(null);
    try {
      // Switching the isolation rule off or changing the three-person setting asks for the
      // administrator's step-up when needed.
      const save = () =>
        upsertPermitType({
          id: draft.id,
          code: draft.code.trim(),
          name: draft.name.trim(),
          description: draft.description,
          requires_hse_approval: draft.hse,
          requires_operations_approval: draft.operations,
          requires_atmospheric_test: draft.atmospheric,
          max_duration_hours: maxHoursValue,
          mandatory_ppe_ids_json: draft.ppeJson,
          mandatory_control_rules_json: draft.rulesJson,
          requires_energy_isolation: draft.energyIsolation,
          // Sent only when the save switches the isolation rule off (trimmed like the server).
          disable_isolation_reason: relaxing ? disableReason : null,
          // Sent only when the user changed it: null keeps the server's current value, so a
          // value changed elsewhere since the form loaded is not rewritten by an unrelated edit.
          requires_three_person_removal: changingThreePerson ? draft.threePerson : null,
          three_person_change_reason: changingThreePerson ? threePersonReason : null,
        });
      await (relaxing || changingThreePerson ? withStepUp(save) : save());
      setDraft(EMPTY);
      setSubmitted(false);
      onSaved();
    } catch (err) {
      if (!isStepUpCancelled(err)) setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  const flag = (key: "hse" | "operations" | "atmospheric" | "energyIsolation", label: string) => (
    <label className="flex items-center gap-2 text-sm text-text-primary">
      <Checkbox
        checked={draft[key]}
        onCheckedChange={(v) => setDraft((d) => ({ ...d, [key]: v === true }))}
      />
      {label}
    </label>
  );

  return (
    <div className="space-y-4">
      <div className="overflow-x-auto rounded-md border border-surface-border bg-surface-0">
        <table className="w-full text-left text-sm">
          <thead>
            <tr className="border-b border-surface-border text-text-muted">
              <th className="px-2 py-1.5 font-medium">{t("types.code")}</th>
              <th className="px-2 py-1.5 font-medium">{t("types.name")}</th>
              <th className="px-2 py-1.5 font-medium">{t("types.maxHours")}</th>
              <th className="px-2 py-1.5 font-medium">{t("types.energyIsolationShort")}</th>
              <th className="px-2 py-1.5 font-medium" title={t("types.threePerson")}>
                {t("types.threePersonShort")}
              </th>
              <th className="px-2 py-1.5" />
            </tr>
          </thead>
          <tbody>
            {types.map((ty) => (
              <tr key={ty.id} className="border-b border-surface-border/60">
                <td className="px-2 py-1.5 font-mono text-xs">{ty.code}</td>
                <td className="px-2 py-1.5">{typeName(ty.code, ty.name)}</td>
                <td className="px-2 py-1.5">{ty.max_duration_hours ?? t("detail.none")}</td>
                <td className="px-2 py-1.5">
                  {ty.requires_energy_isolation ? t("types.yes") : t("types.no")}
                </td>
                <td className="px-2 py-1.5">
                  {ty.requires_three_person_removal ? t("types.yes") : t("types.no")}
                </td>
                <td className="px-2 py-1.5 text-right">
                  {canEdit ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        setDraft(toDraft(ty));
                        setSubmitted(false);
                        setError(null);
                      }}
                    >
                      {t("actions.edit")}
                    </Button>
                  ) : null}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {types.length === 0 ? (
          <p className="p-2 text-sm text-text-muted">{t("types.empty")}</p>
        ) : null}
      </div>

      {canEdit ? (
        <form
          className="space-y-3 rounded-md border border-surface-border bg-surface-0 p-4"
          onSubmit={(e) => void handleSubmit(e)}
        >
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-medium text-text-primary">
              {draft.id == null ? t("types.new") : `${t("actions.edit")} — ${draft.code}`}
            </h3>
            {draft.id != null ? (
              <Button type="button" variant="ghost" size="sm" onClick={() => setDraft(EMPTY)}>
                {t("types.new")}
              </Button>
            ) : null}
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <FormField
              name="type-code"
              label={t("types.code")}
              required
              description={draft.id != null ? t("types.codeLocked") : undefined}
              error={submitted && !draft.code.trim() ? t("errors.required") : undefined}
            >
              <Input
                id="type-code"
                value={draft.code}
                readOnly={draft.id != null}
                aria-describedby={draft.id != null ? "type-code-description" : undefined}
                onChange={(e) => setDraft((d) => ({ ...d, code: e.target.value }))}
              />
            </FormField>
            <FormField
              name="type-name"
              label={t("types.name")}
              required
              error={submitted && !draft.name.trim() ? t("errors.required") : undefined}
            >
              <Input
                id="type-name"
                value={draft.name}
                onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
              />
            </FormField>
          </div>
          <FormField name="type-description" label={t("types.description")}>
            <Textarea
              id="type-description"
              rows={2}
              value={draft.description}
              onChange={(e) => setDraft((d) => ({ ...d, description: e.target.value }))}
            />
          </FormField>
          <FormField
            name="type-max-hours"
            label={t("types.maxHours")}
            error={submitted && maxHoursInvalid ? t("errors.invalidNumber") : undefined}
          >
            <Input
              id="type-max-hours"
              inputMode="decimal"
              value={draft.maxHours}
              onChange={(e) => setDraft((d) => ({ ...d, maxHours: e.target.value }))}
            />
          </FormField>
          <div className="space-y-1.5">
            {flag("hse", t("types.hse"))}
            {flag("operations", t("types.operations"))}
            {flag("atmospheric", t("types.atmospheric"))}
            {draft.lotoLocked ? (
              <label className="flex items-center gap-2 text-sm text-text-primary">
                <Checkbox checked disabled aria-describedby="type-loto-locked" />
                {t("types.energyIsolation")}
              </label>
            ) : isolationAdminOnly ? (
              <label className="flex items-center gap-2 text-sm text-text-primary">
                <Checkbox
                  checked={draft.energyIsolation}
                  disabled
                  aria-describedby="type-loto-locked"
                />
                {t("types.energyIsolation")}
              </label>
            ) : (
              flag("energyIsolation", t("types.energyIsolation"))
            )}
            <p id="type-loto-locked" className="pl-6 text-xs text-text-muted">
              {draft.lotoLocked ? t("types.energyIsolationLocked") : t("types.energyIsolationHint")}
              {!draft.lotoLocked && isolationAdminOnly
                ? ` ${t("types.energyIsolationAdminOnly")}`
                : null}
            </p>
            {relaxing ? (
              <FormField
                name="type-disable-reason"
                label={t("types.disableReason")}
                description={t("types.disableReasonHint", { min: ISOLATION_REASON_MIN })}
                required
                error={
                  (submitted || disableReason.length > 0) && disableReasonShort
                    ? t("types.reasonMin", { min: ISOLATION_REASON_MIN })
                    : undefined
                }
              >
                <Textarea
                  id="type-disable-reason"
                  rows={2}
                  value={draft.disableReason}
                  onChange={(e) => setDraft((d) => ({ ...d, disableReason: e.target.value }))}
                />
              </FormField>
            ) : null}
          </div>
          <div className="space-y-1.5">
            <label className="flex items-center gap-2 text-sm text-text-primary">
              <Switch
                aria-label={t("types.threePerson")}
                aria-describedby="type-three-person-hint"
                checked={draft.threePerson}
                disabled={!canChangeThreePerson}
                onCheckedChange={(v) => setDraft((d) => ({ ...d, threePerson: v }))}
              />
              {t("types.threePerson")}
            </label>
            <p id="type-three-person-hint" className="pl-6 text-xs text-text-muted">
              {t("types.threePersonHint")}
              {canChangeThreePerson ? null : ` ${t("types.threePersonAdminOnly")}`}
            </p>
            {changingThreePerson ? (
              <FormField
                name="type-three-person-reason"
                label={t("types.threePersonReason")}
                description={t("types.threePersonReasonHint", { min: THREE_PERSON_REASON_MIN })}
                required
                error={
                  (submitted || threePersonReason.length > 0) && threePersonReasonShort
                    ? t("types.reasonMin", { min: THREE_PERSON_REASON_MIN })
                    : undefined
                }
              >
                <Textarea
                  id="type-three-person-reason"
                  rows={2}
                  value={draft.threePersonReason}
                  onChange={(e) => setDraft((d) => ({ ...d, threePersonReason: e.target.value }))}
                />
              </FormField>
            ) : null}
          </div>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
          <div className="flex justify-end">
            <Button type="submit" disabled={busy}>
              {t("types.save")}
            </Button>
          </div>
        </form>
      ) : (
        <p className="text-xs text-text-muted">{t("types.readOnly")}</p>
      )}
      {StepUpDialogElement}
    </div>
  );
}
