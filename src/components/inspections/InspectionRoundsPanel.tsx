/**
 * Round list (DataTable, filtered by the page's SmartFilterBar or a KPI tile) and the dialog to
 * schedule a single round with a checklist, a technician and/or a team picked by name.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { CloudOff, Loader2, Play, Search, X } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import {
  Button,
  Dialog,
  DialogContent,
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
} from "@/components/ui";
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { DateTimeLocalField } from "@/components/ui/date-time-local-field";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorText } from "@/lib/app-error";
import {
  listInspectionRoundPage,
  listInspectionTemplates,
  scheduleInspectionRound,
  startInspectionRound,
} from "@/services/inspection-service";
import { listActiveTeams, listPersonnel } from "@/services/personnel-service";
import type {
  InspectionRoundSummary,
  InspectionRoundSummaryFilter,
  InspectionTemplate,
  Personnel,
  TeamSummary,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { ROUND_OPEN_STATUSES, ROUND_STATUS_TONE } from "./inspection-rules";

/** Rounds loaded per page (backend maximum: `runner::MAX_ROUND_PAGE`). */
const PAGE_SIZE = 200;

interface InspectionRoundsPanelProps {
  filter: InspectionRoundSummaryFilter;
  onOpenRound: (roundId: number) => void;
  /** Bump to reload (after the runner, a dialog or a generation run changed rounds). */
  refreshKey: number;
  onCountChange?: (total: number | null) => void;
}

export function InspectionRoundsPanel({
  filter,
  onOpenRound,
  refreshKey,
  onCountChange,
}: InspectionRoundsPanelProps) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const { formatDateTime } = useFormatters();
  const [rounds, setRounds] = useState<InspectionRoundSummary[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  const filterKey = JSON.stringify(filter);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await listInspectionRoundPage({ ...filter, limit: PAGE_SIZE, offset: 0 });
      setRounds(page.items);
      setTotal(page.total);
      setError(null);
      onCountChange?.(page.total);
    } catch (err) {
      setError(err);
      onCountChange?.(null);
    } finally {
      setLoading(false);
    }
    // `filterKey` stands for `filter` (a new object on every render of the page).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, onCountChange]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const loadMore = async () => {
    setLoadingMore(true);
    try {
      const page = await listInspectionRoundPage({
        ...filter,
        limit: PAGE_SIZE,
        offset: rounds.length,
      });
      setRounds((prev) => [...prev, ...page.items]);
      setTotal(page.total);
    } catch {
      // Reported by the global IPC toast; the rows already shown stay.
    } finally {
      setLoadingMore(false);
    }
  };

  const startAndOpen = useCallback(
    async (s: InspectionRoundSummary) => {
      const startable = ROUND_OPEN_STATUSES.has(s.round.status) && s.round.status !== "in_progress";
      if (!startable || !can(P.INS_EXECUTE)) {
        onOpenRound(s.round.id);
        return;
      }
      setBusyId(s.round.id);
      try {
        await startInspectionRound({
          round_id: s.round.id,
          expected_row_version: s.round.row_version,
        });
        onOpenRound(s.round.id);
      } catch {
        // Reported by the global IPC toast.
      } finally {
        setBusyId(null);
      }
    },
    [can, onOpenRound],
  );

  const actionLabel = useCallback(
    (s: InspectionRoundSummary) => {
      if (s.round.status === "in_progress") return t("rounds.continue");
      if (ROUND_OPEN_STATUSES.has(s.round.status) && can(P.INS_EXECUTE)) return t("rounds.start");
      if (s.round.status === "completed_with_findings") return t("rounds.review");
      return t("rounds.open");
    },
    [can, t],
  );

  const columns = useMemo<ColumnDef<InspectionRoundSummary>[]>(
    () => [
      {
        id: "template",
        accessorFn: (s) => s.template_name,
        header: t("rounds.columns.checklist"),
        cell: ({ row }) => (
          <div className="min-w-0">
            <p className="truncate font-medium text-text-primary">{row.original.template_name}</p>
            <p className="font-mono text-xs text-text-muted">
              {row.original.template_code}
              {row.original.schedule_code ? ` · ${row.original.schedule_code}` : ""}
            </p>
          </div>
        ),
      },
      {
        id: "planned",
        accessorFn: (s) => s.round.scheduled_at ?? "",
        header: t("rounds.columns.planned"),
        cell: ({ row }) => (
          <div className="flex flex-col gap-1 text-xs">
            <span>
              {row.original.round.scheduled_at
                ? formatDateTime(row.original.round.scheduled_at)
                : t("rounds.notScheduled")}
            </span>
            {row.original.is_overdue ? (
              <StatusChip label={t("rounds.overdue")} tone="danger" />
            ) : null}
          </div>
        ),
      },
      {
        id: "assignee",
        accessorFn: (s) => s.assigned_to_name ?? s.assigned_team_name ?? "",
        header: t("rounds.columns.assignee"),
        cell: ({ row }) => {
          const s = row.original;
          if (!s.assigned_to_name && !s.assigned_team_name)
            return <span className="text-xs text-text-muted">{t("rounds.unassigned")}</span>;
          return (
            <div className="text-xs">
              {s.assigned_to_name ? <p>{s.assigned_to_name}</p> : null}
              {s.assigned_team_name ? (
                <p className="text-text-muted">
                  {t("rounds.team", { name: s.assigned_team_name })}
                </p>
              ) : null}
            </div>
          );
        },
      },
      {
        id: "status",
        accessorFn: (s) => s.round.status,
        header: t("rounds.columns.status"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`status.${row.original.round.status}`)}
            tone={toneOf(ROUND_STATUS_TONE, row.original.round.status)}
            dot
          />
        ),
      },
      {
        id: "progress",
        accessorFn: (s) => (s.checkpoint_count > 0 ? s.recorded_count / s.checkpoint_count : 0),
        header: t("rounds.columns.progress"),
        cell: ({ row }) => {
          const s = row.original;
          const pct = s.checkpoint_count > 0 ? (100 * s.recorded_count) / s.checkpoint_count : 0;
          return (
            <div className="w-28 space-y-1">
              <div
                className="h-1.5 overflow-hidden rounded bg-surface-3"
                role="progressbar"
                aria-label={t("rounds.progress", {
                  done: s.recorded_count,
                  total: s.checkpoint_count,
                })}
                aria-valuemin={0}
                aria-valuemax={s.checkpoint_count}
                aria-valuenow={s.recorded_count}
              >
                <div className="h-full bg-primary" style={{ width: `${pct}%` }} />
              </div>
              <p className="text-xs text-text-muted">
                {t("rounds.progress", { done: s.recorded_count, total: s.checkpoint_count })}
              </p>
            </div>
          );
        },
      },
      {
        id: "findings",
        accessorFn: (s) => s.open_anomaly_count,
        header: t("rounds.columns.findings"),
        cell: ({ row }) => {
          const s = row.original;
          return (
            <div className="flex flex-col gap-1 text-xs">
              {s.exception_count > 0 ? (
                <span>{t("rounds.exceptions", { count: s.exception_count })}</span>
              ) : null}
              {s.open_anomaly_count > 0 ? (
                <StatusChip
                  label={t("rounds.openAnomalies", { count: s.open_anomaly_count })}
                  tone="attention"
                />
              ) : null}
              {s.pending_sync_count > 0 ? (
                <span className="flex items-center gap-1 text-tone-warning-fg">
                  <CloudOff className="h-3.5 w-3.5" aria-hidden />
                  {t("rounds.pendingSync", { count: s.pending_sync_count })}
                </span>
              ) : null}
              {s.exception_count === 0 &&
              s.open_anomaly_count === 0 &&
              s.pending_sync_count === 0 ? (
                <span className="text-text-muted">—</span>
              ) : null}
            </div>
          );
        },
      },
      {
        id: "action",
        enableSorting: false,
        header: () => <span className="sr-only">{t("rounds.columns.action")}</span>,
        cell: ({ row }) => {
          const s = row.original;
          const busy = busyId === s.round.id;
          return (
            <Button
              size="sm"
              className="h-11 min-w-28"
              variant={ROUND_OPEN_STATUSES.has(s.round.status) ? "default" : "outline"}
              disabled={busy}
              aria-label={`${actionLabel(s)} — ${s.template_name}`}
              onClick={(e) => {
                e.stopPropagation();
                void startAndOpen(s);
              }}
            >
              {busy ? (
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" aria-hidden />
              ) : ROUND_OPEN_STATUSES.has(s.round.status) ? (
                <Play className="mr-1.5 h-4 w-4" aria-hidden />
              ) : null}
              {actionLabel(s)}
            </Button>
          );
        },
      },
    ],
    [actionLabel, busyId, formatDateTime, startAndOpen, t],
  );

  if (error != null && rounds.length === 0) {
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  }
  if (!loading && rounds.length === 0) {
    return <EmptyState title={t("rounds.empty")} description={t("rounds.emptyHint")} />;
  }
  return (
    <div className="space-y-3" data-testid="inspection-round-list">
      <TruncationNotice
        shown={rounds.length}
        total={total}
        onLoadMore={() => void loadMore()}
        loadingMore={loadingMore}
      />
      <DataTable
        columns={columns}
        data={rounds}
        pageSize={20}
        isLoading={loading}
        skeletonRows={6}
        onRowClick={(s) => onOpenRound(s.round.id)}
      />
    </div>
  );
}

function localToIso(value: string): string | null {
  if (!value) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

interface ScheduleRoundDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onScheduled: () => void;
}

export function ScheduleRoundDialog({ open, onOpenChange, onScheduled }: ScheduleRoundDialogProps) {
  const { t } = useTranslation("inspections");
  const [templates, setTemplates] = useState<InspectionTemplate[]>([]);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [templateId, setTemplateId] = useState("");
  const [when, setWhen] = useState("");
  const [assignee, setAssignee] = useState<Personnel | null>(null);
  const [teamId, setTeamId] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setTemplateId("");
    setWhen("");
    setAssignee(null);
    setTeamId(null);
    setLoadError(null);
    listInspectionTemplates()
      .then((all) => setTemplates(all.filter((x) => x.is_active && x.current_version_id != null)))
      .catch((err: unknown) => setLoadError(err));
  }, [open]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!templateId) return;
    setBusy(true);
    try {
      await scheduleInspectionRound({
        template_id: Number(templateId),
        scheduled_at: localToIso(when),
        assigned_to_id: assignee?.id ?? null,
        assigned_team_id: teamId,
        explicit_template_version_id: null,
      });
      onScheduled();
      onOpenChange(false);
    } catch {
      // Reported by the global IPC toast; the dialog stays open.
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <form onSubmit={(e) => void submit(e)} className="space-y-4">
          <DialogHeader>
            <DialogTitle>{t("schedule.title")}</DialogTitle>
          </DialogHeader>
          {loadError != null ? (
            <ErrorState error={loadError} />
          ) : templates.length === 0 ? (
            <p className="text-sm text-text-muted">{t("schedule.noTemplate")}</p>
          ) : (
            <FormField name="schedule-template" label={t("schedule.template")} required>
              <Select value={templateId} onValueChange={setTemplateId}>
                <SelectTrigger id="schedule-template" className="h-11">
                  <SelectValue placeholder={t("schedule.templatePlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {templates.map((x) => (
                    <SelectItem key={x.id} value={String(x.id)}>
                      {x.name} ({x.code})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
          )}
          <DateTimeLocalField
            id="schedule-when"
            label={t("schedule.when")}
            value={when}
            onChange={setWhen}
          />
          <PersonnelPicker id="schedule-assignee" value={assignee} onChange={setAssignee} />
          <TeamSelect id="schedule-team" value={teamId} onChange={setTeamId} />
          <DialogFooter>
            <Button type="submit" className="h-11" disabled={busy || !templateId}>
              {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
              {t("schedule.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface PersonnelPickerProps {
  id: string;
  value: Personnel | { id: number; full_name: string; employee_code?: string } | null;
  onChange: (p: Personnel | null) => void;
}

/** Person picked by name or employee code; never by id. */
export function PersonnelPicker({ id, value, onChange }: PersonnelPickerProps) {
  const { t } = useTranslation("inspections");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Personnel[]>([]);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searched, setSearched] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const search = (q: string) => {
    setQuery(q);
    if (timer.current) clearTimeout(timer.current);
    if (!q.trim()) {
      setResults([]);
      setSearched(false);
      return;
    }
    timer.current = setTimeout(() => {
      setSearching(true);
      setError(null);
      listPersonnel({ search: q.trim(), limit: 15, employment_status: ["active"] })
        .then((page) => {
          setResults(page.items);
          setSearched(true);
        })
        .catch((err: unknown) => {
          setResults([]);
          setError(appErrorText(err));
        })
        .finally(() => setSearching(false));
    }, 250);
  };

  if (value) {
    return (
      <FormField name={id} label={t("schedule.assignee")}>
        <div className="flex min-h-11 items-center justify-between rounded-md border border-surface-border px-3">
          <span>
            {value.full_name}{" "}
            {value.employee_code ? (
              <span className="text-xs text-text-muted">({value.employee_code})</span>
            ) : null}
          </span>
          <Button
            type="button"
            variant="ghost"
            className="h-11"
            aria-label={t("schedule.clearAssigneeNamed", { name: value.full_name })}
            onClick={() => onChange(null)}
          >
            <X className="mr-1 h-4 w-4" aria-hidden />
            {t("schedule.clearAssignee")}
          </Button>
        </div>
      </FormField>
    );
  }

  return (
    <FormField name={id} label={t("schedule.assignee")}>
      <div className="relative">
        <Search className="absolute left-2.5 top-3.5 h-4 w-4 text-text-muted" aria-hidden />
        <Input
          id={id}
          className="min-h-11 pl-9"
          placeholder={t("schedule.assigneeSearch")}
          value={query}
          onChange={(e) => search(e.target.value)}
        />
        {searching ? (
          <Loader2
            className="absolute right-2.5 top-3.5 h-4 w-4 animate-spin text-text-muted"
            aria-hidden
          />
        ) : null}
      </div>
      {error ? (
        <p role="alert" className="text-xs text-tone-danger-fg">
          {t("schedule.assigneeSearchError", { message: error })}
        </p>
      ) : null}
      {searched && results.length === 0 && !error ? (
        <p className="text-xs text-text-muted">{t("schedule.assigneeNone")}</p>
      ) : null}
      {results.length > 0 ? (
        <ul className="max-h-56 overflow-y-auto rounded-md border border-surface-border">
          {results.map((p) => (
            <li key={p.id}>
              <button
                type="button"
                className="flex min-h-11 w-full items-center gap-2 px-3 text-left text-sm hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                onClick={() => {
                  onChange(p);
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

const NO_TEAM = "__none__";

interface TeamSelectProps {
  id: string;
  value: number | null;
  onChange: (teamId: number | null) => void;
}

/** Team picked by name among the active teams (needs the right to list personnel). */
export function TeamSelect({ id, value, onChange }: TeamSelectProps) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const allowed = can(P.PER_VIEW);
  const [teams, setTeams] = useState<TeamSummary[]>([]);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!allowed) return;
    listActiveTeams()
      .then((rows) => setTeams(rows))
      .catch(() => setFailed(true));
  }, [allowed]);

  if (!allowed) {
    return (
      <FormField name={id} label={t("schedule.team")}>
        <p className="text-sm text-text-muted">{t("schedule.teamNotAllowed")}</p>
      </FormField>
    );
  }
  return (
    <FormField name={id} label={t("schedule.team")}>
      {failed ? (
        <p role="alert" className="text-sm text-tone-danger-fg">
          {t("schedule.teamLoadError")}
        </p>
      ) : (
        <Select
          value={value == null ? NO_TEAM : String(value)}
          onValueChange={(v) => onChange(v === NO_TEAM ? null : Number(v))}
        >
          <SelectTrigger id={id} className="h-11">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NO_TEAM}>{t("schedule.noTeam")}</SelectItem>
            {teams.map((team) => (
              <SelectItem key={team.id} value={String(team.id)}>
                {team.name} ({team.code})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </FormField>
  );
}
