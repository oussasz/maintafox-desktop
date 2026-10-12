/**
 * Field runner for one inspection round (PRD §6.25): one check at a time, large touch
 * targets, numeric keypad for readings, camera for photos, and the offline queue visible
 * (results saved on this device stay "waiting" until the server has accepted them).
 */
import {
  AlertTriangle,
  ArrowLeft,
  Camera,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  CloudOff,
  CloudUpload,
  Loader2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge, Button, Input, Label, Textarea } from "@/components/ui";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  addInspectionPhotoEvidence,
  completeInspectionRound,
  getInspectionRoundRunner,
  recordInspectionResult,
} from "@/services/inspection-service";
import { toErrorMessage } from "@/utils/errors";
import type { InspectionRoundRunner, InspectionRunnerCheckpoint } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { InspectionRoundReview } from "./InspectionRoundReview";
import {
  MAX_PHOTO_MB,
  NOT_PERFORMED,
  ROUND_OPEN_STATUSES,
  type ResultStatus,
  answerChoices,
  canComplete,
  describeBand,
  firstOpenIndex,
  parseReading,
  pendingSyncCount,
  predictNumericStatus,
} from "./inspection-rules";

interface InspectionRunnerProps {
  roundId: number;
  onClose: () => void;
  /** Called after any change so lists can refresh their counts. */
  onChanged?: () => void;
}

const STATUS_TONE: Record<string, string> = {
  pass: "border-status-success bg-status-success/15 text-text-primary",
  warning: "border-status-warning bg-status-warning/15 text-text-primary",
  fail: "border-status-danger bg-status-danger/15 text-text-primary",
  not_accessible: "border-text-muted bg-surface-2 text-text-primary",
  not_done: "border-text-muted bg-surface-2 text-text-primary",
};

function chipTone(c: InspectionRunnerCheckpoint): string {
  if (!c.result) return "border-surface-border bg-surface-1";
  if (c.missing.length > 0) return "border-status-warning bg-status-warning/10";
  return STATUS_TONE[c.result.result_status] ?? "border-surface-border bg-surface-1";
}

interface Draft {
  status: ResultStatus | null;
  reading: string;
  text: string;
  comment: string;
}

/** Saved answer → editable draft. A numeric reading is shown as the value, not as a status. */
function draftFrom(c: InspectionRunnerCheckpoint | undefined): Draft {
  const r = c?.result;
  const saved = (r?.result_status as ResultStatus | undefined) ?? null;
  const numericAnswer =
    c?.checkpoint.check_type === "numeric" && saved != null && !NOT_PERFORMED.has(saved);
  return {
    status: numericAnswer ? null : saved,
    reading: r?.numeric_value != null ? String(r.numeric_value) : "",
    text: r?.text_value ?? "",
    comment: r?.comment ?? "",
  };
}

export function InspectionRunner({ roundId, onClose, onChanged }: InspectionRunnerProps) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const [runner, setRunner] = useState<InspectionRoundRunner | null>(null);
  const [index, setIndex] = useState(0);
  const [draft, setDraft] = useState<Draft>(draftFrom(undefined));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const load = useCallback(
    async (keepIndex: boolean) => {
      try {
        const data = await getInspectionRoundRunner(roundId);
        setRunner(data);
        setIndex((prev) =>
          keepIndex
            ? Math.min(prev, Math.max(data.checkpoints.length - 1, 0))
            : firstOpenIndex(data.checkpoints),
        );
        setError(null);
      } catch (err) {
        setError(t("errors.load", { message: toErrorMessage(err) }));
      }
    },
    [roundId, t],
  );

  useEffect(() => {
    void load(false);
  }, [load]);

  const current = runner?.checkpoints[index];
  const currentKey = `${current?.checkpoint.id ?? 0}:${current?.result?.row_version ?? 0}`;
  useEffect(() => {
    setDraft(draftFrom(current));
    // Reset only when the check or its saved result changes, not on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentKey]);

  const sync = useMemo(() => pendingSyncCount(runner?.checkpoints ?? []), [runner]);

  if (!runner) {
    return (
      <div className="flex items-center gap-2 p-6 text-sm text-text-muted">
        {error ? <span role="alert">{error}</span> : <Loader2 className="h-5 w-5 animate-spin" />}
      </div>
    );
  }

  const status = runner.summary.round.status;
  const open = ROUND_OPEN_STATUSES.has(status);
  const editable = open && can(P.INS_EXECUTE);
  const total = runner.checkpoints.length;
  const answered = runner.checkpoints.filter((c) => c.result).length;
  const cp = current?.checkpoint;
  const isNumeric = cp?.check_type === "numeric";
  const reading = parseReading(draft.reading);
  const predicted = isNumeric && reading != null && cp ? predictNumericStatus(reading, cp) : null;
  const effectiveStatus: string | null = draft.status ?? predicted;
  const notPerformed = draft.status != null && NOT_PERFORMED.has(draft.status);
  const commentRequired =
    effectiveStatus != null &&
    (NOT_PERFORMED.has(effectiveStatus) ||
      (cp?.requires_comment_on_exception === true && effectiveStatus !== "pass"));
  const commentMissing = commentRequired && draft.comment.trim() === "";
  const hasAnswer = isNumeric ? reading != null || notPerformed : draft.status != null;
  const readyToSave = editable && !busy && !commentMissing && hasAnswer;

  const save = async (advance: boolean) => {
    if (!cp || !readyToSave) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await recordInspectionResult({
        round_id: runner.summary.round.id,
        checkpoint_id: cp.id,
        result_status: isNumeric && !notPerformed ? null : draft.status,
        numeric_value: isNumeric && !notPerformed ? reading : null,
        boolean_value:
          cp.check_type === "boolean" && !notPerformed && draft.status != null
            ? draft.status === "pass"
            : null,
        text_value: cp.check_type === "observation" ? draft.text.trim() || null : null,
        comment: draft.comment.trim() || null,
        expected_row_version: current?.result?.row_version ?? null,
      });
      setNotice(t("runner.saved"));
      await load(true);
      onChanged?.();
      if (advance && index < total - 1) setIndex(index + 1);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const onPhoto = async (file: File | undefined) => {
    if (!file || !current?.result) return;
    if (file.size > MAX_PHOTO_MB * 1024 * 1024) {
      setError(t("runner.photoTooLarge", { mb: MAX_PHOTO_MB }));
      return;
    }
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      await addInspectionPhotoEvidence({
        result_id: current.result.id,
        file_name: file.name,
        mime_type: file.type,
        file_bytes: bytes,
      });
      setNotice(t("runner.photoAdded"));
      await load(true);
      onChanged?.();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  const complete = async () => {
    setBusy(true);
    setError(null);
    try {
      await completeInspectionRound({
        round_id: runner.summary.round.id,
        expected_row_version: runner.summary.round.row_version,
      });
      await load(true);
      onChanged?.();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const photoCount = current?.evidence.filter((e) => e.evidence_type === "photo").length ?? 0;
  const normalBand = cp ? describeBand(cp.normal_min, cp.normal_max) : null;
  const warningBand = cp ? describeBand(cp.warning_min, cp.warning_max) : null;
  const bandText = (b: ReturnType<typeof describeBand>) => {
    if (b == null) return "";
    if (b.kind === "between") return t("runner.between", { min: b.min, max: b.max });
    if (b.kind === "below") return t("runner.below", { value: b.max });
    return t("runner.above", { value: b.min });
  };
  const answerLabel = (s: ResultStatus) =>
    cp?.check_type === "boolean" && (s === "pass" || s === "fail")
      ? t(s === "pass" ? "runner.answers.yes" : "runner.answers.no")
      : t(`runner.answers.${s}`);
  const reviewable =
    status === "completed_with_findings" ||
    (status === "completed" && runner.summary.requires_review);
  const completable = canComplete(runner.checkpoints);

  return (
    <div className="flex flex-col gap-4" data-testid="inspection-runner">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="outline" size="lg" className="min-h-12" onClick={onClose}>
          <ArrowLeft className="mr-2 h-5 w-5" />
          {t("runner.back")}
        </Button>
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-lg font-semibold text-text-primary">
            {runner.summary.template_name}
          </h2>
          <p className="text-sm text-text-muted">
            {t(`status.${status}`)} · {t("rounds.progress", { done: answered, total })}
          </p>
        </div>
      </div>

      <div
        role="status"
        data-testid="inspection-sync-banner"
        className={cn(
          "flex items-center gap-2 rounded-md border px-3 py-2 text-sm",
          sync.errors > 0
            ? "border-status-danger/50 bg-status-danger/10"
            : sync.waiting > 0
              ? "border-status-warning/50 bg-status-warning/10"
              : "border-status-success/40 bg-status-success/10",
        )}
      >
        {sync.errors > 0 ? (
          <AlertTriangle className="h-4 w-4 shrink-0" aria-hidden />
        ) : sync.waiting > 0 ? (
          <CloudOff className="h-4 w-4 shrink-0" aria-hidden />
        ) : (
          <CloudUpload className="h-4 w-4 shrink-0" aria-hidden />
        )}
        <span className="font-medium">{t("sync.title")}</span>
        <span>
          {sync.errors > 0
            ? t("sync.errors", { count: sync.errors })
            : sync.waiting > 0
              ? t("sync.waiting", { count: sync.waiting })
              : t("sync.allSynced")}
        </span>
      </div>

      {!open ? (
        <p className="rounded-md bg-surface-2 px-3 py-2 text-sm text-text-muted">
          {t("runner.readOnly", { status: t(`status.${status}`) })}
        </p>
      ) : null}

      <nav
        aria-label={t("runner.checkOf", { index: index + 1, total })}
        className="flex gap-2 overflow-x-auto pb-1"
      >
        {runner.checkpoints.map((c, i) => (
          <button
            key={c.checkpoint.id}
            type="button"
            onClick={() => setIndex(i)}
            aria-current={i === index ? "step" : undefined}
            title={c.checkpoint.description ?? c.checkpoint.checkpoint_code}
            className={cn(
              "min-h-12 min-w-12 shrink-0 rounded-md border-2 px-3 text-sm font-semibold",
              chipTone(c),
              i === index && "ring-2 ring-primary ring-offset-2",
            )}
          >
            {i + 1}
          </button>
        ))}
      </nav>

      {current && cp ? (
        <section
          className="space-y-4 rounded-lg border border-surface-border bg-surface-1 p-4"
          aria-live="polite"
        >
          <div className="space-y-1">
            <p className="text-xs uppercase tracking-wide text-text-muted">
              {t("runner.checkOf", { index: index + 1, total })} · {cp.checkpoint_code}
            </p>
            <h3 className="text-xl font-semibold text-text-primary">
              {cp.description ?? cp.checkpoint_code}
            </h3>
            {current.asset_code ? (
              <p className="text-sm text-text-muted">
                {t("runner.asset")} : <span className="font-mono">{current.asset_code}</span>{" "}
                {current.asset_name}
              </p>
            ) : null}
            <p className="text-xs text-text-muted" data-testid="checkpoint-sync-state">
              {t(`sync.state.${current.sync_state}`)}
            </p>
          </div>

          {isNumeric ? (
            <div className="space-y-2">
              {normalBand || warningBand ? (
                <p className="text-sm text-text-muted">
                  {t("runner.limits")} —{" "}
                  {normalBand ? `${t("runner.normal")} ${bandText(normalBand)}` : ""}
                  {normalBand && warningBand ? " · " : ""}
                  {warningBand ? `${t("runner.warning")} ${bandText(warningBand)}` : ""}
                  {cp.measurement_unit ? ` ${cp.measurement_unit}` : ""}
                </p>
              ) : null}
              <Label htmlFor="inspection-reading">{t("runner.reading")}</Label>
              <div className="flex items-center gap-2">
                <Input
                  id="inspection-reading"
                  inputMode="decimal"
                  autoComplete="off"
                  className="h-16 max-w-xs text-3xl"
                  placeholder={t("runner.readingPlaceholder")}
                  value={draft.reading}
                  disabled={!editable}
                  onChange={(e) =>
                    setDraft((d) => ({
                      ...d,
                      reading: e.target.value,
                      status: d.status != null && NOT_PERFORMED.has(d.status) ? null : d.status,
                    }))
                  }
                />
                {cp.measurement_unit ? (
                  <span className="text-xl text-text-muted">{cp.measurement_unit}</span>
                ) : null}
              </div>
              {draft.reading.trim() !== "" && reading == null ? (
                <p role="alert" className="text-sm text-status-danger">
                  {t("runner.invalidNumber")}
                </p>
              ) : null}
              {predicted && !notPerformed ? (
                <p className="text-sm font-medium" data-testid="predicted-status">
                  {t("runner.predicted", { status: t(`result.${predicted}`) })}
                </p>
              ) : null}
            </div>
          ) : null}

          {cp.check_type === "observation" ? (
            <div className="space-y-1">
              <Label htmlFor="inspection-observation">{t("runner.observation")}</Label>
              <Textarea
                id="inspection-observation"
                rows={3}
                className="text-base"
                value={draft.text}
                disabled={!editable}
                onChange={(e) => setDraft((d) => ({ ...d, text: e.target.value }))}
              />
            </div>
          ) : null}

          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4" role="radiogroup">
            {answerChoices(cp.check_type).map((s) => (
              <button
                key={s}
                type="button"
                role="radio"
                aria-checked={draft.status === s}
                disabled={!editable}
                onClick={() => setDraft((d) => ({ ...d, status: d.status === s ? null : s }))}
                className={cn(
                  "min-h-16 rounded-lg border-2 px-3 text-base font-semibold transition-colors disabled:opacity-60",
                  draft.status === s
                    ? STATUS_TONE[s]
                    : "border-surface-border bg-surface-0 text-text-primary",
                )}
              >
                {answerLabel(s)}
              </button>
            ))}
          </div>

          <div className="space-y-1">
            <Label htmlFor="inspection-comment">
              {effectiveStatus != null && NOT_PERFORMED.has(effectiveStatus)
                ? t("runner.reason")
                : t("runner.comment")}
            </Label>
            <Textarea
              id="inspection-comment"
              rows={2}
              className="text-base"
              placeholder={t("runner.commentPlaceholder")}
              value={draft.comment}
              disabled={!editable}
              onChange={(e) => setDraft((d) => ({ ...d, comment: e.target.value }))}
            />
            {commentMissing ? (
              <p role="alert" className="text-sm text-status-danger">
                {t("runner.commentRequired")}
              </p>
            ) : null}
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <input
              ref={fileRef}
              type="file"
              accept="image/*"
              capture="environment"
              className="hidden"
              data-testid="inspection-photo-input"
              onChange={(e) => void onPhoto(e.target.files?.[0])}
            />
            <Button
              variant="outline"
              size="lg"
              className="min-h-12"
              disabled={!editable || busy || !current.result}
              onClick={() => fileRef.current?.click()}
              title={current.result ? undefined : t("runner.recordFirst")}
            >
              <Camera className="mr-2 h-5 w-5" />
              {t("runner.photo")}
            </Button>
            {photoCount > 0 ? (
              <span className="text-sm text-text-muted">
                {t("runner.photos", { count: photoCount })}
              </span>
            ) : null}
            {cp.requires_photo && photoCount === 0 ? (
              <span className="text-sm text-status-warning">{t("runner.photoRequired")}</span>
            ) : null}
          </div>

          {current.missing.length > 0 && current.result ? (
            <div className="flex flex-wrap gap-2">
              {current.missing.map((m) => (
                <Badge
                  key={m}
                  variant="outline"
                  className="border-status-warning text-status-warning"
                >
                  {t(`runner.missing.${m}`)}
                </Badge>
              ))}
            </div>
          ) : null}
          {current.open_anomalies.map((a) => (
            <p key={a.id} className="flex items-center gap-2 text-sm text-status-danger">
              <AlertTriangle className="h-4 w-4" aria-hidden />
              {t("runner.anomalyOpen", {
                description: t(`review.anomalyType.${a.anomaly_type}`, {
                  defaultValue: a.description,
                }),
              })}
            </p>
          ))}

          {error ? (
            <p
              role="alert"
              className="rounded-md bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
            >
              {error}
            </p>
          ) : null}
          {notice ? (
            <p className="flex items-center gap-2 text-sm text-status-success">
              <CheckCircle2 className="h-4 w-4" aria-hidden />
              {notice}
            </p>
          ) : null}

          <div className="flex flex-wrap items-center justify-between gap-3 border-t border-surface-border pt-4">
            <Button
              variant="outline"
              size="lg"
              className="min-h-14 min-w-32"
              disabled={index === 0}
              onClick={() => setIndex(index - 1)}
            >
              <ChevronLeft className="mr-1 h-5 w-5" />
              {t("runner.previous")}
            </Button>
            <div className="flex flex-wrap gap-3">
              {index < total - 1 ? (
                <Button
                  variant="ghost"
                  size="lg"
                  className="min-h-14"
                  onClick={() => setIndex(index + 1)}
                >
                  {t("runner.next")}
                  <ChevronRight className="ml-1 h-5 w-5" />
                </Button>
              ) : null}
              {editable ? (
                <Button
                  size="lg"
                  className="min-h-14 min-w-48"
                  disabled={!readyToSave}
                  onClick={() => void save(true)}
                >
                  {busy ? <Loader2 className="mr-2 h-5 w-5 animate-spin" /> : null}
                  {index < total - 1 ? t("runner.saveAndNext") : t("runner.save")}
                </Button>
              ) : null}
            </div>
          </div>
        </section>
      ) : null}

      {open ? (
        <div className="flex flex-wrap items-center gap-3">
          <Button
            size="lg"
            className="min-h-14"
            disabled={!editable || busy || !completable}
            onClick={() => void complete()}
          >
            <CheckCircle2 className="mr-2 h-5 w-5" />
            {t("runner.complete")}
          </Button>
          {!completable ? (
            <span className="text-sm text-text-muted">{t("runner.completeHint")}</span>
          ) : null}
        </div>
      ) : null}

      {reviewable ? (
        <InspectionRoundReview
          runner={runner}
          onChanged={async () => {
            await load(true);
            onChanged?.();
          }}
        />
      ) : null}
    </div>
  );
}
