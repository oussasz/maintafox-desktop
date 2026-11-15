/**
 * Isolation points tab (L4-12): where the energies of the equipment are isolated — code, label,
 * energy type and isolation method (Données de référence, the permit lists), normal state. Kept
 * once here and proposed on every work permit of the equipment. Managed with `eq.manage`;
 * deactivated, never deleted.
 */

import { Plus, ShieldOff } from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { useLoad } from "@/components/assets/detail/use-load";
import { optionLabel } from "@/components/permits/permit-rules";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
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
import { appErrorReason } from "@/lib/app-error";
import {
  listEquipmentIsolationPointOptions,
  listEquipmentIsolationPoints,
  setEquipmentIsolationPointActive,
  upsertEquipmentIsolationPoint,
} from "@/services/asset-isolation-point-service";
import type { EquipmentIsolationPoint } from "@/services/permit-safety-service";
import type { IsolationOptions } from "@/services/permit-service";
import type { Asset } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface Draft {
  id: number | null;
  rowVersion: number | null;
  code: string;
  label: string;
  energy: string;
  method: string;
  normalState: string;
}

const EMPTY: Draft = {
  id: null,
  rowVersion: null,
  code: "",
  label: "",
  energy: "",
  method: "",
  normalState: "",
};

interface Data {
  points: EquipmentIsolationPoint[];
  options: IsolationOptions;
}

export function AssetIsolationPointsTab({ asset }: { asset: Asset }) {
  const { t, i18n } = useTranslation("equipment");
  const lang = i18n.resolvedLanguage || i18n.language;
  const [showInactive, setShowInactive] = useState(false);
  const load = useMemo(
    () => async (): Promise<Data> => {
      const [points, options] = await Promise.all([
        listEquipmentIsolationPoints(asset.id, showInactive),
        listEquipmentIsolationPointOptions(),
      ]);
      return { points, options };
    },
    [asset.id, showInactive],
  );
  const { data, error, loading, reload } = useLoad(load, `${asset.id}:${String(showInactive)}`);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [writeError, setWriteError] = useState<string | null>(null);

  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }

  const run = async (fn: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    setWriteError(null);
    try {
      await fn();
      await reload();
      return true;
    } catch (e) {
      setWriteError(appErrorReason(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!draft || !draft.code.trim() || !draft.label.trim() || !draft.energy || !draft.method)
      return;
    const ok = await run(() =>
      upsertEquipmentIsolationPoint({
        id: draft.id,
        equipment_id: asset.id,
        expected_row_version: draft.rowVersion,
        point_code: draft.code,
        label: draft.label,
        energy_type_id: Number(draft.energy),
        isolation_method_id: Number(draft.method),
        normal_state: draft.normalState.trim() || null,
      }),
    );
    if (ok) {
      setDraft(null);
      setSubmitted(false);
    }
  };

  const required = (v: string) =>
    submitted && !v.trim() ? t("isolationPoints.required") : undefined;

  return (
    <div className="space-y-4" data-testid="asset-isolation-points">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <h3 className="text-sm font-medium text-text-primary">{t("isolationPoints.title")}</h3>
          <p className="text-xs text-text-muted">{t("isolationPoints.description")}</p>
        </div>
        <PermissionGate permission={P.EQ_MANAGE}>
          <Button
            variant="outline"
            className="h-11 gap-1.5"
            onClick={() => {
              setSubmitted(false);
              setDraft(EMPTY);
            }}
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("isolationPoints.add")}
          </Button>
        </PermissionGate>
      </div>
      <label className="flex items-center gap-2 text-xs text-text-muted">
        <Checkbox checked={showInactive} onCheckedChange={(v) => setShowInactive(v === true)} />
        {t("isolationPoints.showInactive")}
      </label>

      {writeError ? (
        <p
          role="alert"
          className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
        >
          {writeError}
        </p>
      ) : null}

      {draft ? (
        <form
          className="grid gap-2 rounded-md border border-dashed border-surface-border p-3 sm:grid-cols-2"
          onSubmit={(e) => void handleSave(e)}
        >
          <p className="text-sm font-medium sm:col-span-2">
            {draft.id == null
              ? t("isolationPoints.dialogTitleNew")
              : t("isolationPoints.dialogTitleEdit")}
          </p>
          <FormField
            name="ip-code"
            label={t("isolationPoints.code")}
            required
            error={required(draft.code)}
          >
            <Input
              id="ip-code"
              value={draft.code}
              onChange={(e) => setDraft({ ...draft, code: e.target.value })}
            />
          </FormField>
          <FormField
            name="ip-label"
            label={t("isolationPoints.label")}
            required
            error={required(draft.label)}
          >
            <Input
              id="ip-label"
              value={draft.label}
              onChange={(e) => setDraft({ ...draft, label: e.target.value })}
            />
          </FormField>
          <FormField
            name="ip-energy"
            label={t("isolationPoints.energy")}
            required
            error={required(draft.energy)}
          >
            <Select value={draft.energy} onValueChange={(v) => setDraft({ ...draft, energy: v })}>
              <SelectTrigger id="ip-energy" aria-label={t("isolationPoints.energy")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {data.options.energy_types.map((o) => (
                  <SelectItem key={o.id} value={String(o.id)}>
                    {optionLabel(o, lang)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
          <FormField
            name="ip-method"
            label={t("isolationPoints.method")}
            required
            error={required(draft.method)}
          >
            <Select value={draft.method} onValueChange={(v) => setDraft({ ...draft, method: v })}>
              <SelectTrigger id="ip-method" aria-label={t("isolationPoints.method")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {data.options.isolation_methods.map((o) => (
                  <SelectItem key={o.id} value={String(o.id)}>
                    {optionLabel(o, lang)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
          <FormField name="ip-normal" label={t("isolationPoints.normalState")}>
            <Input
              id="ip-normal"
              placeholder={t("isolationPoints.normalStatePlaceholder")}
              value={draft.normalState}
              onChange={(e) => setDraft({ ...draft, normalState: e.target.value })}
            />
          </FormField>
          <p className="text-xs text-text-muted sm:col-span-2">{t("isolationPoints.listsHint")}</p>
          <div className="flex gap-2 sm:col-span-2">
            <Button type="submit" size="sm" disabled={busy}>
              {t("isolationPoints.save")}
            </Button>
            <Button type="button" size="sm" variant="ghost" onClick={() => setDraft(null)}>
              {t("isolationPoints.cancel")}
            </Button>
          </div>
        </form>
      ) : null}

      {data.points.length === 0 ? (
        <EmptyState
          icon={<ShieldOff className="h-8 w-8" aria-hidden />}
          title={t("isolationPoints.empty")}
          description={t("isolationPoints.description")}
        />
      ) : (
        <ul className="space-y-2">
          {data.points.map((p) => {
            const energy = data.options.energy_types.find((o) => o.id === p.energy_type_id);
            const method = data.options.isolation_methods.find(
              (o) => o.id === p.isolation_method_id,
            );
            return (
              <li
                key={p.id}
                data-testid="asset-isolation-point"
                className="flex flex-wrap items-center gap-2 rounded-md border border-surface-border p-2 text-sm"
              >
                <span className="font-mono font-medium">{p.point_code}</span>
                <span>{p.label}</span>
                <span className="text-xs text-text-muted">
                  {(optionLabel(energy, lang) || p.energy_type_label) ?? ""}
                  {" · "}
                  {(optionLabel(method, lang) || p.isolation_method_label) ?? ""}
                  {p.normal_state
                    ? ` · ${t("isolationPoints.normalState")} : ${p.normal_state}`
                    : ""}
                </span>
                {!p.is_active ? (
                  <StatusChip tone="neutral" label={t("isolationPoints.inactive")} />
                ) : null}
                <PermissionGate permission={P.EQ_MANAGE}>
                  <div className="ml-auto flex gap-2">
                    {p.is_active ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        onClick={() => {
                          setSubmitted(false);
                          setDraft({
                            id: p.id,
                            rowVersion: p.row_version,
                            code: p.point_code,
                            label: p.label,
                            energy: String(p.energy_type_id),
                            method: String(p.isolation_method_id),
                            normalState: p.normal_state ?? "",
                          });
                        }}
                      >
                        {t("isolationPoints.edit")}
                      </Button>
                    ) : null}
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={busy}
                      onClick={() =>
                        void run(() =>
                          setEquipmentIsolationPointActive({
                            id: p.id,
                            expected_row_version: p.row_version,
                            is_active: !p.is_active,
                          }),
                        )
                      }
                    >
                      {p.is_active
                        ? t("isolationPoints.deactivate")
                        : t("isolationPoints.reactivate")}
                    </Button>
                  </div>
                </PermissionGate>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
