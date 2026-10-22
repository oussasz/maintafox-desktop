/**
 * Field runner for one inspection round (PRD §6.25): one check at a time, 44 px+ targets,
 * keyboard driven (arrows, digits, Enter), numeric keypad for readings, camera for photos,
 * the history and trend of each checkpoint, and the offline queue visible (results saved on
 * this device stay "waiting" until the server has accepted them).
 *
 * A reading outside its limits needs a comment and proposes a request written from the real
 * data (equipment, reading, limit, round), editable before creation.
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
  Keyboard,
  Loader2,
} from "lucide-react";
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { Button, Input, Label, Textarea } from "@/components/ui";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  addInspectionPhotoEvidence,
  completeInspectionRound,
  getInspectionRoundRunner,
  recordInspectionResult,
} from "@/services/inspection-service";
import type {
  InspectionCheckpointHistoryPoint,
  InspectionRoundRunner,
  InspectionRunnerCheckpoint,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import {
  type FollowUpCreated,
  FollowUpCreatedNotice,
  type FollowUpKind,
  InspectionFollowUpDialog,
} from "./InspectionFollowUpDialog";
import { InspectionRoundReview } from "./InspectionRoundReview";
import {
  MAX_PHOTO_MB,
  NOT_PERFORMED,
  RESULT_TONE,
  ROUND_OPEN_STATUSES,
  ROUND_STATUS_TONE,
  type ResultStatus,
  answerChoices,
  canComplete,
  describeBand,
  firstOpenIndex,
  parseReading,
  pendingSyncCount,
  predictNumericStatus,
  readingNeedsComment,
  sparklinePoints,
  violatedLimit,
} from "./inspection-rules";

interface InspectionRunnerProps {
  roundId: number;
  onClose: () => void;
  /** Called after any change so lists can refresh their counts. */
  onChanged?: () => void;
}

const STATUS_TONE: Record<string, string> = {
  pass: "border-tone-success-border bg-tone-success-bg text-text-primary",
  warning: "border-tone-warning-border bg-tone-warning-bg text-text-primary",
  fail: "border-tone-danger-border bg-tone-danger-bg text-text-primary",
  not_accessible: "border-tone-neutral-border bg-tone-neutral-bg text-text-primary",
  not_done: "border-tone-neutral-border bg-tone-neutral-bg text-text-primary",
};

function chipTone(c: InspectionRunnerCheckpoint): string {
  if (!c.result) return "border-surface-border bg-surface-1";
  if (c.missing.length > 0) return "border-tone-warning-border bg-tone-warning-bg";
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

/** Whether a key event comes from a field where typing must not trigger shortcuts. */
function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.tagName === "SELECT" ||
    target.isContentEditable
  );
}

export function InspectionRunner({ roundId, onClose, onChanged }: InspectionRunnerProps) {
  const { t } = useTranslation("inspections");
  const { formatNumber } = useFormatters();
  const { can, canAny } = usePermissions();
  const [runner, setRunner] = useState<InspectionRoundRunner | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [index, setIndex] = useState(0);
  const [draft, setDraft] = useState<Draft>(draftFrom(undefined));
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [followUp, setFollowUp] = useState<{ id: number; kind: FollowUpKind } | null>(null);
  const [created, setCreated] = useState<FollowUpCreated | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const readingRef = useRef<HTMLInputElement>(null);

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
        setLoadError(null);
      } catch (err) {
        setLoadError(err);
      }
    },
    [roundId],
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
    return loadError != null ? (
      <ErrorState error={loadError} onRetry={() => void load(false)} />
    ) : (
      <LoadingState />
    );
  }

  const status = runner.summary.round.status;
  const open = ROUND_OPEN_STATUSES.has(status);
  const editable = open && can(P.INS_EXECUTE);
  const canDi = can(P.INS_EXECUTE) && canAny(P.DI_CREATE, P.DI_CREATE_OWN);
  const total = runner.checkpoints.length;
  const answered = runner.checkpoints.filter((c) => c.result).length;
  const cp = current?.checkpoint;
  const isNumeric = cp?.check_type === "numeric";
  const reading = parseReading(draft.reading);
  const predicted = isNumeric && reading != null && cp ? predictNumericStatus(reading, cp) : null;
  const violation = isNumeric && reading != null && cp ? violatedLimit(reading, cp) : null;
  const effectiveStatus: string | null = draft.status ?? predicted;
  const notPerformed = draft.status != null && NOT_PERFORMED.has(draft.status);
  const commentRequired =
    effectiveStatus != null &&
    cp != null &&
    (NOT_PERFORMED.has(effectiveStatus) ||
      readingNeedsComment(cp.check_type, effectiveStatus) ||
      (cp.requires_comment_on_exception && effectiveStatus !== "pass"));
  const commentMissing = commentRequired && draft.comment.trim() === "";
  const hasAnswer = isNumeric ? reading != null || notPerformed : draft.status != null;
  const readyToSave = editable && !busy && !commentMissing && hasAnswer;
  const choices = cp ? answerChoices(cp.check_type) : [];
  const unit = cp?.measurement_unit ? ` ${cp.measurement_unit}` : "";

  const goTo = (i: number) => {
    if (i < 0 || i >= total) return;
    setIndex(i);
    setNotice(null);
  };

  const save = async (advance: boolean) => {
    if (!cp || !readyToSave) return;
    setBusy(true);
    setNotice(null);
    setCreated(null);
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
      // Out of tolerance: stay on the check so the follow-up proposal is seen.
      const outOfTolerance = predicted === "warning" || predicted === "fail";
      if (advance && !outOfTolerance && index < total - 1) setIndex(index + 1);
    } catch {
      // Reported by the global IPC toast (gate codes are translated).
    } finally {
      setBusy(false);
    }
  };

  const onPhoto = async (file: File | undefined) => {
    if (!file || !current?.result) return;
    if (file.size > MAX_PHOTO_MB * 1024 * 1024) {
      setNotice(t("runner.photoTooLarge", { mb: MAX_PHOTO_MB }));
      return;
    }
    setBusy(true);
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
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  const complete = async () => {
    setBusy(true);
    try {
      await completeInspectionRound({
        round_id: runner.summary.round.id,
        expected_row_version: runner.summary.round.row_version,
      });
      await load(true);
      onChanged?.();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setBusy(false);
    }
  };

  /** Shortcuts outside text fields: ←/→ checks, 1…n answers, P photo. */
  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.altKey || e.ctrlKey || e.metaKey || isTyping(e.target)) return;
    if (e.key === "ArrowRight") {
      e.preventDefault();
      goTo(index + 1);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      goTo(index - 1);
    } else if (/^[1-9]$/.test(e.key) && editable) {
      const choice = choices[Number(e.key) - 1];
      if (choice) {
        e.preventDefault();
        setDraft((d) => ({ ...d, status: d.status === choice ? null : choice }));
      }
    } else if ((e.key === "p" || e.key === "P") && editable && current?.result) {
      e.preventDefault();
      fileRef.current?.click();
    }
  };

  /** Arrow keys move inside the answer group (one tab stop, WAI-ARIA radio group). */
  const onAnswerKey = (e: ReactKeyboardEvent<HTMLButtonElement>, i: number) => {
    const delta =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (delta === 0) return;
    e.preventDefault();
    e.stopPropagation();
    const next = (i + delta + choices.length) % choices.length;
    const group = e.currentTarget.parentElement;
    group?.querySelectorAll<HTMLButtonElement>("[role=radio]")[next]?.focus();
  };

  const photoCount = current?.evidence.filter((e) => e.evidence_type === "photo").length ?? 0;
  const normalBand = cp ? describeBand(cp.normal_min, cp.normal_max) : null;
  const warningBand = cp ? describeBand(cp.warning_min, cp.warning_max) : null;
  const bandText = (b: ReturnType<typeof describeBand>) => {
    if (b == null) return "";
    if (b.kind === "between" && b.min != null && b.max != null) {
      return t("runner.between", { min: formatNumber(b.min), max: formatNumber(b.max) });
    }
    if (b.kind === "below" && b.max != null)
      return t("runner.below", { value: formatNumber(b.max) });
    if (b.kind === "above" && b.min != null)
      return t("runner.above", { value: formatNumber(b.min) });
    return "";
  };
  const answerLabel = (s: ResultStatus) =>
    cp?.check_type === "boolean" && (s === "pass" || s === "fail")
      ? t(s === "pass" ? "runner.answers.yes" : "runner.answers.no")
      : t(`runner.answers.${s}`);
  const reviewable =
    status === "completed_with_findings" ||
    (status === "completed" && runner.summary.requires_review);
  const completable = canComplete(runner.checkpoints);
  const checkedIndex = Math.max(
    0,
    choices.findIndex((s) => s === draft.status),
  );
  const progressPct = total > 0 ? Math.round((100 * answered) / total) : 0;

  return (
    <div
      className="flex flex-col gap-4 outline-none"
      data-testid="inspection-runner"
      onKeyDown={onKeyDown}
    >
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="outline" className="h-12" onClick={onClose}>
          <ArrowLeft className="mr-2 h-5 w-5" aria-hidden />
          {t("runner.back")}
        </Button>
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-lg font-semibold text-text-primary">
            {runner.summary.template_name}
          </h2>
          <div className="flex flex-wrap items-center gap-2 text-sm text-text-muted">
            <StatusChip label={t(`status.${status}`)} tone={toneOf(ROUND_STATUS_TONE, status)} />
            {runner.summary.is_overdue ? (
              <StatusChip label={t("rounds.overdue")} tone="danger" />
            ) : null}
            {runner.summary.assigned_to_name ? (
              <span>{t("rounds.assignedTo", { name: runner.summary.assigned_to_name })}</span>
            ) : null}
            {runner.summary.assigned_team_name ? (
              <span>{t("rounds.team", { name: runner.summary.assigned_team_name })}</span>
            ) : null}
          </div>
        </div>
      </div>

      <div className="space-y-1">
        <div className="flex items-center justify-between text-sm">
          <span className="font-medium text-text-primary" data-testid="runner-progress">
            {t("runner.progressOf", { done: answered, total })}
          </span>
          <span className="text-text-muted">{progressPct} %</span>
        </div>
        <div
          className="h-2 overflow-hidden rounded bg-surface-3"
          role="progressbar"
          aria-label={t("runner.progressOf", { done: answered, total })}
          aria-valuemin={0}
          aria-valuemax={total}
          aria-valuenow={answered}
        >
          <div className="h-full bg-primary transition-all" style={{ width: `${progressPct}%` }} />
        </div>
      </div>

      <div
        role="status"
        data-testid="inspection-sync-banner"
        className={cn(
          "flex items-center gap-2 rounded-md border px-3 py-2 text-sm",
          sync.errors > 0
            ? "border-tone-danger-border bg-tone-danger-bg"
            : sync.waiting > 0
              ? "border-tone-warning-border bg-tone-warning-bg"
              : "border-tone-success-border bg-tone-success-bg",
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

      <nav aria-label={t("runner.checkpoints")} className="flex gap-2 overflow-x-auto pb-1">
        {runner.checkpoints.map((c, i) => (
          <button
            key={c.checkpoint.id}
            type="button"
            onClick={() => goTo(i)}
            aria-current={i === index ? "step" : undefined}
            aria-label={t("runner.checkChip", {
              index: i + 1,
              label: c.checkpoint.description ?? c.checkpoint.checkpoint_code,
              state: c.result ? t(`result.${c.result.result_status}`) : t("runner.missing.result"),
            })}
            title={c.checkpoint.description ?? c.checkpoint.checkpoint_code}
            className={cn(
              "min-h-12 min-w-12 shrink-0 rounded-md border-2 px-3 text-sm font-semibold focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
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
          aria-label={t("runner.checkOf", { index: index + 1, total })}
        >
          <div className="space-y-1" aria-live="polite">
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
                  {unit}
                </p>
              ) : null}
              <Label htmlFor="inspection-reading">{t("runner.reading")}</Label>
              <div className="flex items-center gap-2">
                <Input
                  ref={readingRef}
                  id="inspection-reading"
                  inputMode="decimal"
                  autoComplete="off"
                  className="h-16 max-w-xs text-3xl"
                  placeholder={t("runner.readingPlaceholder")}
                  value={draft.reading}
                  disabled={!editable}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      void save(true);
                    }
                  }}
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
                <p role="alert" className="text-sm text-tone-danger-fg">
                  {t("runner.invalidNumber")}
                </p>
              ) : null}
              {predicted && !notPerformed ? (
                <p
                  className="flex flex-wrap items-center gap-2 text-sm font-medium"
                  data-testid="predicted-status"
                >
                  {t("runner.predicted", { status: t(`result.${predicted}`) })}
                  {violation && reading != null ? (
                    <span className="font-mono text-tone-danger-fg" data-testid="violation">
                      {formatNumber(reading)}
                      {unit} {violation.direction === "above" ? ">" : "<"}{" "}
                      {formatNumber(violation.limit)}
                      {unit}
                    </span>
                  ) : null}
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

          <div
            className="grid grid-cols-2 gap-3 sm:grid-cols-4"
            role="radiogroup"
            aria-label={t("runner.answer")}
          >
            {choices.map((s, i) => (
              <button
                key={s}
                type="button"
                role="radio"
                aria-checked={draft.status === s}
                aria-keyshortcuts={String(i + 1)}
                tabIndex={i === checkedIndex ? 0 : -1}
                disabled={!editable}
                onKeyDown={(e) => onAnswerKey(e, i)}
                onClick={() => setDraft((d) => ({ ...d, status: d.status === s ? null : s }))}
                className={cn(
                  "min-h-16 rounded-lg border-2 px-3 text-base font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60",
                  draft.status === s
                    ? STATUS_TONE[s]
                    : "border-surface-border bg-surface-0 text-text-primary",
                )}
              >
                <span className="mr-1.5 text-xs text-text-muted" aria-hidden>
                  {i + 1}
                </span>
                {answerLabel(s)}
              </button>
            ))}
          </div>

          <div className="space-y-1">
            <Label htmlFor="inspection-comment">
              {effectiveStatus != null && NOT_PERFORMED.has(effectiveStatus)
                ? t("runner.reason")
                : commentRequired
                  ? t("runner.commentRequiredLabel")
                  : t("runner.comment")}
            </Label>
            <Textarea
              id="inspection-comment"
              rows={2}
              className="text-base"
              placeholder={t("runner.commentPlaceholder")}
              value={draft.comment}
              disabled={!editable}
              aria-required={commentRequired}
              aria-invalid={commentMissing}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                  e.preventDefault();
                  void save(true);
                }
              }}
              onChange={(e) => setDraft((d) => ({ ...d, comment: e.target.value }))}
            />
            {commentMissing ? (
              <p role="alert" className="text-sm text-tone-danger-fg">
                {readingNeedsComment(cp.check_type, effectiveStatus)
                  ? t("runner.commentRequiredOutOfTolerance")
                  : t("runner.commentRequired")}
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
              className="h-12"
              disabled={!editable || busy || !current.result}
              onClick={() => fileRef.current?.click()}
              aria-keyshortcuts="P"
            >
              <Camera className="mr-2 h-5 w-5" aria-hidden />
              {t("runner.photo")}
            </Button>
            {!current.result && editable ? (
              <span className="text-sm text-text-muted">{t("runner.recordFirst")}</span>
            ) : null}
            {photoCount > 0 ? (
              <span className="text-sm text-text-muted">
                {t("runner.photos", { count: photoCount })}
              </span>
            ) : null}
            {cp.requires_photo && photoCount === 0 ? (
              <span className="text-sm text-tone-warning-fg">{t("runner.photoRequired")}</span>
            ) : null}
          </div>

          {current.missing.length > 0 && current.result ? (
            <div className="flex flex-wrap gap-2">
              {current.missing.map((m) => (
                <StatusChip key={m} label={t(`runner.missing.${m}`)} tone="warning" />
              ))}
            </div>
          ) : null}

          {current.open_anomalies.map((a) => (
            <div
              key={a.id}
              className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-tone-attention-border bg-tone-attention-bg px-3 py-2 text-sm text-tone-attention-fg"
              data-testid="runner-anomaly"
            >
              <span className="flex items-center gap-2">
                <AlertTriangle className="h-4 w-4" aria-hidden />
                {t("runner.anomalyOpen", {
                  description: t(`review.anomalyType.${a.anomaly_type}`, {
                    defaultValue: t("anomalies.otherType"),
                  }),
                })}
              </span>
              {canDi && current.asset_code ? (
                <Button
                  size="sm"
                  className="h-11"
                  onClick={() => {
                    setCreated(null);
                    setFollowUp({ id: a.id, kind: "di" });
                  }}
                >
                  {t("runner.proposeDi")}
                </Button>
              ) : null}
            </div>
          ))}
          {created ? <FollowUpCreatedNotice created={created} /> : null}

          <CheckpointHistory
            history={current.history}
            numeric={isNumeric}
            unit={cp.measurement_unit}
          />

          {notice ? (
            <p role="status" className="flex items-center gap-2 text-sm text-tone-success-fg">
              <CheckCircle2 className="h-4 w-4" aria-hidden />
              {notice}
            </p>
          ) : null}

          <div className="flex flex-wrap items-center justify-between gap-3 border-t border-surface-border pt-4">
            <Button
              variant="outline"
              className="h-14 min-w-32"
              disabled={index === 0}
              onClick={() => goTo(index - 1)}
              aria-keyshortcuts="ArrowLeft"
            >
              <ChevronLeft className="mr-1 h-5 w-5" aria-hidden />
              {t("runner.previous")}
            </Button>
            <div className="flex flex-wrap gap-3">
              {index < total - 1 ? (
                <Button
                  variant="ghost"
                  className="h-14"
                  onClick={() => goTo(index + 1)}
                  aria-keyshortcuts="ArrowRight"
                >
                  {t("runner.next")}
                  <ChevronRight className="ml-1 h-5 w-5" aria-hidden />
                </Button>
              ) : null}
              {editable ? (
                <Button
                  className="h-14 min-w-48"
                  disabled={!readyToSave}
                  onClick={() => void save(true)}
                >
                  {busy ? <Loader2 className="mr-2 h-5 w-5 animate-spin" aria-hidden /> : null}
                  {index < total - 1 ? t("runner.saveAndNext") : t("runner.save")}
                </Button>
              ) : null}
            </div>
          </div>
          {editable ? (
            <p className="flex items-center gap-2 text-xs text-text-muted">
              <Keyboard className="h-3.5 w-3.5" aria-hidden />
              {t("runner.shortcuts")}
            </p>
          ) : null}
        </section>
      ) : null}

      {open ? (
        <div className="flex flex-wrap items-center gap-3">
          <Button
            className="h-14"
            disabled={!editable || busy || !completable}
            onClick={() => void complete()}
          >
            <CheckCircle2 className="mr-2 h-5 w-5" aria-hidden />
            {t("runner.complete")}
          </Button>
          {!completable ? (
            <span className="text-sm text-text-muted">{t("runner.completeHint")}</span>
          ) : null}
        </div>
      ) : null}

      {reviewable || status === "reviewed" ? (
        <InspectionRoundReview
          runner={runner}
          onChanged={async () => {
            await load(true);
            onChanged?.();
          }}
        />
      ) : null}

      <InspectionFollowUpDialog
        anomalyId={followUp?.id ?? null}
        kind={followUp?.kind ?? "di"}
        onClose={() => setFollowUp(null)}
        onCreated={(c) => {
          setFollowUp(null);
          setCreated(c);
          void load(true);
          onChanged?.();
        }}
      />
    </div>
  );
}

interface CheckpointHistoryProps {
  history: InspectionCheckpointHistoryPoint[];
  numeric: boolean;
  unit: string | null;
}

/** Earlier results of the checkpoint (newest first) and, for readings, their trend. */
function CheckpointHistory({ history, numeric, unit }: CheckpointHistoryProps) {
  const { t } = useTranslation("inspections");
  const { formatNumber, formatDate } = useFormatters();
  if (history.length === 0) {
    return <p className="text-xs text-text-muted">{t("runner.historyNone")}</p>;
  }
  const values = numeric
    ? history
        .map((h) => h.numeric_value)
        .filter((v): v is number => v != null)
        .reverse()
    : [];
  return (
    <section
      aria-label={t("runner.history")}
      className="space-y-2"
      data-testid="checkpoint-history"
    >
      <h4 className="text-sm font-semibold text-text-primary">{t("runner.history")}</h4>
      {values.length >= 2 ? (
        <svg
          viewBox="0 0 160 36"
          className="h-9 w-40 text-primary"
          role="img"
          aria-label={t("runner.trend", {
            first: formatNumber(values[0] ?? 0),
            last: formatNumber(values[values.length - 1] ?? 0),
            unit: unit ?? "",
          })}
        >
          <polyline
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            points={sparklinePoints(values, 156, 32)}
            transform="translate(2,2)"
          />
        </svg>
      ) : null}
      <ul className="flex flex-wrap gap-2">
        {history.map((h) => (
          <li
            key={`${h.round_id}-${h.recorded_at}`}
            className="flex items-center gap-1.5 rounded-md border border-surface-border px-2 py-1 text-xs"
            {...(h.comment ? { title: h.comment } : {})}
          >
            <span className="text-text-muted">{formatDate(h.recorded_at)}</span>
            {h.numeric_value != null ? (
              <span className="font-mono">
                {formatNumber(h.numeric_value)}
                {unit ? ` ${unit}` : ""}
              </span>
            ) : null}
            <StatusChip
              label={t(`result.${h.result_status}`)}
              tone={toneOf(RESULT_TONE, h.result_status)}
            />
          </li>
        ))}
      </ul>
    </section>
  );
}
