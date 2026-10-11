/**
 * Round list for the field (large cards, labels only, offline queue per round) and the
 * dialog to schedule a round with a checklist and a technician picked by name.
 */
import { CalendarPlus, CloudOff, Loader2, Play, Search, X } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  Badge,
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
import { DateTimeLocalField } from "@/components/ui/date-time-local-field";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  listInspectionRoundSummaries,
  listInspectionTemplates,
  scheduleInspectionRound,
  startInspectionRound,
} from "@/services/inspection-service";
import { listPersonnel } from "@/services/personnel-service";
import { toErrorMessage } from "@/utils/errors";
import type { InspectionRoundSummary, InspectionTemplate, Personnel } from "@shared/ipc-types";

import { ROUND_FILTERS, ROUND_OPEN_STATUSES, type RoundFilterKey } from "./inspection-rules";

interface InspectionRoundsPanelProps {
  onOpenRound: (roundId: number) => void;
  /** Bump to reload (e.g. after the runner changed a round). */
  refreshKey: number;
}

const FILTER_KEYS: RoundFilterKey[] = ["todo", "toReview", "done", "all"];

const STATUS_BADGE: Record<string, string> = {
  in_progress: "border-primary text-primary",
  completed_with_findings: "border-status-warning text-status-warning",
  reviewed: "border-status-success text-status-success",
  completed: "border-status-success text-status-success",
};

export function InspectionRoundsPanel({ onOpenRound, refreshKey }: InspectionRoundsPanelProps) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const { formatDateTime } = useFormatters();
  const [filter, setFilter] = useState<RoundFilterKey>("todo");
  const [rounds, setRounds] = useState<InspectionRoundSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [scheduleOpen, setScheduleOpen] = useState(false);
  const [busyId, setBusyId] = useState<number | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const statuses = ROUND_FILTERS[filter];
      setRounds(
        await listInspectionRoundSummaries({ statuses: statuses.length > 0 ? statuses : null }),
      );
      setError(null);
    } catch (err) {
      setError(t("errors.load", { message: toErrorMessage(err) }));
    } finally {
      setLoading(false);
    }
  }, [filter, t]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const startAndOpen = async (s: InspectionRoundSummary) => {
    if (s.round.status === "in_progress" || !ROUND_OPEN_STATUSES.has(s.round.status)) {
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
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusyId(null);
    }
  };

  const actionLabel = (s: InspectionRoundSummary) => {
    if (s.round.status === "in_progress") return t("rounds.continue");
    if (ROUND_OPEN_STATUSES.has(s.round.status)) return t("rounds.start");
    if (s.round.status === "completed_with_findings") return t("rounds.review");
    return t("rounds.open");
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        {FILTER_KEYS.map((key) => (
          <Button
            key={key}
            size="lg"
            variant={filter === key ? "default" : "outline"}
            className="min-h-12"
            aria-pressed={filter === key}
            onClick={() => setFilter(key)}
          >
            {t(`filters.${key}`)}
          </Button>
        ))}
        {can("ins.admin") ? (
          <Button size="lg" className="ml-auto min-h-12" onClick={() => setScheduleOpen(true)}>
            <CalendarPlus className="mr-2 h-5 w-5" />
            {t("rounds.schedule")}
          </Button>
        ) : null}
      </div>

      {error ? (
        <p role="alert" className="text-sm text-status-danger">
          {error}
        </p>
      ) : null}

      {loading ? (
        <Loader2 className="h-5 w-5 animate-spin text-text-muted" />
      ) : rounds.length === 0 ? (
        <p className="text-sm text-text-muted">{t("rounds.empty")}</p>
      ) : (
        <ul className="grid gap-3 md:grid-cols-2">
          {rounds.map((s) => (
            <li
              key={s.round.id}
              className="flex flex-col gap-2 rounded-lg border border-surface-border bg-surface-1 p-4"
              data-testid="round-card"
            >
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <p className="truncate text-base font-semibold text-text-primary">
                    {s.template_name}
                  </p>
                  <p className="text-xs text-text-muted">
                    {s.round.scheduled_at
                      ? t("rounds.scheduledFor", { date: formatDateTime(s.round.scheduled_at) })
                      : t("rounds.notScheduled")}{" "}
                    ·{" "}
                    {s.assigned_to_name
                      ? t("rounds.assignedTo", { name: s.assigned_to_name })
                      : t("rounds.unassigned")}
                  </p>
                </div>
                <Badge
                  variant="outline"
                  className={cn("shrink-0", STATUS_BADGE[s.round.status] ?? "")}
                >
                  {t(`status.${s.round.status}`)}
                </Badge>
              </div>
              <div className="h-2 overflow-hidden rounded bg-surface-2" aria-hidden>
                <div
                  className="h-full bg-primary"
                  style={{
                    width: `${s.checkpoint_count > 0 ? (100 * s.recorded_count) / s.checkpoint_count : 0}%`,
                  }}
                />
              </div>
              <p className="text-sm text-text-secondary">
                {t("rounds.progress", { done: s.recorded_count, total: s.checkpoint_count })}
                {s.exception_count > 0
                  ? ` · ${t("rounds.exceptions", { count: s.exception_count })}`
                  : ""}
                {s.open_anomaly_count > 0
                  ? ` · ${t("rounds.openAnomalies", { count: s.open_anomaly_count })}`
                  : ""}
              </p>
              {s.pending_sync_count > 0 ? (
                <p className="flex items-center gap-1 text-xs text-status-warning">
                  <CloudOff className="h-3.5 w-3.5" aria-hidden />
                  {t("rounds.pendingSync", { count: s.pending_sync_count })}
                </p>
              ) : null}
              <Button
                size="lg"
                className="mt-auto min-h-12"
                variant={ROUND_OPEN_STATUSES.has(s.round.status) ? "default" : "outline"}
                disabled={busyId === s.round.id}
                onClick={() => void startAndOpen(s)}
              >
                {busyId === s.round.id ? (
                  <Loader2 className="mr-2 h-5 w-5 animate-spin" />
                ) : ROUND_OPEN_STATUSES.has(s.round.status) ? (
                  <Play className="mr-2 h-5 w-5" />
                ) : null}
                {actionLabel(s)}
              </Button>
            </li>
          ))}
        </ul>
      )}

      <ScheduleRoundDialog
        open={scheduleOpen}
        onOpenChange={setScheduleOpen}
        onScheduled={() => void load()}
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

function ScheduleRoundDialog({ open, onOpenChange, onScheduled }: ScheduleRoundDialogProps) {
  const { t } = useTranslation("inspections");
  const [templates, setTemplates] = useState<InspectionTemplate[]>([]);
  const [templateId, setTemplateId] = useState("");
  const [when, setWhen] = useState("");
  const [assignee, setAssignee] = useState<Personnel | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setTemplateId("");
    setWhen("");
    setAssignee(null);
    setError(null);
    listInspectionTemplates()
      .then((all) => setTemplates(all.filter((x) => x.is_active && x.current_version_id != null)))
      .catch((err: unknown) => setError(t("errors.load", { message: toErrorMessage(err) })));
  }, [open, t]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!templateId) return;
    setBusy(true);
    setError(null);
    try {
      await scheduleInspectionRound({
        template_id: Number(templateId),
        scheduled_at: localToIso(when),
        assigned_to_id: assignee?.id ?? null,
        explicit_template_version_id: null,
      });
      onScheduled();
      onOpenChange(false);
    } catch (err) {
      setError(toErrorMessage(err));
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
          {templates.length === 0 ? (
            <p className="text-sm text-text-muted">{t("schedule.noTemplate")}</p>
          ) : (
            <FormField name="schedule-template" label={t("schedule.template")} required>
              <Select value={templateId} onValueChange={setTemplateId}>
                <SelectTrigger id="schedule-template">
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
          <PersonnelPicker value={assignee} onChange={setAssignee} />
          {error ? (
            <p role="alert" className="text-sm text-status-danger">
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="submit" size="lg" disabled={busy || !templateId}>
              {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
              {t("schedule.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface PersonnelPickerProps {
  value: Personnel | null;
  onChange: (p: Personnel | null) => void;
}

/** Person picked by name or employee code; never by id. */
function PersonnelPicker({ value, onChange }: PersonnelPickerProps) {
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
          setError(toErrorMessage(err));
        })
        .finally(() => setSearching(false));
    }, 250);
  };

  if (value) {
    return (
      <FormField name="schedule-assignee" label={t("schedule.assignee")}>
        <div className="flex min-h-12 items-center justify-between rounded-md border border-surface-border px-3">
          <span>
            {value.full_name}{" "}
            <span className="text-xs text-text-muted">({value.employee_code})</span>
          </span>
          <Button type="button" variant="ghost" size="sm" onClick={() => onChange(null)}>
            <X className="mr-1 h-4 w-4" />
            {t("schedule.clearAssignee")}
          </Button>
        </div>
      </FormField>
    );
  }

  return (
    <FormField name="schedule-assignee" label={t("schedule.assignee")}>
      <div className="relative">
        <Search className="absolute left-2.5 top-3.5 h-4 w-4 text-text-muted" />
        <Input
          id="schedule-assignee"
          className="min-h-12 pl-9"
          placeholder={t("schedule.assigneeSearch")}
          value={query}
          onChange={(e) => search(e.target.value)}
        />
        {searching ? (
          <Loader2 className="absolute right-2.5 top-3.5 h-4 w-4 animate-spin text-text-muted" />
        ) : null}
      </div>
      {error ? (
        <p role="alert" className="text-xs text-status-danger">
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
                className="flex min-h-12 w-full items-center gap-2 px-3 text-left text-sm hover:bg-surface-1"
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
