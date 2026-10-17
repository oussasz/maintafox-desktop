import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import {
  Button,
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
  Textarea,
} from "@/components/ui";
import { Checkbox } from "@/components/ui/checkbox";
import { useStepUp } from "@/hooks/use-step-up";
import { describeAppError } from "@/lib/app-error";
import { upsertPermitType, type PermitTypeRecord } from "@/services/permit-service";

interface PermitTypesDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  types: PermitTypeRecord[];
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
    maxHours: ty.max_duration_hours == null ? "" : String(ty.max_duration_hours),
    ppeJson: ty.mandatory_ppe_ids_json,
    rulesJson: ty.mandatory_control_rules_json,
  };
}

export function PermitTypesDialog({ open, onOpenChange, types, onSaved }: PermitTypesDialogProps) {
  const { t } = useTranslation("permits");
  const [draft, setDraft] = useState<TypeDraft>(EMPTY);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { withStepUp, StepUpDialogElement } = useStepUp();

  useEffect(() => {
    if (open) {
      setDraft(EMPTY);
      setSubmitted(false);
      setError(null);
    }
  }, [open]);

  const relaxing = draft.id != null && draft.savedEnergyIsolation && !draft.energyIsolation;
  const maxHoursValue = draft.maxHours.trim() === "" ? null : Number(draft.maxHours);
  const maxHoursInvalid =
    maxHoursValue != null && (!Number.isFinite(maxHoursValue) || maxHoursValue <= 0);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!draft.code.trim() || !draft.name.trim() || maxHoursInvalid) return;
    if (relaxing && !draft.disableReason.trim()) return;
    setBusy(true);
    setError(null);
    try {
      // Switching the isolation rule off asks for the administrator's step-up when needed.
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
          disable_isolation_reason: relaxing ? draft.disableReason.trim() : null,
        });
      await (relaxing ? withStepUp(save) : save());
      setDraft(EMPTY);
      setSubmitted(false);
      onSaved();
    } catch (err) {
      setError(describeAppError(err).title);
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
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("types.title")}</DialogTitle>
        </DialogHeader>

        <div className="max-h-56 overflow-y-auto rounded-md border border-surface-border">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-border text-text-muted">
                <th className="px-2 py-1.5 font-medium">{t("types.code")}</th>
                <th className="px-2 py-1.5 font-medium">{t("types.name")}</th>
                <th className="px-2 py-1.5 font-medium">{t("types.maxHours")}</th>
                <th className="px-2 py-1.5 font-medium">{t("types.energyIsolationShort")}</th>
                <th className="px-2 py-1.5" />
              </tr>
            </thead>
            <tbody>
              {types.map((ty) => (
                <tr key={ty.id} className="border-b border-surface-border/60">
                  <td className="px-2 py-1.5 font-mono text-xs">{ty.code}</td>
                  <td className="px-2 py-1.5">{ty.name}</td>
                  <td className="px-2 py-1.5">{ty.max_duration_hours ?? t("detail.none")}</td>
                  <td className="px-2 py-1.5">
                    {ty.requires_energy_isolation ? t("types.yes") : t("types.no")}
                  </td>
                  <td className="px-2 py-1.5 text-right">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => setDraft(toDraft(ty))}
                    >
                      {t("actions.edit")}
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {types.length === 0 ? (
            <p className="p-2 text-sm text-text-muted">{t("types.empty")}</p>
          ) : null}
        </div>

        <form className="space-y-3" onSubmit={(e) => void handleSubmit(e)}>
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
              error={submitted && !draft.code.trim() ? t("errors.required") : undefined}
            >
              <Input
                id="type-code"
                value={draft.code}
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
            ) : (
              flag("energyIsolation", t("types.energyIsolation"))
            )}
            <p id="type-loto-locked" className="pl-6 text-xs text-text-muted">
              {draft.lotoLocked ? t("types.energyIsolationLocked") : t("types.energyIsolationHint")}
            </p>
            {relaxing ? (
              <FormField
                name="type-disable-reason"
                label={t("types.disableReason")}
                description={t("types.disableReasonHint")}
                required
                error={submitted && !draft.disableReason.trim() ? t("errors.required") : undefined}
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
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("actions.close")}
            </Button>
            <Button type="submit" disabled={busy}>
              {t("types.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
      {StepUpDialogElement}
    </Dialog>
  );
}
