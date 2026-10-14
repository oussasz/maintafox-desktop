/**
 * Meters tab: the asset's counters (running hours, cycles, distance…), recording a reading and
 * the reading history, plus adding a meter. Readings feed meter-based preventive plans; the
 * backend refuses a value below the previous one unless the meter rolls over.
 */

import { Gauge, History, Plus } from "lucide-react";
import { useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailSectionCard } from "@/components/detail";
import { EntityFormDialog, EntityFormFooter } from "@/components/entity-form";
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
import { useFormatters } from "@/hooks/use-formatters";
import {
  createAssetMeter,
  listAssetMeters,
  listMeterReadings,
  recordMeterReading,
} from "@/services/asset-lifecycle-service";
import { getLookupValues } from "@/services/lookup-service";
import type { Asset, AssetMeter, LookupValueOption, MeterReading } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Readings shown in a meter's history. */
const HISTORY_ROWS = 20;

function optionLabel(v: LookupValueOption, language: string): string {
  const localized = language.startsWith("fr") ? v.fr_label : v.en_label;
  return localized?.trim() || v.label;
}

/** `datetime-local` value for now, in local time. */
function nowLocalInput(): string {
  const d = new Date();
  d.setMinutes(d.getMinutes() - d.getTimezoneOffset());
  return d.toISOString().slice(0, 16);
}

/** null when empty, NaN when not a number. */
function parseNumber(raw: string): number | null {
  const v = raw.trim();
  if (v === "") return null;
  const n = Number(v.replace(",", "."));
  return Number.isFinite(n) ? n : Number.NaN;
}

interface MetersData {
  meters: AssetMeter[];
  types: LookupValueOption[];
  sources: LookupValueOption[];
}

export function AssetMetersTab({ asset }: { asset: Asset }) {
  const { t, i18n } = useTranslation("equipment");
  const { formatDecimal, formatDateTime } = useFormatters();
  const load = useMemo(
    () => async (): Promise<MetersData> => {
      const [meters, types, sources] = await Promise.all([
        listAssetMeters(asset.id),
        getLookupValues("equipment.meter_type"),
        getLookupValues("equipment.reading_source_type"),
      ]);
      return { meters, types, sources };
    },
    [asset.id],
  );
  const { data, error, loading, reload } = useLoad(load, String(asset.id));
  const [readingFor, setReadingFor] = useState<AssetMeter | null>(null);
  const [historyFor, setHistoryFor] = useState<number | null>(null);
  const [addOpen, setAddOpen] = useState(false);

  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  const typeLabel = (code: string) => {
    const opt = data.types.find((o) => o.code === code);
    return opt ? optionLabel(opt, i18n.language) : code;
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <PermissionGate permission={P.EQ_MANAGE}>
          <Button variant="outline" className="h-11 gap-1.5" onClick={() => setAddOpen(true)}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("meters.add")}
          </Button>
        </PermissionGate>
      </div>
      {data.meters.length === 0 ? (
        <EmptyState
          icon={<Gauge className="h-8 w-8" aria-hidden />}
          title={t("empty.noMeters")}
          description={t("meters.emptyHint")}
        />
      ) : (
        data.meters.map((m) => (
          <DetailSectionCard key={m.id} title={m.name} icon={Gauge}>
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-2xl font-semibold tabular-nums">
                {formatDecimal(m.current_reading, 1)}
              </span>
              <span className="text-sm text-text-muted">{m.unit ?? ""}</span>
              <StatusChip tone="neutral" label={typeLabel(m.meter_type)} />
              {m.is_primary ? (
                <StatusChip tone="info" label={t("registry.detail.primary")} />
              ) : null}
              {!m.is_active ? <StatusChip tone="neutral" label={t("meters.inactive")} /> : null}
            </div>
            <p className="text-xs text-text-muted">
              {m.last_read_at
                ? t("meters.lastRead", { date: formatDateTime(m.last_read_at) })
                : t("meters.neverRead")}
              {m.expected_rate_per_day != null
                ? ` · ${t("meters.expectedRate", {
                    rate: formatDecimal(m.expected_rate_per_day, 1),
                    unit: m.unit ?? "",
                  })}`
                : ""}
            </p>
            <div className="flex flex-wrap gap-2 pt-1">
              <PermissionGate permission={P.EQ_MANAGE}>
                <Button
                  className="h-9 gap-1.5"
                  disabled={!m.is_active}
                  onClick={() => setReadingFor(m)}
                >
                  <Plus className="h-4 w-4" aria-hidden />
                  {t("meters.record")}
                </Button>
              </PermissionGate>
              <Button
                variant="outline"
                className="h-9 gap-1.5"
                aria-expanded={historyFor === m.id}
                onClick={() => setHistoryFor((cur) => (cur === m.id ? null : m.id))}
              >
                <History className="h-4 w-4" aria-hidden />
                {historyFor === m.id ? t("meters.hideHistory") : t("meters.showHistory")}
              </Button>
            </div>
            {historyFor === m.id ? <MeterHistory meter={m} sources={data.sources} /> : null}
          </DetailSectionCard>
        ))
      )}

      {readingFor ? (
        <RecordReadingDialog
          meter={readingFor}
          sources={data.sources}
          onClose={() => setReadingFor(null)}
          onSaved={() => {
            setReadingFor(null);
            void reload();
          }}
        />
      ) : null}
      <AddMeterDialog
        open={addOpen}
        asset={asset}
        types={data.types}
        hasPrimary={data.meters.some((m) => m.is_primary && m.is_active)}
        onClose={() => setAddOpen(false)}
        onSaved={() => {
          setAddOpen(false);
          void reload();
        }}
      />
    </div>
  );
}

function MeterHistory({ meter, sources }: { meter: AssetMeter; sources: LookupValueOption[] }) {
  const { t, i18n } = useTranslation("equipment");
  const { formatDecimal, formatDateTime } = useFormatters();
  const load = useMemo(() => () => listMeterReadings(meter.id, HISTORY_ROWS), [meter.id]);
  const { data, error, loading, reload } = useLoad<MeterReading[]>(
    load,
    `${meter.id}:${meter.last_read_at ?? ""}`,
  );
  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  if (data.length === 0) return <p className="text-sm text-text-muted">{t("meters.noReadings")}</p>;
  return (
    <table className="w-full text-xs">
      <caption className="sr-only">{t("meters.historyCaption", { name: meter.name })}</caption>
      <thead>
        <tr className="text-left text-text-muted">
          <th className="py-1 font-medium">{t("meters.readingAt")}</th>
          <th className="py-1 text-right font-medium">{t("meters.value")}</th>
          <th className="py-1 pl-3 font-medium">{t("meters.source")}</th>
          <th className="py-1 font-medium">{t("meters.reference")}</th>
        </tr>
      </thead>
      <tbody>
        {data.map((r) => {
          const src = sources.find((s) => s.code === r.source_type);
          return (
            <tr key={r.id} className="border-t border-surface-border">
              <td className="py-1">{formatDateTime(r.reading_at)}</td>
              <td className="py-1 text-right tabular-nums">
                {formatDecimal(r.reading_value, 1)} {meter.unit ?? ""}
              </td>
              <td className="py-1 pl-3">{src ? optionLabel(src, i18n.language) : r.source_type}</td>
              <td className="py-1">{r.source_reference ?? "—"}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function RecordReadingDialog({
  meter,
  sources,
  onClose,
  onSaved,
}: {
  meter: AssetMeter;
  sources: LookupValueOption[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t, i18n } = useTranslation("equipment");
  const { formatDecimal } = useFormatters();
  const formId = useId();
  const [value, setValue] = useState("");
  const [readingAt, setReadingAt] = useState(nowLocalInput);
  const [source, setSource] = useState(
    () => sources.find((s) => s.code === "MANUAL")?.code ?? sources[0]?.code ?? "",
  );
  const [reference, setReference] = useState("");
  const [saving, setSaving] = useState(false);

  const parsed = parseNumber(value);
  const valueError =
    value.trim() === ""
      ? undefined
      : parsed === null || Number.isNaN(parsed)
        ? t("meters.errors.number")
        : parsed < meter.current_reading && meter.rollover_value == null
          ? t("meters.errors.belowCurrent", { current: formatDecimal(meter.current_reading, 1) })
          : undefined;
  const future = readingAt > nowLocalInput();
  const invalid = parsed === null || Number.isNaN(parsed) || !!valueError || future || !source;

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (invalid || parsed === null) return;
    setSaving(true);
    try {
      await recordMeterReading({
        meter_id: meter.id,
        reading_value: parsed,
        reading_at: new Date(readingAt).toISOString(),
        source_type: source,
        source_reference: reference.trim() || null,
      });
      onSaved();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <EntityFormDialog
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={t("meters.recordTitle", { name: meter.name })}
      description={t("meters.currentValue", {
        value: formatDecimal(meter.current_reading, 1),
        unit: meter.unit ?? "",
      })}
      footer={
        <EntityFormFooter
          cancelLabel={t("commercial.cancel")}
          primaryLabel={t("meters.record")}
          onCancel={onClose}
          formId={formId}
          primaryDisabled={invalid}
          primaryLoading={saving}
        />
      }
    >
      <form id={formId} onSubmit={(e) => void submit(e)} className="grid gap-4 sm:grid-cols-2">
        <FormField name={`${formId}-value`} label={t("meters.value")} required error={valueError}>
          <Input
            id={`${formId}-value`}
            inputMode="decimal"
            value={value}
            onChange={(e) => setValue(e.target.value)}
          />
        </FormField>
        <FormField
          name={`${formId}-at`}
          label={t("meters.readingAt")}
          required
          error={future ? t("meters.errors.future") : undefined}
        >
          <Input
            id={`${formId}-at`}
            type="datetime-local"
            value={readingAt}
            max={nowLocalInput()}
            onChange={(e) => setReadingAt(e.target.value)}
          />
        </FormField>
        <FormField name={`${formId}-source`} label={t("meters.source")} required>
          <Select value={source} onValueChange={setSource}>
            <SelectTrigger id={`${formId}-source`} className="h-10">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sources.map((s) => (
                <SelectItem key={s.id} value={s.code}>
                  {optionLabel(s, i18n.language)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>
        <FormField
          name={`${formId}-ref`}
          label={t("meters.reference")}
          description={t("meters.referenceHint")}
        >
          <Input
            id={`${formId}-ref`}
            value={reference}
            onChange={(e) => setReference(e.target.value)}
          />
        </FormField>
      </form>
    </EntityFormDialog>
  );
}

function AddMeterDialog({
  open,
  asset,
  types,
  hasPrimary,
  onClose,
  onSaved,
}: {
  open: boolean;
  asset: Asset;
  types: LookupValueOption[];
  hasPrimary: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t, i18n } = useTranslation("equipment");
  const formId = useId();
  const [name, setName] = useState("");
  const [code, setCode] = useState("");
  const [type, setType] = useState("");
  const [unit, setUnit] = useState("");
  const [initial, setInitial] = useState("");
  const [rate, setRate] = useState("");
  const [rollover, setRollover] = useState("");
  const [primary, setPrimary] = useState(!hasPrimary);
  const [saving, setSaving] = useState(false);

  const nums = {
    initial: parseNumber(initial),
    rate: parseNumber(rate),
    rollover: parseNumber(rollover),
  };
  const isBad = (n: number | null) => n !== null && (Number.isNaN(n) || n < 0);
  const numberError = (n: number | null) => (isBad(n) ? t("meters.errors.positive") : undefined);
  const invalid =
    !name.trim() || !type || isBad(nums.initial) || isBad(nums.rate) || isBad(nums.rollover);

  const reset = () => {
    setName("");
    setCode("");
    setType("");
    setUnit("");
    setInitial("");
    setRate("");
    setRollover("");
    setPrimary(!hasPrimary);
  };
  const cancel = () => {
    reset();
    onClose();
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (invalid) return;
    setSaving(true);
    try {
      await createAssetMeter({
        asset_id: asset.id,
        name: name.trim(),
        meter_code: code.trim() || null,
        meter_type: type,
        unit: unit.trim() || null,
        initial_reading: nums.initial,
        expected_rate_per_day: nums.rate,
        rollover_value: nums.rollover,
        is_primary: primary,
      });
      reset();
      onSaved();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <EntityFormDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) cancel();
      }}
      title={t("meters.addTitle", { code: asset.asset_code })}
      footer={
        <EntityFormFooter
          cancelLabel={t("commercial.cancel")}
          primaryLabel={t("meters.add")}
          onCancel={cancel}
          formId={formId}
          primaryDisabled={invalid}
          primaryLoading={saving}
        />
      }
    >
      <form id={formId} onSubmit={(e) => void submit(e)} className="grid gap-4 sm:grid-cols-2">
        <FormField name={`${formId}-name`} label={t("meters.name")} required>
          <Input id={`${formId}-name`} value={name} onChange={(e) => setName(e.target.value)} />
        </FormField>
        <FormField name={`${formId}-code`} label={t("meters.code")}>
          <Input id={`${formId}-code`} value={code} onChange={(e) => setCode(e.target.value)} />
        </FormField>
        <FormField name={`${formId}-type`} label={t("meters.type")} required>
          <Select value={type} onValueChange={setType}>
            <SelectTrigger id={`${formId}-type`} className="h-10">
              <SelectValue placeholder={t("meters.typePlaceholder")} />
            </SelectTrigger>
            <SelectContent>
              {types.map((o) => (
                <SelectItem key={o.id} value={o.code}>
                  {optionLabel(o, i18n.language)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>
        <FormField
          name={`${formId}-unit`}
          label={t("meters.unit")}
          description={t("meters.unitHint")}
        >
          <Input id={`${formId}-unit`} value={unit} onChange={(e) => setUnit(e.target.value)} />
        </FormField>
        <FormField
          name={`${formId}-initial`}
          label={t("meters.initial")}
          error={numberError(nums.initial)}
        >
          <Input
            id={`${formId}-initial`}
            inputMode="decimal"
            value={initial}
            onChange={(e) => setInitial(e.target.value)}
          />
        </FormField>
        <FormField
          name={`${formId}-rate`}
          label={t("meters.rate")}
          description={t("meters.rateHint")}
          error={numberError(nums.rate)}
        >
          <Input
            id={`${formId}-rate`}
            inputMode="decimal"
            value={rate}
            onChange={(e) => setRate(e.target.value)}
          />
        </FormField>
        <FormField
          name={`${formId}-rollover`}
          label={t("meters.rollover")}
          description={t("meters.rolloverHint")}
          error={numberError(nums.rollover)}
        >
          <Input
            id={`${formId}-rollover`}
            inputMode="decimal"
            value={rollover}
            onChange={(e) => setRollover(e.target.value)}
          />
        </FormField>
        <label className="flex min-h-[44px] items-center gap-2 self-end text-sm">
          <Checkbox checked={primary} onCheckedChange={(v) => setPrimary(v === true)} />
          {t("meters.primary")}
        </label>
      </form>
    </EntityFormDialog>
  );
}
