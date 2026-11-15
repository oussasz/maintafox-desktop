/**
 * "Pannes répétitives" tab of the asset record: whether the equipment is flagged (unplanned
 * failures within the window, both from Paramètres > Fiabilité), its "5 pourquoi" analyses and
 * the tracked corrective actions (action, owner, due date, status).
 */

import { AlertTriangle, ListChecks, Plus, Search, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailSectionCard } from "@/components/detail";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import { Input } from "@/components/ui/input";
import { ListAccessNote } from "@/components/ui/list-access-note";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { appErrorText } from "@/lib/app-error";
import { useCanReadList } from "@/lib/list-access";
import {
  getAssetRepeatFailureStatus,
  listAssetCorrectiveActions,
  listAssetFiveWhys,
  saveAssetCorrectiveAction,
  saveAssetFiveWhy,
} from "@/services/asset-follow-up-service";
import { listPersonnel } from "@/services/personnel-service";
import { pushAppToast } from "@/store/app-toast-store";
import type {
  AssetRepeatFailureStatus,
  CorrectiveAction,
  CorrectiveActionStatus,
  FiveWhyAnalysis,
  Personnel,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const ACTION_STATUSES: CorrectiveActionStatus[] = ["open", "done", "cancelled"];
const WHY_KEYS = ["why_1", "why_2", "why_3", "why_4", "why_5"] as const;

interface FollowUp {
  status: AssetRepeatFailureStatus;
  whys: FiveWhyAnalysis[];
  actions: CorrectiveAction[];
}

export function AssetRepeatFailureTab({ assetId }: { assetId: number }) {
  const { t } = useTranslation("equipment");
  const load = useMemo(
    () => async (): Promise<FollowUp> => {
      const [status, whys, actions] = await Promise.all([
        getAssetRepeatFailureStatus(assetId),
        listAssetFiveWhys(assetId),
        listAssetCorrectiveActions(assetId),
      ]);
      return { status, whys, actions };
    },
    [assetId],
  );
  const { data, error, loading, reload } = useLoad<FollowUp>(load, String(assetId));
  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  const { status } = data;
  const vars = {
    count: status.failure_count,
    days: status.rule.window_days,
    min: status.rule.min_failures,
  };
  return (
    <div className="space-y-4">
      <div
        role="status"
        className={
          status.is_repeat_failure
            ? "flex items-start gap-2 rounded-md border border-status-danger/40 bg-status-danger/10 p-3 text-sm"
            : "flex items-start gap-2 rounded-md bg-surface-2 p-3 text-sm"
        }
      >
        {status.is_repeat_failure ? (
          <AlertTriangle className="mt-0.5 h-4 w-4 text-tone-danger-fg" aria-hidden />
        ) : null}
        <div>
          <p className={status.is_repeat_failure ? "font-medium" : undefined}>
            {status.is_repeat_failure
              ? t("repeatFailures.status.flagged", vars)
              : t("repeatFailures.status.notFlagged", vars)}
          </p>
          <p className="text-xs text-text-muted">
            {t("repeatFailures.status.actions", {
              open: status.open_action_count,
              overdue: status.overdue_action_count,
            })}
          </p>
        </div>
      </div>
      <FiveWhySection assetId={assetId} whys={data.whys} onSaved={() => void reload()} />
      <ActionsSection assetId={assetId} actions={data.actions} onSaved={() => void reload()} />
    </div>
  );
}

// ── 5 pourquoi ─────────────────────────────────────────────────────────────

type WhyDraft = {
  id: number | null;
  version: number | null;
  problem: string;
  whys: string[];
  rootCause: string;
};

function draftOf(a: FiveWhyAnalysis | null): WhyDraft {
  return {
    id: a?.id ?? null,
    version: a?.row_version ?? null,
    problem: a?.problem_statement ?? "",
    whys: WHY_KEYS.map((k) => a?.[k] ?? ""),
    rootCause: a?.root_cause ?? "",
  };
}

function FiveWhySection({
  assetId,
  whys,
  onSaved,
}: {
  assetId: number;
  whys: FiveWhyAnalysis[];
  onSaved: () => void;
}) {
  const { t } = useTranslation("equipment");
  const { formatDate } = useFormatters();
  const [draft, setDraft] = useState<WhyDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const text = (v: string) => v.trim() || null;
  const save = async () => {
    if (!draft || !draft.problem.trim()) return;
    setSaving(true);
    try {
      await saveAssetFiveWhy({
        id: draft.id,
        asset_id: assetId,
        expected_row_version: draft.version,
        problem_statement: draft.problem,
        why_1: text(draft.whys[0] ?? ""),
        why_2: text(draft.whys[1] ?? ""),
        why_3: text(draft.whys[2] ?? ""),
        why_4: text(draft.whys[3] ?? ""),
        why_5: text(draft.whys[4] ?? ""),
        root_cause: text(draft.rootCause),
      });
      setDraft(null);
      pushAppToast({ title: t("repeatFailures.fiveWhy.saved"), variant: "success" });
      onSaved();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <DetailSectionCard title={t("repeatFailures.fiveWhy.title")} icon={ListChecks}>
      {draft ? (
        <form
          className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <FormField name="five-why-problem" label={t("repeatFailures.fiveWhy.problem")} required>
            <Textarea
              id="five-why-problem"
              rows={2}
              value={draft.problem}
              onChange={(e) => setDraft({ ...draft, problem: e.target.value })}
            />
          </FormField>
          {draft.whys.map((w, i) => (
            <FormField
              key={WHY_KEYS[i]}
              name={`five-why-${i + 1}`}
              label={t("repeatFailures.fiveWhy.why", { n: i + 1 })}
            >
              <Input
                id={`five-why-${i + 1}`}
                value={w}
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    whys: draft.whys.map((x, j) => (j === i ? e.target.value : x)),
                  })
                }
              />
            </FormField>
          ))}
          <FormField name="five-why-root" label={t("repeatFailures.fiveWhy.rootCause")}>
            <Textarea
              id="five-why-root"
              rows={2}
              value={draft.rootCause}
              onChange={(e) => setDraft({ ...draft, rootCause: e.target.value })}
            />
          </FormField>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={() => setDraft(null)}>
              {t("repeatFailures.fiveWhy.cancel")}
            </Button>
            <Button type="submit" disabled={saving || !draft.problem.trim()}>
              {t("repeatFailures.fiveWhy.save")}
            </Button>
          </div>
        </form>
      ) : (
        <>
          {whys.length === 0 ? (
            <p className="text-sm text-text-muted">{t("repeatFailures.fiveWhy.empty")}</p>
          ) : (
            whys.map((a) => (
              <article key={a.id} className="space-y-1 rounded-md border border-surface-border p-3">
                <p className="text-sm font-medium">{a.problem_statement}</p>
                <ol className="list-decimal space-y-0.5 pl-5 text-sm">
                  {WHY_KEYS.map((k) => (a[k] ? <li key={k}>{a[k]}</li> : null))}
                </ol>
                {a.root_cause ? (
                  <p className="text-sm">
                    <span className="font-medium">{t("repeatFailures.fiveWhy.rootCause")} : </span>
                    {a.root_cause}
                  </p>
                ) : null}
                <div className="flex items-center justify-between gap-2">
                  <p className="text-xs text-text-muted">
                    {t("repeatFailures.fiveWhy.by", {
                      name: a.created_by_name ?? "—",
                      date: formatDate(a.created_at),
                    })}
                  </p>
                  <PermissionGate permission={P.EQ_MANAGE}>
                    <Button variant="outline" className="h-8" onClick={() => setDraft(draftOf(a))}>
                      {t("repeatFailures.fiveWhy.edit")}
                    </Button>
                  </PermissionGate>
                </div>
              </article>
            ))
          )}
          <PermissionGate permission={P.EQ_MANAGE}>
            <div className="flex justify-end">
              <Button
                variant="outline"
                className="h-9 gap-1.5"
                onClick={() => setDraft(draftOf(null))}
              >
                <Plus className="h-4 w-4" aria-hidden />
                {t("repeatFailures.fiveWhy.add")}
              </Button>
            </div>
          </PermissionGate>
        </>
      )}
    </DetailSectionCard>
  );
}

// ── Corrective actions ────────────────────────────────────────────────────

interface Owner {
  id: number;
  full_name: string;
  employee_code?: string | null;
}

type ActionDraft = {
  id: number | null;
  version: number | null;
  text: string;
  owner: Owner | null;
  dueDate: string;
  status: CorrectiveActionStatus;
};

function actionDraftOf(a: CorrectiveAction | null): ActionDraft {
  return {
    id: a?.id ?? null,
    version: a?.row_version ?? null,
    text: a?.action_text ?? "",
    owner:
      a?.owner_personnel_id != null
        ? {
            id: a.owner_personnel_id,
            full_name: a.owner_name ?? String(a.owner_personnel_id),
            employee_code: a.owner_employee_code,
          }
        : null,
    dueDate: a?.due_date ?? "",
    status: a?.status ?? "open",
  };
}

const STATUS_TONE: Record<CorrectiveActionStatus, "info" | "success" | "neutral"> = {
  open: "info",
  done: "success",
  cancelled: "neutral",
};

function ActionsSection({
  assetId,
  actions,
  onSaved,
}: {
  assetId: number;
  actions: CorrectiveAction[];
  onSaved: () => void;
}) {
  const { t } = useTranslation("equipment");
  const { formatDate } = useFormatters();
  const [draft, setDraft] = useState<ActionDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const complete = draft != null && !!draft.text.trim() && !!draft.owner && !!draft.dueDate;
  const save = async () => {
    if (!draft || !complete) return;
    setSaving(true);
    try {
      await saveAssetCorrectiveAction({
        id: draft.id,
        asset_id: assetId,
        expected_row_version: draft.version,
        action_text: draft.text,
        owner_personnel_id: draft.owner?.id ?? null,
        due_date: draft.dueDate || null,
        status: draft.status,
      });
      setDraft(null);
      pushAppToast({ title: t("repeatFailures.actions.saved"), variant: "success" });
      onSaved();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <DetailSectionCard title={t("repeatFailures.actions.title")} icon={ListChecks}>
      {actions.length === 0 ? (
        <p className="text-sm text-text-muted">{t("repeatFailures.actions.empty")}</p>
      ) : (
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs text-text-muted">
              <th className="py-1 font-medium">{t("repeatFailures.actions.action")}</th>
              <th className="py-1 font-medium">{t("repeatFailures.actions.owner")}</th>
              <th className="py-1 font-medium">{t("repeatFailures.actions.dueDate")}</th>
              <th className="py-1 font-medium">{t("repeatFailures.actions.status")}</th>
              <th className="py-1" />
            </tr>
          </thead>
          <tbody>
            {actions.map((a) => (
              <tr key={a.id} className="border-t border-surface-border align-top">
                <td className="py-1.5 pr-2">{a.action_text}</td>
                <td className="py-1.5 pr-2">{a.owner_name ?? "—"}</td>
                <td className="py-1.5 pr-2">
                  {a.due_date ? formatDate(a.due_date) : "—"}
                  {a.is_overdue ? (
                    <span className="ml-1">
                      <StatusChip tone="danger" label={t("repeatFailures.actions.overdue")} />
                    </span>
                  ) : null}
                </td>
                <td className="py-1.5 pr-2">
                  <StatusChip
                    tone={STATUS_TONE[a.status]}
                    label={t(`repeatFailures.actions.statuses.${a.status}`)}
                  />
                </td>
                <td className="py-1.5 text-right">
                  <PermissionGate permission={P.EQ_MANAGE}>
                    <Button
                      variant="outline"
                      className="h-8"
                      onClick={() => setDraft(actionDraftOf(a))}
                    >
                      {t("repeatFailures.actions.edit")}
                    </Button>
                  </PermissionGate>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {draft ? (
        <form
          className="grid gap-3 rounded-md border border-surface-border p-3 sm:grid-cols-2"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <div className="sm:col-span-2">
            <FormField name="action-text" label={t("repeatFailures.actions.action")} required>
              <Textarea
                id="action-text"
                rows={2}
                value={draft.text}
                onChange={(e) => setDraft({ ...draft, text: e.target.value })}
              />
            </FormField>
          </div>
          <OwnerPicker value={draft.owner} onChange={(owner) => setDraft({ ...draft, owner })} />
          <FormField name="action-due" label={t("repeatFailures.actions.dueDate")} required>
            <DateField
              id="action-due"
              value={draft.dueDate}
              onChange={(v) => setDraft({ ...draft, dueDate: v })}
            />
          </FormField>
          <FormField name="action-status" label={t("repeatFailures.actions.status")} required>
            <Select
              value={draft.status}
              onValueChange={(v) => setDraft({ ...draft, status: v as CorrectiveActionStatus })}
            >
              <SelectTrigger id="action-status" className="h-10">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {ACTION_STATUSES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {t(`repeatFailures.actions.statuses.${s}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
          <div className="flex items-end justify-end gap-2 sm:col-span-2">
            <Button type="button" variant="outline" onClick={() => setDraft(null)}>
              {t("repeatFailures.actions.cancel")}
            </Button>
            <Button type="submit" disabled={saving || !complete}>
              {t("repeatFailures.actions.save")}
            </Button>
          </div>
        </form>
      ) : (
        <PermissionGate permission={P.EQ_MANAGE}>
          <div className="flex justify-end">
            <Button
              variant="outline"
              className="h-9 gap-1.5"
              onClick={() => setDraft(actionDraftOf(null))}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("repeatFailures.actions.add")}
            </Button>
          </div>
        </PermissionGate>
      )}
    </DetailSectionCard>
  );
}

/** The action's owner, picked by name or employee code from the personnel list. */
function OwnerPicker({
  value,
  onChange,
}: {
  value: Owner | null;
  onChange: (owner: Owner | null) => void;
}) {
  const { t } = useTranslation("equipment");
  const canSearch = useCanReadList("people");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Personnel[]>([]);
  const [problem, setProblem] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );
  const label = t("repeatFailures.actions.owner");
  if (value) {
    return (
      <FormField name="action-owner" label={label} required>
        <div className="flex min-h-10 items-center justify-between rounded-md border border-surface-border px-3">
          <span>
            {value.full_name}
            {value.employee_code ? (
              <span className="ml-1 text-xs text-text-muted">({value.employee_code})</span>
            ) : null}
          </span>
          <Button
            type="button"
            variant="ghost"
            className="h-8"
            aria-label={t("repeatFailures.actions.cancel")}
            onClick={() => onChange(null)}
          >
            <X className="h-4 w-4" aria-hidden />
          </Button>
        </div>
      </FormField>
    );
  }
  if (!canSearch) {
    return (
      <FormField name="action-owner" label={label} required>
        <ListAccessNote list="people" />
      </FormField>
    );
  }
  const search = (q: string) => {
    setQuery(q);
    if (timer.current) clearTimeout(timer.current);
    if (!q.trim()) {
      setResults([]);
      return;
    }
    timer.current = setTimeout(() => {
      setProblem(null);
      listPersonnel({ search: q.trim(), limit: 10, employment_status: ["active"] })
        .then((page) => setResults(page.items))
        .catch((err: unknown) => {
          setResults([]);
          setProblem(appErrorText(err));
        });
    }, 250);
  };
  return (
    <FormField name="action-owner" label={label} required error={problem ?? undefined}>
      <div className="relative">
        <Search className="absolute left-2.5 top-3 h-4 w-4 text-text-muted" aria-hidden />
        <Input
          id="action-owner"
          className="pl-9"
          value={query}
          onChange={(e) => search(e.target.value)}
        />
      </div>
      {results.length > 0 ? (
        <ul className="max-h-48 overflow-y-auto rounded-md border border-surface-border">
          {results.map((p) => (
            <li key={p.id}>
              <button
                type="button"
                className="flex min-h-10 w-full items-center gap-2 px-3 text-left text-sm hover:bg-surface-2"
                onClick={() => {
                  onChange({ id: p.id, full_name: p.full_name, employee_code: p.employee_code });
                  setResults([]);
                  setQuery("");
                }}
              >
                <span className="font-medium">{p.full_name}</span>
                <span className="text-xs text-text-muted">{p.employee_code}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </FormField>
  );
}
