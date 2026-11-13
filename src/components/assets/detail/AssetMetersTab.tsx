/**
 * Meters tab: the asset's counters (running hours, cycles, distance…), recording a reading and
 * the reading history, plus adding a meter. Readings feed meter-based preventive plans; the
 * backend refuses a value below the previous one unless the meter rolls over. A meter may carry
 * an alert and a critical threshold: a reading that crosses one raises an intervention request
 * (one per meter and level while it is open), said when the reading is saved.
 */

import type { TFunction } from "i18next";
import { Ban, BellRing, Gauge, History, Plus, RefreshCw } from "lucide-react";
import { useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailSectionCard } from "@/components/detail";
import { EntityFormDialog, EntityFormFooter } from "@/components/entity-form";
import {
  MeasureUnitSelect,
  unitSymbol,
  useMeasureUnits,
} from "@/components/lookups/MeasureUnitPicker";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DateTimeField } from "@/components/ui/date-time-field";
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
import { Textarea } from "@/components/ui/textarea";
import { mfTable } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { gateSentenceText } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  assessMeterReading,
  cancelMeterReading,
  createAssetMeter,
  listAssetMeters,
  listMeterReadings,
  recordMeterReading,
  setMeterThresholds,
} from "@/services/asset-lifecycle-service";
import { getLookupValues } from "@/services/lookup-service";
import { pushAppToast } from "@/store/app-toast-store";
import type {
  Asset,
  AssetMeter,
  LookupValueOption,
  MeterReading,
  MeterReadingWarning,
  MeterThresholdDirection,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Readings shown in a meter's history. */
const HISTORY_ROWS = 20;

function optionLabel(v: LookupValueOption, language: string): string {
  const localized = language.startsWith("fr") ? v.fr_label : v.en_label;
  return localized?.trim() || v.label;
}

/** Unit symbol proposed for a meter type of the system list `equipment.meter_type`. */
const METER_TYPE_DEFAULT_UNIT: Record<string, string> = { HOURS: "h", DISTANCE: "km" };

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
  const [replaceFor, setReplaceFor] = useState<AssetMeter | null>(null);
  // Meter whose correct reading is entered right after cancelling a wrong one.
  const [reopenFor, setReopenFor] = useState<number | null>(null);
  const [historyFor, setHistoryFor] = useState<number | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [thresholdsFor, setThresholdsFor] = useState<AssetMeter | null>(null);

  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  const reopenMeter = reopenFor != null ? data.meters.find((x) => x.id === reopenFor) : undefined;
  if (reopenMeter && !readingFor) {
    setReopenFor(null);
    setReadingFor(reopenMeter);
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
            {m.threshold_direction ? (
              <p className="flex items-center gap-1.5 text-xs">
                <BellRing className="h-3.5 w-3.5 text-tone-warning-fg" aria-hidden />
                {t(`meters.thresholds.summary.${m.threshold_direction}`, {
                  alert:
                    m.alert_threshold != null
                      ? formatDecimal(m.alert_threshold, 1)
                      : t("meters.thresholds.none"),
                  critical:
                    m.critical_threshold != null
                      ? formatDecimal(m.critical_threshold, 1)
                      : t("meters.thresholds.none"),
                  unit: m.unit ?? "",
                })}
              </p>
            ) : null}
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
              <PermissionGate permission={P.EQ_MANAGE}>
                <Button
                  variant="outline"
                  className="h-9 gap-1.5"
                  disabled={!m.is_active}
                  onClick={() => setThresholdsFor(m)}
                >
                  <BellRing className="h-4 w-4" aria-hidden />
                  {t("meters.thresholds.edit")}
                </Button>
              </PermissionGate>
              <PermissionGate permission={P.EQ_MANAGE}>
                <Button
                  variant="outline"
                  className="h-9 gap-1.5"
                  disabled={!m.is_active}
                  onClick={() => setReplaceFor(m)}
                >
                  <RefreshCw className="h-4 w-4" aria-hidden />
                  {t("meters.replace.action")}
                </Button>
              </PermissionGate>
            </div>
            {historyFor === m.id ? (
              <MeterHistory
                meter={m}
                sources={data.sources}
                onCancelled={(thenRecord) => {
                  if (thenRecord) setReopenFor(m.id);
                  void reload();
                }}
              />
            ) : null}
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
      {replaceFor ? (
        <RecordReadingDialog
          meter={replaceFor}
          sources={data.sources}
          replacement
          onClose={() => setReplaceFor(null)}
          onSaved={() => {
            setReplaceFor(null);
            void reload();
          }}
        />
      ) : null}
      {thresholdsFor ? (
        <ThresholdsDialog
          meter={thresholdsFor}
          onClose={() => setThresholdsFor(null)}
          onSaved={() => {
            setThresholdsFor(null);
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

function MeterHistory({
  meter,
  sources,
  onCancelled,
}: {
  meter: AssetMeter;
  sources: LookupValueOption[];
  onCancelled: (thenRecord: boolean) => void;
}) {
  const { t, i18n } = useTranslation("equipment");
  const { formatDecimal, formatDateTime } = useFormatters();
  const [cancelling, setCancelling] = useState<MeterReading | null>(null);
  const load = useMemo(() => () => listMeterReadings(meter.id, HISTORY_ROWS), [meter.id]);
  const { data, error, loading, reload } = useLoad<MeterReading[]>(
    load,
    `${meter.id}:${meter.last_read_at ?? ""}:${meter.current_reading}`,
  );
  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  if (data.length === 0) return <p className="text-sm text-text-muted">{t("meters.noReadings")}</p>;
  // Only the latest counted reading can be cancelled (older ones: cancel the newer first).
  // The starting value of a meter is not kept as a reading, so its only reading cannot be cancelled.
  const counted = data.filter(
    (r) => !r.is_cancelled && r.quality_flag !== "corrected" && r.quality_flag !== "rejected",
  );
  const latestValidId = counted.length >= 2 ? counted[0]?.id : undefined;
  return (
    <>
      <table className="w-full text-xs">
        <caption className="sr-only">{t("meters.historyCaption", { name: meter.name })}</caption>
        <thead>
          <tr className={cn("text-start", mfTable.header)}>
            <th className="py-1 font-medium">{t("meters.readingAt")}</th>
            <th className="py-1 text-right font-medium">{t("meters.value")}</th>
            <th className="py-1 pl-3 font-medium">{t("meters.source")}</th>
            <th className="py-1 font-medium">{t("meters.readBy")}</th>
            <th className="py-1 font-medium">{t("meters.reference")}</th>
            <th className="py-1" />
          </tr>
        </thead>
        <tbody>
          {data.map((r) => {
            const src = sources.find((s) => s.code === r.source_type);
            return (
              <tr
                key={r.id}
                className={cn(
                  "border-t border-surface-border",
                  r.is_cancelled && "text-text-muted",
                )}
              >
                <td className={cn("py-1", r.is_cancelled && "line-through")}>
                  {formatDateTime(r.reading_at)}
                </td>
                <td
                  className={cn("py-1 text-right tabular-nums", r.is_cancelled && "line-through")}
                >
                  {formatDecimal(r.reading_value, 1)} {meter.unit ?? ""}
                </td>
                <td className="py-1 pl-3">
                  {src ? optionLabel(src, i18n.language) : r.source_type}
                </td>
                <td className="py-1">{r.created_by_name ?? "—"}</td>
                <td className="py-1">
                  {r.is_cancelled ? (
                    <span className="space-y-0.5">
                      <StatusChip tone="neutral" label={t("meters.cancelledBadge")} />
                      <span className="block">
                        {t("meters.cancelledBy", {
                          name: r.cancelled_by_name ?? "—",
                          date: r.cancelled_at ? formatDateTime(r.cancelled_at) : "—",
                          reason: r.cancel_reason ?? "",
                        })}
                      </span>
                    </span>
                  ) : (
                    (r.source_reference ?? "—")
                  )}
                  {r.raised_request_code ? (
                    <span className="mt-0.5 block">
                      <StatusChip
                        tone={r.threshold_level === "critical" ? "danger" : "warning"}
                        label={t("meters.thresholds.raisedChip", {
                          code: r.raised_request_code,
                          level: t(`meters.thresholds.level.${r.threshold_level ?? "alert"}`),
                        })}
                      />
                    </span>
                  ) : null}
                </td>
                <td className="py-1 text-right">
                  {r.id === latestValidId ? (
                    <PermissionGate permission={P.EQ_MANAGE}>
                      <Button
                        variant="outline"
                        className="h-7 gap-1 px-2 text-xs"
                        onClick={() => setCancelling(r)}
                      >
                        <Ban className="h-3 w-3" aria-hidden />
                        {t("meters.cancel")}
                      </Button>
                    </PermissionGate>
                  ) : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {cancelling ? (
        <CancelReadingDialog
          meter={meter}
          reading={cancelling}
          onClose={() => setCancelling(null)}
          onDone={(thenRecord) => {
            setCancelling(null);
            onCancelled(thenRecord);
          }}
        />
      ) : null}
    </>
  );
}

function CancelReadingDialog({
  meter,
  reading,
  onClose,
  onDone,
}: {
  meter: AssetMeter;
  reading: MeterReading;
  onClose: () => void;
  onDone: (thenRecord: boolean) => void;
}) {
  const { t } = useTranslation("equipment");
  const { formatDecimal, formatDateTime } = useFormatters();
  const formId = useId();
  const [reason, setReason] = useState("");
  const [thenRecord, setThenRecord] = useState(true);
  const [saving, setSaving] = useState(false);
  const invalid = reason.trim() === "";

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (invalid) return;
    setSaving(true);
    try {
      await cancelMeterReading(reading.id, reason.trim());
      onDone(thenRecord);
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
      title={t("meters.cancelTitle", { name: meter.name })}
      description={t("meters.cancelIntro", {
        value: formatDecimal(reading.reading_value, 1),
        unit: meter.unit ?? "",
        date: formatDateTime(reading.reading_at),
      })}
      footer={
        <EntityFormFooter
          cancelLabel={t("commercial.cancel")}
          primaryLabel={t("meters.cancelConfirm")}
          onCancel={onClose}
          formId={formId}
          primaryDisabled={invalid}
          primaryLoading={saving}
        />
      }
    >
      <form id={formId} onSubmit={(e) => void submit(e)} className="grid gap-4">
        <FormField
          name={`${formId}-reason`}
          label={t("meters.cancelReason")}
          description={t("meters.cancelReasonHint")}
          required
        >
          <Textarea
            id={`${formId}-reason`}
            value={reason}
            rows={3}
            onChange={(e) => setReason(e.target.value)}
          />
        </FormField>
        <label className="flex min-h-[44px] items-center gap-2 text-sm">
          <Checkbox checked={thenRecord} onCheckedChange={(v) => setThenRecord(v === true)} />
          {t("meters.cancelThenRecord")}
        </label>
        <p className="text-xs text-text-muted">{t("meters.cancelPmNote")}</p>
      </form>
    </EntityFormDialog>
  );
}

function RecordReadingDialog({
  meter,
  sources,
  onClose,
  onSaved,
  replacement = false,
}: {
  meter: AssetMeter;
  sources: LookupValueOption[];
  onClose: () => void;
  onSaved: () => void;
  /** A new physical meter was fitted: its first value may be lower than the old one (L2-25). */
  replacement?: boolean;
}) {
  const { t, i18n } = useTranslation("equipment");
  const { formatDecimal } = useFormatters();
  const formId = useId();
  const [value, setValue] = useState("");
  const [readingAt, setReadingAt] = useState(nowLocalInput);
  // Until the person changes the time, the reading is dated when it is saved, not when the
  // window was opened (UX L2-25).
  const [timeTouched, setTimeTouched] = useState(false);
  const [source, setSource] = useState(
    () => sources.find((s) => s.code === "MANUAL")?.code ?? sources[0]?.code ?? "",
  );
  const [reference, setReference] = useState("");
  const [saving, setSaving] = useState(false);
  // Set when the typed value is far above the usual use: asks to confirm before saving.
  const [warning, setWarning] = useState<MeterReadingWarning | null>(null);

  const parsed = parseNumber(value);
  const valueError =
    value.trim() === ""
      ? undefined
      : parsed === null || Number.isNaN(parsed)
        ? t("meters.errors.number")
        : !replacement && parsed < meter.current_reading && meter.rollover_value == null
          ? t("meters.errors.belowCurrent", { current: formatDecimal(meter.current_reading, 1) })
          : undefined;
  const future = readingAt > nowLocalInput();
  const invalid = parsed === null || Number.isNaN(parsed) || !!valueError || future || !source;

  const save = async (confirmJump: boolean) => {
    if (invalid || parsed === null) return;
    setSaving(true);
    try {
      const at = timeTouched ? new Date(readingAt).toISOString() : new Date().toISOString();
      if (!confirmJump) {
        const found = await assessMeterReading(meter.id, parsed, at);
        if (found) {
          setWarning(found);
          return;
        }
      }
      const saved = await recordMeterReading(
        {
          meter_id: meter.id,
          reading_value: parsed,
          reading_at: at,
          source_type: source,
          source_reference: reference.trim() || null,
        },
        confirmJump,
        replacement ? "replacement" : null,
      );
      announceThreshold(saved, t);
      onSaved();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    void save(false);
  };

  return (
    <EntityFormDialog
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={
        replacement
          ? t("meters.replace.title", { name: meter.name })
          : t("meters.recordTitle", { name: meter.name })
      }
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
      <form id={formId} onSubmit={submit} className="grid gap-4 sm:grid-cols-2">
        {replacement ? (
          <p className="text-sm text-text-secondary sm:col-span-2">{t("meters.replace.body")}</p>
        ) : null}
        {warning ? (
          <div
            role="alert"
            className="space-y-2 rounded-md border border-status-warning/40 bg-status-warning/10 p-3 text-sm sm:col-span-2"
          >
            <p className="font-medium">{t("meters.jumpTitle")}</p>
            <p>
              {t("meters.jumpBody", {
                delta: formatDecimal(warning.delta, 1),
                previous: formatDecimal(warning.previous_value, 1),
                expected: formatDecimal(warning.expected_delta, 1),
                value: formatDecimal(parsed ?? 0, 1),
                unit: meter.unit ?? "",
              })}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" onClick={() => setWarning(null)}>
                {t("meters.jumpBack")}
              </Button>
              <Button type="button" disabled={saving} onClick={() => void save(true)}>
                {t("meters.jumpConfirm")}
              </Button>
            </div>
          </div>
        ) : null}
        <FormField name={`${formId}-value`} label={t("meters.value")} required error={valueError}>
          <Input
            id={`${formId}-value`}
            inputMode="decimal"
            value={value}
            onChange={(e) => {
              setValue(e.target.value);
              setWarning(null);
            }}
          />
        </FormField>
        <FormField
          name={`${formId}-at`}
          label={t("meters.readingAt")}
          required
          error={future ? t("meters.errors.future") : undefined}
        >
          <DateTimeField
            id={`${formId}-at`}
            value={readingAt}
            max={nowLocalInput()}
            onChange={(v) => {
              setReadingAt(v);
              setTimeTouched(true);
            }}
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
  // Unit of the meter: a unit of the governed list MEASURE.UNITS (Données de référence).
  const { units } = useMeasureUnits(open);
  const [unitId, setUnitId] = useState<number | null>(null);
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
    setUnitId(null);
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
        unit_value_id: unitId,
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
          <Select
            value={type}
            onValueChange={(next) => {
              setType(next);
              // The unit follows the type while none was chosen: an hours meter is in hours, a
              // distance meter in km (standard units of DonnÃ©es de rÃ©fÃ©rence, UX L2-25).
              const symbol = METER_TYPE_DEFAULT_UNIT[next];
              if (unitId == null && symbol) {
                const unit = units.find((u) => unitSymbol(u) === symbol);
                if (unit) setUnitId(unit.id);
              }
            }}
          >
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
          <MeasureUnitSelect
            id={`${formId}-unit`}
            value={unitId}
            units={units}
            onChange={setUnitId}
            allowEmpty
          />
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

/** Says what crossing a threshold led to, once the reading is saved. */
function announceThreshold(reading: MeterReading, t: TFunction<"equipment">): void {
  if (!reading.threshold_level) return;
  const level = t(`meters.thresholds.level.${reading.threshold_level}`);
  if (reading.raised_request_code) {
    pushAppToast({
      title: t("meters.thresholds.raised", { level, code: reading.raised_request_code }),
      variant: "success",
    });
  } else if (reading.open_request_code) {
    pushAppToast({
      title: t("meters.thresholds.alreadyOpen", { level, code: reading.open_request_code }),
    });
  } else if (reading.threshold_problem) {
    pushAppToast({
      title: t("meters.thresholds.notRaised", { level }),
      description: gateSentenceText(reading.threshold_problem),
      variant: "destructive",
    });
  }
}

/** Alert and critical thresholds of a meter, and the side that is abnormal. */
function ThresholdsDialog({
  meter,
  onClose,
  onSaved,
}: {
  meter: AssetMeter;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("equipment");
  const formId = useId();
  const [alert, setAlert] = useState(meter.alert_threshold?.toString() ?? "");
  const [critical, setCritical] = useState(meter.critical_threshold?.toString() ?? "");
  const [direction, setDirection] = useState<MeterThresholdDirection | "">(
    meter.threshold_direction ?? "",
  );
  const [saving, setSaving] = useState(false);
  const alertValue = parseNumber(alert);
  const criticalValue = parseNumber(critical);
  const anySet = alertValue !== null || criticalValue !== null;
  const notNumber = Number.isNaN(alertValue) || Number.isNaN(criticalValue);
  const orderError =
    alertValue !== null &&
    criticalValue !== null &&
    !notNumber &&
    direction !== "" &&
    (direction === "above" ? alertValue >= criticalValue : alertValue <= criticalValue)
      ? t("meters.thresholds.orderError")
      : undefined;
  const invalid = notNumber || !!orderError || (anySet && direction === "");

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (invalid) return;
    setSaving(true);
    try {
      await setMeterThresholds({
        meter_id: meter.id,
        alert_threshold: alertValue,
        critical_threshold: criticalValue,
        threshold_direction: anySet && direction !== "" ? direction : null,
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
      title={t("meters.thresholds.title", { name: meter.name })}
      description={t("meters.thresholds.description")}
      footer={
        <EntityFormFooter
          cancelLabel={t("commercial.cancel")}
          primaryLabel={t("meters.thresholds.save")}
          onCancel={onClose}
          formId={formId}
          primaryDisabled={invalid}
          primaryLoading={saving}
        />
      }
    >
      <form id={formId} onSubmit={(e) => void submit(e)} className="grid gap-4 sm:grid-cols-2">
        <div className="sm:col-span-2">
          <FormField
            name={`${formId}-direction`}
            label={t("meters.thresholds.direction")}
            required={anySet}
          >
            <Select
              value={direction}
              onValueChange={(v) => setDirection(v as MeterThresholdDirection)}
            >
              <SelectTrigger id={`${formId}-direction`} className="h-10">
                <SelectValue placeholder={t("meters.thresholds.directionPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="above">{t("meters.thresholds.above")}</SelectItem>
                <SelectItem value="below">{t("meters.thresholds.below")}</SelectItem>
              </SelectContent>
            </Select>
          </FormField>
        </div>
        <FormField
          name={`${formId}-alert`}
          label={t("meters.thresholds.alert", { unit: meter.unit ?? "" })}
          error={Number.isNaN(alertValue) ? t("meters.errors.number") : undefined}
        >
          <Input
            id={`${formId}-alert`}
            inputMode="decimal"
            value={alert}
            onChange={(e) => setAlert(e.target.value)}
          />
        </FormField>
        <FormField
          name={`${formId}-critical`}
          label={t("meters.thresholds.critical", { unit: meter.unit ?? "" })}
          error={Number.isNaN(criticalValue) ? t("meters.errors.number") : orderError}
        >
          <Input
            id={`${formId}-critical`}
            inputMode="decimal"
            value={critical}
            onChange={(e) => setCritical(e.target.value)}
          />
        </FormField>
        <p className="text-xs text-text-muted sm:col-span-2">{t("meters.thresholds.hint")}</p>
      </form>
    </EntityFormDialog>
  );
}
