/**
 * Meter reading entered from the PM workspace. A value lower than the last one must be stated as
 * a rollover (the counter passed its maximum) or a meter replacement; usage-based PM counts on
 * the lifetime value, which stays continuous either way.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { Button } from "@/components/ui/button";
import { DateTimeField } from "@/components/ui/date-time-field";
import { DecimalField } from "@/components/ui/decimal-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type PmAssetMeter,
  type PmMeterReadingKind,
  listPmAssetMeters,
  recordPmMeterReading,
} from "@/services/pm-service";
import type { AssetSearchResult } from "@shared/ipc-types";

import { pmUnreportedErrorText } from "./pm-errors";

interface PmMeterReadingDialogProps {
  open: boolean;
  onClose: () => void;
  /** Called after a reading was recorded (the forecast of that asset was re-evaluated). */
  onRecorded: (meter: PmAssetMeter) => void;
}

/** `YYYY-MM-DDTHH:mm` in local time for a datetime-local input. */
function localNow(): string {
  const d = new Date();
  d.setMinutes(d.getMinutes() - d.getTimezoneOffset());
  return d.toISOString().slice(0, 16);
}

export function PmMeterReadingDialog({ open, onClose, onRecorded }: PmMeterReadingDialogProps) {
  const { t } = useTranslation("pm");
  const { formatNumber, formatDateTime } = useFormatters();
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [meters, setMeters] = useState<PmAssetMeter[] | null>(null);
  const [meterId, setMeterId] = useState("");
  const [value, setValue] = useState<number | null>(null);
  const [readAt, setReadAt] = useState(localNow);
  const [kind, setKind] = useState<PmMeterReadingKind>("normal");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setAsset(null);
    setMeters(null);
    setMeterId("");
    setValue(null);
    setReadAt(localNow());
    setKind("normal");
    setError(null);
  }, [open]);

  useEffect(() => {
    setMeters(null);
    setMeterId("");
    if (!asset) return;
    let cancelled = false;
    listPmAssetMeters(asset.id)
      .then((rows) => {
        if (cancelled) return;
        setMeters(rows);
        const first = rows[0];
        if (rows.length === 1 && first) setMeterId(String(first.id));
      })
      .catch(() => {
        // Reported by the global IPC toast.
        if (!cancelled) setMeters([]);
      });
    return () => {
      cancelled = true;
    };
  }, [asset]);

  const meter = meters?.find((m) => String(m.id) === meterId) ?? null;
  const numeric = value;
  const lower = meter != null && numeric != null && numeric < meter.current_reading;

  async function submit() {
    if (!meter || numeric == null || !Number.isFinite(numeric) || numeric < 0) {
      setError(t("errors.readingRequired"));
      return;
    }
    if (lower && kind === "normal") {
      setError(t("errors.readingLower"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const updated = await recordPmMeterReading({
        meter_id: meter.id,
        reading_value: numeric,
        reading_at: new Date(readAt).toISOString(),
        kind,
      });
      onRecorded(updated);
      onClose();
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && !saving && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("dialogs.meterReading.title")}</DialogTitle>
          <DialogDescription>{t("dialogs.meterReading.description")}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 py-2">
          <div className="grid gap-1">
            <Label htmlFor="pm-reading-asset">{t("fields.asset")}</Label>
            <AssetPicker id="pm-reading-asset" value={asset} onChange={setAsset} />
          </div>
          {asset && meters != null && meters.length === 0 ? (
            <p className="text-sm text-tone-warning-fg">{t("hints.noMeters")}</p>
          ) : null}
          {meters && meters.length > 0 ? (
            <div className="grid gap-1">
              <Label htmlFor="pm-reading-meter">{t("fields.meter")}</Label>
              <Select value={meterId} onValueChange={setMeterId}>
                <SelectTrigger id="pm-reading-meter">
                  <SelectValue placeholder={t("errors.meterRequired")} />
                </SelectTrigger>
                <SelectContent>
                  {meters.map((m) => (
                    <SelectItem key={m.id} value={String(m.id)}>
                      {m.name}
                      {m.unit ? ` (${m.unit})` : ""}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {meter ? (
                <p className="text-xs text-text-muted">
                  {t("fields.meterLastReading", {
                    value: formatNumber(meter.current_reading),
                    unit: meter.unit ?? "",
                    at: meter.last_read_at ? formatDateTime(meter.last_read_at) : t("labels.none"),
                  })}
                  {" · "}
                  {t("fields.meterLifetime", {
                    value: formatNumber(meter.lifetime_reading),
                    unit: meter.unit ?? "",
                  })}
                </p>
              ) : null}
            </div>
          ) : null}
          {meter ? (
            <>
              <div className="grid gap-2 sm:grid-cols-2">
                <div className="grid gap-1">
                  <Label htmlFor="pm-reading-value">
                    {meter.unit
                      ? t("fields.readingValue", { unit: meter.unit })
                      : t("fields.readingValueNoUnit")}
                  </Label>
                  <DecimalField
                    id="pm-reading-value"
                    decimals={3}
                    min={0}
                    value={value}
                    onValueChange={setValue}
                  />
                </div>
                <div className="grid gap-1">
                  <Label htmlFor="pm-reading-at">{t("fields.readingAt")}</Label>
                  <DateTimeField id="pm-reading-at" value={readAt} onChange={(v) => setReadAt(v)} />
                </div>
              </div>
              <fieldset className="grid gap-1">
                <legend className="text-sm font-medium">{t("fields.readingKind")}</legend>
                {(["normal", "rollover", "replacement"] as const).map((k) => (
                  <label key={k} className="flex min-h-9 items-start gap-2 text-sm">
                    <input
                      type="radio"
                      name="pm-reading-kind"
                      className="mt-1"
                      value={k}
                      checked={kind === k}
                      disabled={k === "rollover" && meter.rollover_value == null}
                      onChange={() => setKind(k)}
                    />
                    <span>
                      {t(`readingKind.${k}`)}
                      <span className="block text-xs text-text-muted">
                        {k === "rollover" && meter.rollover_value == null
                          ? t("readingKind.rolloverUnavailable")
                          : t(`readingKind.${k}Hint`, {
                              max:
                                meter.rollover_value != null
                                  ? formatNumber(meter.rollover_value)
                                  : "",
                            })}
                      </span>
                    </span>
                  </label>
                ))}
              </fieldset>
              {lower && kind === "normal" ? (
                <p className="text-sm text-tone-warning-fg">{t("errors.readingLower")}</p>
              ) : null}
            </>
          ) : null}
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            {t("actions.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={saving || !meter}>
            {t("actions.recordReading")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
