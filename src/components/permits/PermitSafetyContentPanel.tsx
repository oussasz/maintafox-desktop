/**
 * What a work permit says before it is issued (L4-03): the work, its risks, the preventive
 * measures and the PPE; the HSE approval its type may require; and its gas tests (L4-04) against
 * the limits of the settings. The server judges the issue (`permit::safety_content`); this panel
 * shows its verdict (`issue_blockers`) before the click, and never decides on its own.
 */

import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, FormField, Input, Textarea } from "@/components/ui";
import { ErrorState } from "@/components/ui/data-state";
import { DateTimeField } from "@/components/ui/date-time-field";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import { appErrorReason } from "@/lib/app-error";
import {
  type IssueBlocker,
  type PermitSafetyContent,
  recordPermitGasTest,
  recordPermitHseApproval,
  savePermitSafetyContent,
} from "@/services/permit-safety-service";
import type { WorkPermitRecord } from "@/services/permit-service";

import { localInputToIso, toLocalInputValue } from "./permit-rules";

/** Statuses in which the content is written (`permit::safety_content::CONTENT_EDIT_STATUSES`). */
const CONTENT_EDIT_STATUSES: ReadonlySet<string> = new Set(["draft", "pending_review"]);
/** Statuses in which the HSE approval is recorded (`HSE_APPROVAL_STATUSES`). */
const HSE_APPROVAL_STATUSES: ReadonlySet<string> = new Set(["pending_review", "approved"]);
/** Statuses in which a gas test is recorded (`GAS_TEST_STATUSES`). */
const GAS_TEST_STATUSES: ReadonlySet<string> = new Set([
  "draft",
  "pending_review",
  "approved",
  "issued",
  "active",
  "suspended",
  "revalidation_required",
]);

const CONTENT_FIELDS = [
  "work_description",
  "risk_assessment",
  "preventive_measures",
  "ppe_required",
] as const;
type ContentField = (typeof CONTENT_FIELDS)[number];
type ContentDraft = Record<ContentField, string>;

function draftOf(c: PermitSafetyContent): ContentDraft {
  return {
    work_description: c.work_description ?? "",
    risk_assessment: c.risk_assessment ?? "",
    preventive_measures: c.preventive_measures ?? "",
    ppe_required: c.ppe_required ?? "",
  };
}

/** null when empty, NaN when not a number (a comma is read as the decimal mark). */
function parseReading(raw: string): number | null {
  const v = raw.trim();
  if (v === "") return null;
  const n = Number(v.replace(",", "."));
  return Number.isFinite(n) ? n : Number.NaN;
}

const EMPTY_GAS = { o2: "", lel: "", h2s: "", co: "", at: "", instrument: "" };

interface PermitSafetyContentPanelProps {
  permit: WorkPermitRecord;
  /** The content as the server returned it; null while it loads. */
  content: PermitSafetyContent | null;
  loadError: unknown;
  onRetry: () => void;
  canIssue: boolean;
  /** `ptw.hse_approve` at the permit's site (the server also refuses the author and submitter). */
  canHseApprove: boolean;
  /** Called with the new content after a write (the permit itself changed when the content did). */
  onSaved: (content: PermitSafetyContent) => void;
}

/** A blocker of the issue, translated from its `GATE_` code. */
export function useIssueBlockerText(): (b: IssueBlocker) => string {
  const { t } = useTranslation("errors");
  return (b) => t(`gateError.${b.code}` as never, b.params as never) as unknown as string;
}

export function PermitSafetyContentPanel({
  permit,
  content,
  loadError,
  onRetry,
  canIssue,
  canHseApprove,
  onSaved,
}: PermitSafetyContentPanelProps) {
  const { t } = useTranslation("permits");
  const blockerText = useIssueBlockerText();
  const { formatDecimal, formatDateTime } = useFormatters();
  const [draft, setDraft] = useState<ContentDraft | null>(null);
  const [hseNote, setHseNote] = useState("");
  const [hseOpen, setHseOpen] = useState(false);
  const [gasDraft, setGasDraft] = useState(EMPTY_GAS);
  const [gasSubmitted, setGasSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(null);
    setError(null);
  }, [permit.id, permit.row_version]);

  if (loadError != null) {
    return (
      <ErrorState
        error={loadError}
        title={t("safety.loadError")}
        onRetry={onRetry}
        retrying={false}
      />
    );
  }
  if (content == null) {
    return (
      <p className="text-sm text-text-muted" aria-busy="true">
        {t("safety.loading")}
      </p>
    );
  }

  const status = permit.status;
  const contentEditable = canIssue && CONTENT_EDIT_STATUSES.has(status);
  const editing = draft ?? draftOf(content);

  const run = async (fn: () => Promise<PermitSafetyContent>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      onSaved(await fn());
      return true;
    } catch (e) {
      setError(appErrorReason(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const handleSaveContent = async (e: FormEvent) => {
    e.preventDefault();
    if (draft == null) return;
    const ok = await run(() =>
      savePermitSafetyContent({
        permit_id: permit.id,
        expected_row_version: permit.row_version,
        ...draft,
      }),
    );
    if (ok) setDraft(null);
  };

  const handleHse = async (e: FormEvent) => {
    e.preventDefault();
    const ok = await run(() =>
      recordPermitHseApproval({
        permit_id: permit.id,
        expected_row_version: permit.row_version,
        note: hseNote.trim() || null,
      }),
    );
    if (ok) {
      setHseOpen(false);
      setHseNote("");
    }
  };

  const o2 = parseReading(gasDraft.o2);
  const lel = parseReading(gasDraft.lel);
  const h2s = parseReading(gasDraft.h2s);
  const co = parseReading(gasDraft.co);
  const measuredAt = localInputToIso(gasDraft.at);
  const invalidReading = (v: number | null, required: boolean) =>
    (required && v == null) || (v != null && (Number.isNaN(v) || v < 0));
  const gasReady =
    !invalidReading(o2, true) &&
    !invalidReading(lel, true) &&
    !invalidReading(h2s, true) &&
    !invalidReading(co, false) &&
    measuredAt != null &&
    gasDraft.instrument.trim() !== "";

  const handleGas = async (e: FormEvent) => {
    e.preventDefault();
    setGasSubmitted(true);
    if (!gasReady || o2 == null || lel == null || h2s == null || measuredAt == null) return;
    const ok = await run(() =>
      recordPermitGasTest({
        permit_id: permit.id,
        o2_percent: o2,
        lel_percent: lel,
        h2s_ppm: h2s,
        co_ppm: co,
        measured_at: measuredAt,
        instrument: gasDraft.instrument.trim(),
      }),
    );
    if (ok) {
      setGasDraft(EMPTY_GAS);
      setGasSubmitted(false);
    }
  };

  const limits = content.gas_limits;
  /** A limit as the settings hold it (no forced decimal: "10", "19,5"). */
  const limit = (v: number) => formatDecimal(v, Number.isInteger(v) ? 0 : 1);
  const showGas = content.requires_gas_test || content.gas_tests.length > 0;

  return (
    <section className="space-y-3" data-testid="permit-safety-content">
      <h3 className="text-sm font-medium text-text-primary">{t("safety.title")}</h3>

      {content.issue_blockers.length > 0 &&
      ["approved", "pending_review", "draft"].includes(status) ? (
        <div
          role="status"
          data-testid="permit-issue-blockers"
          className="rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-xs text-tone-warning-fg"
        >
          <p className="font-medium">{t("safety.issueBlockersTitle")}</p>
          <ul className="list-disc pl-4">
            {content.issue_blockers.map((b) => (
              <li key={`${b.code}-${JSON.stringify(b.params)}`}>{blockerText(b)}</li>
            ))}
          </ul>
        </div>
      ) : null}

      {error ? (
        <p
          role="alert"
          className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
        >
          {error}
        </p>
      ) : null}

      {contentEditable ? (
        <form className="space-y-2" onSubmit={(e) => void handleSaveContent(e)}>
          {CONTENT_FIELDS.map((f) => (
            <FormField key={f} name={`safety-${f}`} label={t(`safety.field.${f}`)} required>
              <Textarea
                id={`safety-${f}`}
                rows={2}
                value={editing[f]}
                onChange={(e) => setDraft({ ...editing, [f]: e.target.value })}
              />
            </FormField>
          ))}
          <p className="text-xs text-text-muted">{t("safety.ppeHint")}</p>
          {status === "pending_review" ? (
            <p className="text-xs text-tone-warning-fg">{t("safety.backToDraftHint")}</p>
          ) : null}
          <Button type="submit" size="sm" disabled={busy || draft == null}>
            {t("safety.saveContent")}
          </Button>
        </form>
      ) : (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
          {CONTENT_FIELDS.map((f) => (
            <div key={f} className="contents">
              <dt className="text-text-muted">{t(`safety.field.${f}`)}</dt>
              <dd className="whitespace-pre-wrap" data-testid={`permit-safety-${f}`}>
                {content[f] ?? <span className="text-text-muted">{t("safety.notFilled")}</span>}
              </dd>
            </div>
          ))}
        </dl>
      )}

      {content.requires_hse_approval ? (
        <div className="space-y-1 rounded-md border border-surface-border p-2 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{t("safety.hse.title")}</span>
            {content.hse_approved_at ? (
              <StatusChip label={t("safety.hse.recorded")} tone="success" />
            ) : (
              <StatusChip label={t("safety.hse.missing")} tone="warning" />
            )}
          </div>
          {content.hse_approved_at ? (
            <p className="text-xs text-text-muted" data-testid="permit-hse-approval">
              {t("safety.hse.byAt", {
                name: content.hse_approved_by_name ?? t("detail.unknownPerson"),
                date: formatDateTime(content.hse_approved_at),
              })}
              {content.hse_approval_note ? ` — ${content.hse_approval_note}` : ""}
              {content.hse_self_approved === true ? ` · ${t("safety.hse.selfApproved")}` : ""}
            </p>
          ) : canHseApprove && HSE_APPROVAL_STATUSES.has(status) ? (
            hseOpen ? (
              <form className="space-y-2" onSubmit={(e) => void handleHse(e)}>
                <FormField name="hse-note" label={t("safety.hse.note")}>
                  <Textarea
                    id="hse-note"
                    rows={2}
                    value={hseNote}
                    onChange={(e) => setHseNote(e.target.value)}
                  />
                </FormField>
                <div className="flex gap-2">
                  <Button type="submit" size="sm" disabled={busy}>
                    {t("safety.hse.confirm")}
                  </Button>
                  <Button type="button" size="sm" variant="ghost" onClick={() => setHseOpen(false)}>
                    {t("actions.cancel")}
                  </Button>
                </div>
              </form>
            ) : (
              <Button size="sm" variant="outline" disabled={busy} onClick={() => setHseOpen(true)}>
                {t("safety.hse.record")}
              </Button>
            )
          ) : null}
        </div>
      ) : null}

      {showGas ? (
        <div className="space-y-2 rounded-md border border-surface-border p-2 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{t("safety.gas.title")}</span>
            {content.requires_gas_test ? (
              <StatusChip label={t("safety.gas.required")} tone="attention" />
            ) : null}
          </div>
          <p className="text-xs text-text-muted" data-testid="permit-gas-limits">
            {t("safety.gas.limits", {
              o2Min: limit(limits.o2_min_percent),
              o2Max: limit(limits.o2_max_percent),
              lel: limit(limits.lel_max_percent),
              h2s: limit(limits.h2s_max_ppm),
              co: limit(limits.co_max_ppm),
            })}{" "}
            {t("safety.gas.validity", { hours: limit(content.gas_validity_hours) })}
          </p>
          {content.gas_tests.length === 0 ? (
            <p className="text-xs text-text-muted">{t("safety.gas.empty")}</p>
          ) : (
            <ul className="space-y-1">
              {content.gas_tests.map((g) => (
                <li
                  key={g.id}
                  data-testid="permit-gas-test"
                  className="flex flex-wrap items-center gap-2 text-xs"
                >
                  <StatusChip
                    label={g.within_limits ? t("safety.gas.ok") : t("safety.gas.outOfLimits")}
                    tone={g.within_limits ? "success" : "danger"}
                  />
                  {g.counts ? null : (
                    <StatusChip label={t("safety.gas.notCounting")} tone="neutral" />
                  )}
                  <span>
                    {t("safety.gas.reading", {
                      o2: formatDecimal(g.o2_percent, 1),
                      lel: formatDecimal(g.lel_percent, 1),
                      h2s: formatDecimal(g.h2s_ppm, 1),
                    })}
                    {g.co_ppm != null
                      ? ` · ${t("safety.gas.readingCo", { co: formatDecimal(g.co_ppm, 1) })}`
                      : ""}
                  </span>
                  <span className="text-text-muted">
                    {t("safety.gas.measuredBy", {
                      name: g.measured_by_name ?? t("detail.unknownPerson"),
                      date: formatDateTime(g.measured_at),
                      instrument: g.instrument,
                    })}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {canIssue && GAS_TEST_STATUSES.has(status) ? (
            <form className="grid gap-2 sm:grid-cols-3" onSubmit={(e) => void handleGas(e)}>
              {(
                [
                  ["o2", "safety.gas.o2", true],
                  ["lel", "safety.gas.lel", true],
                  ["h2s", "safety.gas.h2s", true],
                  ["co", "safety.gas.co", false],
                ] as const
              ).map(([key, label, required]) => (
                <FormField
                  key={key}
                  name={`gas-${key}`}
                  label={t(label)}
                  required={required}
                  error={
                    gasSubmitted && invalidReading(parseReading(gasDraft[key]), required)
                      ? t("safety.gas.invalidReading")
                      : undefined
                  }
                >
                  <Input
                    id={`gas-${key}`}
                    inputMode="decimal"
                    value={gasDraft[key]}
                    onChange={(e) => setGasDraft((d) => ({ ...d, [key]: e.target.value }))}
                  />
                </FormField>
              ))}
              <FormField
                name="gas-at"
                label={t("safety.gas.measuredAt")}
                required
                error={gasSubmitted && measuredAt == null ? t("errors.required") : undefined}
              >
                <DateTimeField
                  aria-label={t("safety.gas.measuredAt")}
                  value={gasDraft.at}
                  onChange={(v) => setGasDraft((d) => ({ ...d, at: v }))}
                />
              </FormField>
              <FormField
                name="gas-instrument"
                label={t("safety.gas.instrument")}
                required
                error={
                  gasSubmitted && gasDraft.instrument.trim() === ""
                    ? t("errors.required")
                    : undefined
                }
              >
                <Input
                  id="gas-instrument"
                  value={gasDraft.instrument}
                  onChange={(e) => setGasDraft((d) => ({ ...d, instrument: e.target.value }))}
                />
              </FormField>
              <div className="flex flex-wrap items-end gap-2 sm:col-span-3">
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => setGasDraft((d) => ({ ...d, at: toLocalInputValue(new Date()) }))}
                >
                  {t("safety.gas.now")}
                </Button>
                <Button type="submit" size="sm" disabled={busy}>
                  {t("safety.gas.record")}
                </Button>
              </div>
            </form>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
