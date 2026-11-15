/**
 * Technical characteristics of the equipment (power, pressure, voltage…). What a family asks for
 * — label, unit, type and whether it is required — is defined in Données de référence
 * (`EQUIPMENT.CHARACTERISTICS`); this card only shows those fields and stores the values.
 */

import { SlidersHorizontal } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailFieldRow, DetailSectionCard } from "@/components/detail";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import {
  getAssetCharacteristics,
  saveAssetCharacteristics,
} from "@/services/asset-follow-up-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { AssetCharacteristic, AssetCharacteristicSheet } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Value typed for a numeric characteristic is a number (decimal comma accepted). */
export function isNumberText(raw: string): boolean {
  const v = raw.trim().replace(/\s/g, "").replace(",", ".");
  return v !== "" && Number.isFinite(Number(v));
}

/** Problems of the typed values, by code (required and empty, number not a number). */
export function characteristicErrors(
  items: AssetCharacteristic[],
  draft: Record<string, string>,
): Record<string, "required" | "number"> {
  const out: Record<string, "required" | "number"> = {};
  for (const item of items) {
    const v = (draft[item.code] ?? "").trim();
    if (v === "") {
      if (item.required) out[item.code] = "required";
    } else if (item.data_type === "number" && !isNumberText(v)) {
      out[item.code] = "number";
    }
  }
  return out;
}

export function AssetCharacteristicsCard({ assetId }: { assetId: number }) {
  const { t } = useTranslation("equipment");
  const load = useMemo(() => () => getAssetCharacteristics(assetId), [assetId]);
  const { data, error, loading, reload, setData } = useLoad<AssetCharacteristicSheet>(
    load,
    String(assetId),
  );
  const [draft, setDraft] = useState<Record<string, string> | null>(null);
  const [saving, setSaving] = useState(false);

  if (loading && !data) {
    return (
      <DetailSectionCard title={t("characteristics.title")} icon={SlidersHorizontal}>
        <LoadingState />
      </DetailSectionCard>
    );
  }
  if (error || !data) {
    return (
      <DetailSectionCard title={t("characteristics.title")} icon={SlidersHorizontal}>
        <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />
      </DetailSectionCard>
    );
  }

  const family = data.family_label ?? data.family_code ?? "";
  const errors = draft ? characteristicErrors(data.items, draft) : {};
  const startEdit = () =>
    setDraft(Object.fromEntries(data.items.map((i) => [i.code, i.value ?? ""])));
  const save = async () => {
    if (!draft || Object.keys(errors).length > 0) return;
    setSaving(true);
    try {
      const values = Object.fromEntries(
        data.items.map((i) => [i.code, (draft[i.code] ?? "").trim() || null]),
      );
      const next = await saveAssetCharacteristics({ asset_id: assetId, values });
      setData(next);
      setDraft(null);
      pushAppToast({ title: t("characteristics.saved"), variant: "success" });
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <DetailSectionCard title={t("characteristics.title")} icon={SlidersHorizontal}>
      {!data.family_code ? (
        <p className="text-sm text-text-muted">{t("characteristics.noFamily")}</p>
      ) : data.items.length === 0 ? (
        <p className="text-sm text-text-muted">{t("characteristics.none", { family })}</p>
      ) : draft ? (
        <form
          className="grid gap-3 sm:grid-cols-2"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          {data.items.map((item) => (
            <CharacteristicInput
              key={item.code}
              item={item}
              value={draft[item.code] ?? ""}
              error={errors[item.code]}
              onChange={(v) => setDraft({ ...draft, [item.code]: v })}
            />
          ))}
          <div className="flex justify-end gap-2 sm:col-span-2">
            <Button type="button" variant="outline" onClick={() => setDraft(null)}>
              {t("characteristics.cancel")}
            </Button>
            <Button type="submit" disabled={saving || Object.keys(errors).length > 0}>
              {t("characteristics.save")}
            </Button>
          </div>
        </form>
      ) : (
        <>
          {data.items.map((item) => (
            <CharacteristicRow key={item.code} item={item} />
          ))}
          <p className="text-xs text-text-muted">{t("characteristics.hint", { family })}</p>
          <PermissionGate permission={P.EQ_MANAGE}>
            <div className="flex justify-end pt-1">
              <Button variant="outline" className="h-9" onClick={startEdit}>
                {t("characteristics.edit")}
              </Button>
            </div>
          </PermissionGate>
        </>
      )}
      {data.unlisted.length > 0 ? (
        <div className="space-y-1 rounded-md bg-surface-2 p-2 text-xs">
          <p className="font-medium">{t("characteristics.unlisted")}</p>
          {data.unlisted.map((u) => (
            <p key={u.code}>
              <span className="font-mono">{u.code}</span> : {u.value}
            </p>
          ))}
        </div>
      ) : null}
    </DetailSectionCard>
  );
}

function CharacteristicRow({ item }: { item: AssetCharacteristic }) {
  const { t } = useTranslation("equipment");
  const { formatNumber } = useFormatters();
  let shown: string | null = item.value;
  if (item.value != null && item.data_type === "number" && isNumberText(item.value)) {
    shown = `${formatNumber(Number(item.value))}${item.unit_symbol ? ` ${item.unit_symbol}` : ""}`;
  } else if (item.value != null && item.data_type === "boolean") {
    shown = item.value === "true" ? t("characteristics.yes") : t("characteristics.no");
  }
  return (
    <>
      <DetailFieldRow label={item.label} value={shown ?? t("characteristics.notSet")} />
      {item.definition_problem ? (
        <p className="text-xs text-tone-warning-fg">
          {t("characteristics.definitionProblem", { problem: item.definition_problem })}
        </p>
      ) : null}
    </>
  );
}

function CharacteristicInput({
  item,
  value,
  error,
  onChange,
}: {
  item: AssetCharacteristic;
  value: string;
  error: "required" | "number" | undefined;
  onChange: (v: string) => void;
}) {
  const { t } = useTranslation("equipment");
  const id = `characteristic-${item.code}`;
  const label = item.unit_symbol ? `${item.label} (${item.unit_symbol})` : item.label;
  const errorText =
    error === "required"
      ? t("characteristics.requiredError")
      : error === "number"
        ? t("characteristics.numberError")
        : undefined;
  return (
    <FormField name={id} label={label} required={item.required} error={errorText}>
      {item.data_type === "boolean" ? (
        <Select value={value} onValueChange={onChange}>
          <SelectTrigger id={id} className="h-10">
            <SelectValue placeholder={t("characteristics.notSet")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="true">{t("characteristics.yes")}</SelectItem>
            <SelectItem value="false">{t("characteristics.no")}</SelectItem>
          </SelectContent>
        </Select>
      ) : (
        <Input
          id={id}
          inputMode={item.data_type === "number" ? "decimal" : undefined}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      )}
    </FormField>
  );
}
