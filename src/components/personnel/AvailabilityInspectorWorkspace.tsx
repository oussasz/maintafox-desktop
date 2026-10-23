/**
 * AvailabilityInspectorWorkspace — planning inspector for assignability / constraints.
 * Distinct from Directory PersonnelDetailWorkspace (deep-link via onOpenPersonnel).
 */

import { Plus, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import {
  DetailFieldRow,
  DetailSectionCard,
  ENTITY_DETAIL_HEADER_ACTION_BTN,
  EntityDetailHeader,
  EntitySummaryKpiStrip,
  type EntitySummaryKpi,
} from "@/components/detail";
import { absenceBlockTitle } from "@/components/personnel/absence-block-display";
import {
  CELL_STATE_CLASS,
  deriveCellState,
  eachDateInclusive,
  type AvailabilityCellState,
  type AvailabilityPersonRow,
} from "@/components/personnel/availability-model";
import { usePersonnelLists } from "@/components/personnel/use-personnel-lists";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
import { formatOrDash, formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { useWoOpenRuleText } from "@/lib/wo-open-rule";
import {
  endAvailabilityBlock,
  getPersonnelAvailabilityState,
  getPersonnelWorkloadSummary,
  listSkillsMatrix,
} from "@/services/personnel-service";
import type {
  PersonnelAvailabilityBlock,
  PersonnelAvailabilityState,
  PersonnelWorkloadSummary,
  SkillMatrixRow,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export type AvailabilityCertWarning = "expired" | "expiring";

/** Reason codes of the availability state (`personnel::availability::REASON_*`); any other reason
 * is the absence type code of an active block. */
const AVAILABILITY_REASON_CODES: ReadonlySet<string> = new Set([
  "employment_terminated",
  "blocked_override",
  "open_work_orders",
]);

export type AvailabilityInspectorWorkspaceProps = {
  person: AvailabilityPersonRow;
  selectedBlock: PersonnelAvailabilityBlock | null;
  personBlocks: PersonnelAvailabilityBlock[];
  blocksLoading: boolean;
  blocksError: string | null;
  dateFrom: string;
  dateTo: string;
  certWarning?: AvailabilityCertWarning | null;
  onClose: () => void;
  onSelectBlock: (blockId: number | null) => void;
  onCreateBlock: (personnelId: number, date?: string) => void;
  onEditBlock: (block: PersonnelAvailabilityBlock) => void;
  onOpenPersonnel?: (personnelId: number) => void;
  onBlocksChanged: () => void;
};

function isBlockActiveNow(block: PersonnelAvailabilityBlock): boolean {
  const now = Date.now();
  const start = Date.parse(block.start_at);
  const end = Date.parse(block.end_at);
  if (!Number.isFinite(start) || !Number.isFinite(end)) return false;
  return start < now && now < end;
}

export function AvailabilityInspectorWorkspace({
  person,
  selectedBlock,
  personBlocks,
  blocksLoading,
  blocksError,
  dateFrom,
  dateTo,
  certWarning = null,
  onClose,
  onSelectBlock,
  onCreateBlock,
  onEditBlock,
  onOpenPersonnel,
  onBlocksChanged,
}: AvailabilityInspectorWorkspaceProps) {
  const { t } = useTranslation("personnel");
  const { formatDateTime } = useFormatters();
  const formatHorizonInstant = (iso: string) => formatDateTime(iso);
  const woOpenRuleText = useWoOpenRuleText();
  // Absence types are named from the plant's list (Données de référence), by exact code.
  const lists = usePersonnelLists();
  const [tab, setTab] = useState("overview");
  const [availabilityState, setAvailabilityState] = useState<PersonnelAvailabilityState | null>(
    null,
  );
  const [stateError, setStateError] = useState<string | null>(null);
  const [stateLoading, setStateLoading] = useState(false);
  const [skills, setSkills] = useState<SkillMatrixRow[]>([]);
  const [skillsError, setSkillsError] = useState<string | null>(null);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [workload, setWorkload] = useState<PersonnelWorkloadSummary | null>(null);
  const [workloadError, setWorkloadError] = useState<string | null>(null);
  const [workloadLoading, setWorkloadLoading] = useState(false);
  const [ending, setEnding] = useState(false);
  const [endError, setEndError] = useState<string | null>(null);

  const dates = useMemo(() => eachDateInclusive(dateFrom, dateTo), [dateFrom, dateTo]);

  const entityLabel = useMemo(() => {
    for (const cell of person.cells.values()) {
      if (cell.entity_name) return cell.entity_name;
    }
    return null;
  }, [person.cells]);

  const kpiMetrics = useMemo(() => {
    let free = 0;
    let partial = 0;
    let blockedCritical = 0;
    let availableMinutes = 0;
    for (const date of dates) {
      const cell = person.cells.get(date);
      if (!cell) continue;
      const state = deriveCellState(cell);
      if (state === "free") free += 1;
      else if (state === "partial") partial += 1;
      else if (state === "blocked" || state === "critical") blockedCritical += 1;
      availableMinutes += cell.available_minutes;
    }
    return { free, partial, blockedCritical, availableMinutes };
  }, [dates, person.cells]);

  const kpis = useMemo<EntitySummaryKpi[]>(
    () => [
      {
        id: "free",
        label: t("availability.inspector.kpi.freeDays"),
        value: String(kpiMetrics.free),
      },
      {
        id: "partial",
        label: t("availability.inspector.kpi.partialDays"),
        value: String(kpiMetrics.partial),
      },
      {
        id: "blocked",
        label: t("availability.inspector.kpi.blockedDays"),
        value: String(kpiMetrics.blockedCritical),
      },
      {
        id: "minutes",
        label: t("availability.inspector.kpi.availableMinutes"),
        value: String(kpiMetrics.availableMinutes),
      },
    ],
    [kpiMetrics, t],
  );

  const selectedBlockId = selectedBlock?.id ?? null;
  useEffect(() => {
    setTab(selectedBlockId != null ? "constraint" : "overview");
    setEndError(null);
  }, [person.personnel_id, selectedBlockId]);

  useEffect(() => {
    let cancelled = false;
    const personnelId = person.personnel_id;

    setStateLoading(true);
    setStateError(null);
    setSkillsLoading(true);
    setSkillsError(null);
    setWorkloadLoading(true);
    setWorkloadError(null);

    void getPersonnelAvailabilityState(personnelId)
      .then((state) => {
        if (!cancelled) setAvailabilityState(state);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setAvailabilityState(null);
          setStateError(describeAppError(err).title);
        }
      })
      .finally(() => {
        if (!cancelled) setStateLoading(false);
      });

    void listSkillsMatrix({ personnel_id: personnelId, include_inactive: false })
      .then((rows) => {
        if (!cancelled) setSkills(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setSkills([]);
          setSkillsError(describeAppError(err).title);
        }
      })
      .finally(() => {
        if (!cancelled) setSkillsLoading(false);
      });

    void getPersonnelWorkloadSummary(personnelId)
      .then((row) => {
        if (!cancelled) setWorkload(row);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setWorkload(null);
          setWorkloadError(describeAppError(err).title);
        }
      })
      .finally(() => {
        if (!cancelled) setWorkloadLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [person.personnel_id]);

  const handleEndBlock = async () => {
    if (!selectedBlock) return;
    setEnding(true);
    setEndError(null);
    try {
      await endAvailabilityBlock(selectedBlock.id);
      onSelectBlock(null);
      onBlocksChanged();
    } catch (err) {
      setEndError(describeAppError(err).title);
    } finally {
      setEnding(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-col gap-3">
      <EntityDetailHeader
        code={person.employee_code}
        designation={formatPersonLabel(person.full_name)}
        statusSlot={
          <div className="flex flex-wrap gap-1.5 pt-1">
            {availabilityState ? (
              <Badge variant="secondary">
                {t(`availability.inspector.state.${availabilityState.status}`, {
                  defaultValue: availabilityState.status,
                })}
              </Badge>
            ) : null}
            {certWarning === "expired" ? (
              <Badge variant="destructive">{t("availability.badges.certExpired")}</Badge>
            ) : null}
            {certWarning === "expiring" ? (
              <Badge variant="outline">{t("availability.badges.certExpiring")}</Badge>
            ) : null}
            {selectedBlock?.is_critical ? (
              <Badge variant="destructive">{t("availability.detail.critical")}</Badge>
            ) : null}
          </div>
        }
        footerSlot={<EntitySummaryKpiStrip kpis={kpis} />}
        primaryActionsSlot={
          <>
            <PermissionGate permission={P.PER_MANAGE}>
              <Button
                type="button"
                size="sm"
                className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                onClick={() => onCreateBlock(person.personnel_id)}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("availability.actions.createBlock")}
              </Button>
            </PermissionGate>
            {onOpenPersonnel ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                onClick={() => onOpenPersonnel(person.personnel_id)}
              >
                {t("availability.actions.openPersonnel")}
              </Button>
            ) : null}
            <Button
              type="button"
              size="sm"
              variant="outline"
              className={ENTITY_DETAIL_HEADER_ACTION_BTN}
              onClick={onClose}
              aria-label={t("availability.actions.closeDetail")}
            >
              <X className="h-3.5 w-3.5" />
              {t("availability.actions.closeDetail")}
            </Button>
          </>
        }
      />

      <Tabs value={tab} onValueChange={setTab} className="min-h-0 flex-1">
        <TabsList className="h-auto flex-wrap justify-start gap-1">
          <TabsTrigger value="overview">{t("availability.inspector.tabs.overview")}</TabsTrigger>
          <TabsTrigger value="horizon">{t("availability.inspector.tabs.horizon")}</TabsTrigger>
          <TabsTrigger value="constraint">
            {t("availability.inspector.tabs.constraint")}
          </TabsTrigger>
          <TabsTrigger value="skills">{t("availability.inspector.tabs.skills")}</TabsTrigger>
          <TabsTrigger value="workload">{t("availability.inspector.tabs.workload")}</TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="mt-3 space-y-3">
          <DetailSectionCard title={t("availability.inspector.overview.identity")}>
            <DetailFieldRow
              label={t("availability.inspector.overview.team")}
              value={formatOrDash(person.team_name)}
            />
            <DetailFieldRow
              label={t("availability.inspector.overview.entity")}
              value={formatOrDash(entityLabel)}
            />
            <DetailFieldRow
              label={t("availability.inspector.overview.horizon")}
              value={`${dateFrom} → ${dateTo}`}
            />
          </DetailSectionCard>
          <DetailSectionCard title={t("availability.inspector.overview.assignability")}>
            {stateLoading ? <p className="text-sm text-text-muted">{t("common.loading")}</p> : null}
            {stateError ? <p className="text-sm text-destructive">{stateError}</p> : null}
            {!stateLoading && !stateError && availabilityState ? (
              <>
                <DetailFieldRow
                  label={t("availability.inspector.overview.status")}
                  value={t(`availability.inspector.state.${availabilityState.status}`, {
                    defaultValue: availabilityState.status,
                  })}
                />
                {availabilityState.reasons.length > 0 ? (
                  <ul className="mt-2 list-inside list-disc text-sm text-text-muted">
                    {availabilityState.reasons.map((reason) => (
                      <li key={reason}>
                        {AVAILABILITY_REASON_CODES.has(reason)
                          ? t(`availability.inspector.reason.${reason}`, {
                              count: availabilityState.open_work_order_count ?? 0,
                            })
                          : lists.text("absence_types", reason)}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="mt-2 text-sm text-text-muted">
                    {t("availability.inspector.overview.noReasons")}
                  </p>
                )}
              </>
            ) : null}
            {!stateLoading && !stateError && !availabilityState ? (
              <p className="text-sm text-text-muted">{t("common.noData")}</p>
            ) : null}
          </DetailSectionCard>
        </TabsContent>

        <TabsContent value="horizon" className="mt-3">
          <DetailSectionCard title={t("availability.inspector.horizon.title")}>
            <p className="mb-2 text-xs text-text-muted">
              {t("availability.inspector.horizon.hint")}
            </p>
            <div className="flex flex-wrap gap-1">
              {dates.map((d) => {
                const cell = person.cells.get(d);
                const state: AvailabilityCellState = cell ? deriveCellState(cell) : "rest";
                return (
                  <button
                    key={d}
                    type="button"
                    className={cn(
                      "rounded border px-1.5 py-1 text-left text-[10px] leading-tight",
                      CELL_STATE_CLASS[state],
                    )}
                    title={t(`availability.cell.${state}`)}
                    onClick={() => onCreateBlock(person.personnel_id, d)}
                  >
                    <span className="block font-medium">{d.slice(5)}</span>
                    <span className="block opacity-80">{t(`availability.cell.${state}`)}</span>
                  </button>
                );
              })}
            </div>
          </DetailSectionCard>
        </TabsContent>

        <TabsContent value="constraint" className="mt-3 space-y-3">
          {selectedBlock ? (
            <DetailSectionCard title={absenceBlockTitle(selectedBlock, lists, t)}>
              <DetailFieldRow
                label={t("availability.form.startAt")}
                value={formatHorizonInstant(selectedBlock.start_at)}
              />
              <DetailFieldRow
                label={t("availability.form.endAt")}
                value={formatHorizonInstant(selectedBlock.end_at)}
              />
              <DetailFieldRow
                label={t("availability.form.reason")}
                value={formatOrDash(selectedBlock.reason_note)}
              />
              <DetailFieldRow
                label={t("availability.inspector.constraint.createdBy")}
                value={formatOrDash(selectedBlock.created_by_label)}
              />
              {selectedBlock.is_critical ? (
                <Badge variant="destructive" className="mt-2">
                  {t("availability.detail.critical")}
                </Badge>
              ) : null}
              {endError ? <p className="mt-2 text-sm text-destructive">{endError}</p> : null}
              <div className="mt-3 flex flex-wrap gap-2">
                <PermissionGate permission={P.PER_MANAGE}>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => onEditBlock(selectedBlock)}
                  >
                    {t("availability.actions.editBlock")}
                  </Button>
                  {isBlockActiveNow(selectedBlock) ? (
                    <Button
                      type="button"
                      size="sm"
                      variant="destructive"
                      disabled={ending}
                      onClick={() => void handleEndBlock()}
                    >
                      {ending ? t("common.loading") : t("availability.actions.endBlock")}
                    </Button>
                  ) : null}
                </PermissionGate>
              </div>
            </DetailSectionCard>
          ) : (
            <DetailSectionCard title={t("availability.inspector.constraint.emptyTitle")}>
              <p className="text-sm text-text-muted">
                {t("availability.inspector.constraint.empty")}
              </p>
            </DetailSectionCard>
          )}

          <DetailSectionCard title={t("availability.detail.blocks")}>
            {blocksLoading ? (
              <p className="text-sm text-text-muted">{t("common.loading")}</p>
            ) : null}
            {blocksError ? <p className="text-sm text-destructive">{blocksError}</p> : null}
            {!blocksLoading && !blocksError && personBlocks.length === 0 ? (
              <p className="text-sm text-text-muted">{t("availability.emptyBlocks")}</p>
            ) : null}
            {!blocksLoading && personBlocks.length > 0 ? (
              <ul className="space-y-2 text-sm">
                {personBlocks.map((b) => (
                  <li key={b.id}>
                    <button
                      type="button"
                      className={cn(
                        "w-full rounded border px-2 py-1.5 text-left",
                        selectedBlock?.id === b.id
                          ? "border-primary bg-primary/5"
                          : "border-surface-border",
                      )}
                      onClick={() => onSelectBlock(b.id)}
                    >
                      <div className="font-medium">{absenceBlockTitle(b, lists, t)}</div>
                      <div className="text-xs text-text-muted">
                        {formatHorizonInstant(b.start_at)} → {formatHorizonInstant(b.end_at)}
                      </div>
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}
          </DetailSectionCard>
        </TabsContent>

        <TabsContent value="skills" className="mt-3">
          <DetailSectionCard title={t("availability.inspector.skills.title")}>
            {skillsLoading ? (
              <p className="text-sm text-text-muted">{t("common.loading")}</p>
            ) : null}
            {skillsError ? <p className="text-sm text-destructive">{skillsError}</p> : null}
            {!skillsLoading && !skillsError && skills.length === 0 ? (
              <p className="text-sm text-text-muted">{t("availability.inspector.skills.empty")}</p>
            ) : null}
            {!skillsLoading && skills.length > 0 ? (
              <ul className="space-y-1.5 text-sm">
                {skills.map((row) => (
                  <li
                    key={`${row.skill_code ?? "none"}-${row.coverage_status}`}
                    className="flex flex-wrap items-center justify-between gap-2 rounded border border-surface-border px-2 py-1.5"
                  >
                    <span>{formatOrDash(row.skill_label ?? row.skill_code)}</span>
                    <div className="flex items-center gap-2">
                      {row.proficiency_level != null ? (
                        <span className="text-xs text-text-muted">L{row.proficiency_level}</span>
                      ) : null}
                      <Badge
                        variant={
                          row.coverage_status === "expired"
                            ? "destructive"
                            : row.coverage_status === "missing"
                              ? "outline"
                              : "secondary"
                        }
                      >
                        {t(`skills.coverage.${row.coverage_status}`, row.coverage_status)}
                      </Badge>
                    </div>
                  </li>
                ))}
              </ul>
            ) : null}
          </DetailSectionCard>
        </TabsContent>

        <TabsContent value="workload" className="mt-3">
          <DetailSectionCard title={t("availability.inspector.workload.title")}>
            {workloadLoading ? (
              <p className="text-sm text-text-muted">{t("common.loading")}</p>
            ) : null}
            {workloadError ? <p className="text-sm text-destructive">{workloadError}</p> : null}
            {!workloadLoading && !workloadError && workload ? (
              <>
                <DetailFieldRow
                  label={
                    woOpenRuleText(workload.open_work_orders_rule)?.figure ??
                    t("detail.workload.openWo")
                  }
                  value={String(workload.open_work_orders)}
                />
                <DetailFieldRow
                  label={
                    woOpenRuleText(workload.in_progress_work_orders_rule)?.figure ??
                    t("detail.workload.inProgressWo")
                  }
                  value={String(workload.in_progress_work_orders)}
                />
                <DetailFieldRow
                  label={t("detail.workload.pendingDi")}
                  value={String(workload.pending_interventions)}
                />
                <DetailFieldRow
                  label={t("detail.workload.di30d")}
                  value={String(workload.interventions_last_30d)}
                />
              </>
            ) : null}
            {!workloadLoading && !workloadError && !workload ? (
              <p className="text-sm text-text-muted">{t("common.noData")}</p>
            ) : null}
          </DetailSectionCard>
        </TabsContent>
      </Tabs>
    </div>
  );
}
