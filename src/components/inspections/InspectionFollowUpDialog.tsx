/**
 * Follow-up of an inspection anomaly: a request (DI) or a work order (OT) written from the
 * finding. The title and description are drafted from the real data (checkpoint, equipment,
 * reading, limits, round, technician comment) and stay editable before creation. The created
 * record points back to the anomaly (`source_inspection_anomaly_id`) and the anomaly to it.
 */
import { Loader2 } from "lucide-react";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@/components/ui";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { woTypeLabel } from "@/components/wo/use-wo-type-labels";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type InspectionCreatedFollowUp,
  getInspectionAnomalyRow,
  routeInspectionAnomalyToDi,
  routeInspectionAnomalyToWo,
} from "@/services/inspection-service";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import type { InspectionAnomaly, InspectionAnomalyRow } from "@shared/ipc-types";

import { RESULT_TONE, followUpDraft } from "./inspection-rules";

export type FollowUpKind = "di" | "wo";

export interface FollowUpCreated {
  kind: FollowUpKind;
  record: InspectionCreatedFollowUp;
  anomaly: InspectionAnomaly;
}

interface InspectionFollowUpDialogProps {
  /** Anomaly to follow up; the dialog is open while it is set. */
  anomalyId: number | null;
  kind: FollowUpKind;
  onClose: () => void;
  onCreated: (created: FollowUpCreated) => void;
}

/**
 * Why this follow-up cannot be created (translation key), or null. Same rules as
 * `inspection::routing`: one follow-up per anomaly; both need the equipment of the checkpoint;
 * a request also needs that equipment installed at a location.
 */
export function followUpBlocker(row: InspectionAnomalyRow, kind: FollowUpKind): string | null {
  if (row.anomaly.linked_di_id != null || row.anomaly.linked_work_order_id != null)
    return "followUp.blocked.alreadyRouted";
  if (row.asset_id == null) return "followUp.blocked.noEquipment";
  if (kind === "di" && !row.asset_has_location) return "followUp.blocked.noLocation";
  return null;
}

export function InspectionFollowUpDialog({
  anomalyId,
  kind,
  onClose,
  onCreated,
}: InspectionFollowUpDialogProps) {
  const { t, i18n } = useTranslation("inspections");
  const { formatNumber, formatDateTime } = useFormatters();
  const woTypes = useWorkOrderTypesCatalog((s) => s.types);
  const loadWoTypes = useWorkOrderTypesCatalog((s) => s.load);
  const activeWoTypes = useMemo(() => woTypes.filter((w) => w.is_active), [woTypes]);
  const [row, setRow] = useState<InspectionAnomalyRow | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [woType, setWoType] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    if (kind === "wo") void loadWoTypes();
  }, [kind, loadWoTypes]);

  useEffect(() => {
    if (anomalyId == null) return;
    let cancelled = false;
    setRow(null);
    setLoadError(null);
    setSubmitted(false);
    getInspectionAnomalyRow(anomalyId)
      .then((r) => {
        if (cancelled) return;
        setRow(r);
        const draft = followUpDraft(
          r,
          t,
          (v) => formatNumber(v),
          (v) => formatDateTime(v),
        );
        setTitle(draft.title);
        setDescription(draft.description);
      })
      .catch((err: unknown) => {
        if (!cancelled) setLoadError(err);
      });
    return () => {
      cancelled = true;
    };
    // The draft is written once per anomaly; formatters and t are stable for a language.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [anomalyId, reloadKey]);

  const blocker = row ? followUpBlocker(row, kind) : null;
  const effectiveWoType = woType || activeWoTypes[0]?.code || "";
  const titleMissing = title.trim() === "";
  const canSubmit =
    row != null && blocker == null && !busy && (kind === "di" || effectiveWoType !== "");

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!row || !canSubmit || titleMissing) return;
    setBusy(true);
    try {
      const base = {
        anomaly_id: row.anomaly.id,
        expected_row_version: row.anomaly.row_version,
        title: title.trim(),
        description: description.trim() || null,
      };
      const [record, anomaly] =
        kind === "di"
          ? await routeInspectionAnomalyToDi(base)
          : await routeInspectionAnomalyToWo({ ...base, type_code: effectiveWoType });
      onCreated({ kind, record, anomaly });
    } catch {
      // The global IPC toast reports the failure (describeAppError); the dialog stays open.
    } finally {
      setBusy(false);
    }
  };

  const unit = row?.measurement_unit ? ` ${row.measurement_unit}` : "";

  return (
    <Dialog open={anomalyId != null} onOpenChange={(open) => (!open ? onClose() : undefined)}>
      <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
        <form onSubmit={(e) => void submit(e)} className="space-y-4">
          <DialogHeader>
            <DialogTitle>
              {kind === "di" ? t("followUp.diTitle") : t("followUp.woTitle")}
            </DialogTitle>
            <DialogDescription>{t("followUp.intro")}</DialogDescription>
          </DialogHeader>

          {loadError != null ? (
            <ErrorState error={loadError} onRetry={() => setReloadKey((k) => k + 1)} />
          ) : row == null ? (
            <LoadingState />
          ) : (
            <>
              <dl
                className="grid gap-2 rounded-md border border-surface-border bg-surface-2 p-3 text-sm sm:grid-cols-2"
                data-testid="follow-up-context"
              >
                <div>
                  <dt className="text-xs text-text-muted">{t("followUp.equipment")}</dt>
                  <dd className="flex flex-wrap items-center gap-1.5">
                    {row.asset_code ? (
                      <>
                        <LinkedEntityBadge
                          entity="asset"
                          code={row.asset_code}
                          entityId={row.asset_id}
                        />
                        <span>{row.asset_name}</span>
                      </>
                    ) : (
                      <span className="text-text-muted">{t("followUp.noEquipment")}</span>
                    )}
                  </dd>
                </div>
                <div>
                  <dt className="text-xs text-text-muted">{t("followUp.checkpoint")}</dt>
                  <dd>
                    <span className="font-mono text-xs">{row.checkpoint_code}</span>{" "}
                    {row.checkpoint_description}
                  </dd>
                </div>
                <div>
                  <dt className="text-xs text-text-muted">{t("followUp.reading")}</dt>
                  <dd className="flex flex-wrap items-center gap-1.5">
                    {row.numeric_value != null ? (
                      <span className="font-mono">
                        {formatNumber(row.numeric_value)}
                        {unit}
                      </span>
                    ) : null}
                    {row.result_status ? (
                      <StatusChip
                        label={t(`result.${row.result_status}`)}
                        tone={toneOf(RESULT_TONE, row.result_status)}
                      />
                    ) : null}
                  </dd>
                </div>
                <div>
                  <dt className="text-xs text-text-muted">{t("followUp.round")}</dt>
                  <dd>
                    {row.template_name} · {formatDateTime(row.round_scheduled_at)}
                  </dd>
                </div>
              </dl>

              {blocker ? (
                <p
                  role="alert"
                  className="rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-sm text-tone-warning-fg"
                >
                  {t(blocker)}
                </p>
              ) : null}

              <FormField
                name="follow-up-title"
                label={t("followUp.titleLabel")}
                required
                error={submitted && titleMissing ? t("followUp.titleRequired") : undefined}
              >
                <Input
                  id="follow-up-title"
                  value={title}
                  maxLength={200}
                  onChange={(e) => setTitle(e.target.value)}
                />
              </FormField>
              <FormField name="follow-up-description" label={t("followUp.descriptionLabel")}>
                <Textarea
                  id="follow-up-description"
                  rows={7}
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                />
              </FormField>
              {kind === "wo" ? (
                <FormField name="follow-up-wo-type" label={t("review.woType")} required>
                  <Select value={effectiveWoType} onValueChange={setWoType}>
                    <SelectTrigger id="follow-up-wo-type" className="h-11">
                      <SelectValue placeholder={t("followUp.woTypePlaceholder")} />
                    </SelectTrigger>
                    <SelectContent>
                      {activeWoTypes.map((w) => (
                        <SelectItem key={w.code} value={w.code}>
                          {woTypeLabel(w, i18n.language)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </FormField>
              ) : null}
            </>
          )}

          <DialogFooter>
            <Button type="button" variant="outline" className="h-11" onClick={onClose}>
              {t("followUp.cancel")}
            </Button>
            <Button type="submit" className="h-11" disabled={!canSubmit}>
              {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
              {kind === "di" ? t("followUp.createDi") : t("followUp.createWo")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Success line after a follow-up was created, with the link to the new record. */
export function FollowUpCreatedNotice({ created }: { created: FollowUpCreated }) {
  const { t } = useTranslation("inspections");
  return (
    <p
      role="status"
      className="flex flex-wrap items-center gap-2 rounded-md border border-tone-success-border bg-tone-success-bg px-3 py-2 text-sm text-tone-success-fg"
      data-testid="follow-up-created"
    >
      {created.kind === "di" ? t("followUp.createdDi") : t("followUp.createdWo")}
      <LinkedEntityBadge
        entity={created.kind === "di" ? "di" : "work_order"}
        code={created.record.code}
        entityId={created.record.id}
        title={created.record.title}
      />
    </p>
  );
}
