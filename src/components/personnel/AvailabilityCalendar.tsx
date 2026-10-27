/**
 * Availability tab — ResourceTimeline chronology + list secondary view.
 * Detail inspection: split pane (not centered Dialog). Block create/edit: dialog.
 *
 * Absence blocks are read per person. A person whose blocks could not be read (or not all of
 * them for the horizon) is never drawn as available: the row carries an "availability unknown"
 * bar over the whole horizon and a badge, and the page offers a retry.
 */

import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { AvailabilityBlockDialog } from "@/components/personnel/AvailabilityBlockDialog";
import { AvailabilityInspectorWorkspace } from "@/components/personnel/AvailabilityInspectorWorkspace";
import { absenceBlockState, absenceBlockTitle } from "@/components/personnel/absence-block-display";
import {
  addDaysLocal,
  AVAILABILITY_BLOCK_SEED_END_LOCAL,
  AVAILABILITY_BLOCK_SEED_START_LOCAL,
  AVAILABILITY_DEFAULT_HORIZON_DAYS,
  AVAILABILITY_HORIZON_PRESETS,
  buildPersonRows,
  CELL_STATE_CLASS,
  deriveCellState,
  eachDateInclusive,
  parseBlockTimeMs,
  toLocalIsoDate,
  type AvailabilityViewMode,
} from "@/components/personnel/availability-model";
import { usePersonnelLists } from "@/components/personnel/use-personnel-lists";
import {
  ResourceTimeline,
  type ResourceTimelineGroup,
  type ResourceTimelineItem,
} from "@/components/timeline/ResourceTimeline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  RESOURCE_TIMELINE_BLOCK_LEGEND_STATES,
  RESOURCE_TIMELINE_STATE_CLASS,
} from "@/design-system/resource-timeline-tokens";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorReason } from "@/lib/app-error";
import { formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { getOrgDesignerSnapshot } from "@/services/org-designer-service";
import {
  listAvailabilityCalendar,
  listPersonnelAvailabilityBlocks,
  listTeamCapacitySummary,
} from "@/services/personnel-service";
import { listCertificationExpiryDrilldown } from "@/services/qualification-service";
import type {
  AvailabilityCalendarEntry,
  OrgDesignerNodeRow,
  PersonnelAvailabilityBlock,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export type AvailabilityCalendarHandle = {
  refresh: () => void;
  openCreateBlock: () => void;
  setViewMode: (mode: AvailabilityViewMode) => void;
  getViewMode: () => AvailabilityViewMode;
};

interface AvailabilityCalendarProps {
  entityId?: number | null;
  onOpenPersonnel?: (personnelId: number) => void;
  viewMode?: AvailabilityViewMode;
  onViewModeChange?: (mode: AvailabilityViewMode) => void;
}

type CertWarning = "expired" | "expiring";

const CRITICAL_FILTER_VALUE = "critical";
const CERT_LOOKAHEAD_DAYS = 30;
/** Blocks read per person: the backend maximum (`list_personnel_availability_blocks`, newest first). */
const BLOCKS_PER_PERSON = 200;
/** People whose blocks are read at the same time (no cap on the number of people). */
const BLOCK_READ_CONCURRENCY = 20;
/** Id space of the "availability unknown" bars (never collides with block ids). */
const UNKNOWN_ITEM_ID_BASE = -1_000_000;

/** Overlaps the horizon [from, to] (local days). */
function inHorizon(b: PersonnelAvailabilityBlock, dateFrom: string, dateTo: string): boolean {
  const s = parseBlockTimeMs(b.start_at);
  const e = parseBlockTimeMs(b.end_at);
  if (s == null || e == null) return false;
  return e >= Date.parse(`${dateFrom}T00:00:00`) && s <= Date.parse(`${dateTo}T23:59:59`);
}

/**
 * A full page (newest first) whose oldest block still starts inside or after the horizon may hide
 * older blocks of the horizon: the person's availability is then not known for sure.
 */
function pageMayHideHorizon(blocks: PersonnelAvailabilityBlock[], dateFrom: string): boolean {
  if (blocks.length < BLOCKS_PER_PERSON) return false;
  const oldest = Math.min(...blocks.map((b) => parseBlockTimeMs(b.start_at) ?? Infinity));
  return oldest >= Date.parse(`${dateFrom}T00:00:00`);
}

export const AvailabilityCalendar = forwardRef<
  AvailabilityCalendarHandle,
  AvailabilityCalendarProps
>(function AvailabilityCalendar(
  { entityId, onOpenPersonnel, viewMode: viewModeProp, onViewModeChange },
  ref,
) {
  const { t } = useTranslation("personnel");
  // Absence types (and their behaviour tag) come from the plant's list (Données de référence).
  const lists = usePersonnelLists();
  // Filtering by absence type needs the sensitive-data right: without it the server sends no type.
  const { can } = usePermissions();
  const canFilterByType = can(P.PER_SENSITIVEVIEW);

  const [dateFrom, setDateFrom] = useState(() => toLocalIsoDate(new Date()));
  const [dateTo, setDateTo] = useState(() =>
    addDaysLocal(toLocalIsoDate(new Date()), AVAILABILITY_DEFAULT_HORIZON_DAYS - 1),
  );
  const [filterEntityId, setFilterEntityId] = useState<number | null>(entityId ?? null);
  const [filterTeamId, setFilterTeamId] = useState<number | null>(null);
  const [filterPersonId, setFilterPersonId] = useState<number | null>(null);
  const [filterBlockType, setFilterBlockType] = useState<string | null>(null);
  const [criticalOnly, setCriticalOnly] = useState(false);
  const [internalViewMode, setInternalViewMode] = useState<AvailabilityViewMode>("chronology");
  const viewMode = viewModeProp ?? internalViewMode;
  const setViewMode = useCallback(
    (mode: AvailabilityViewMode) => {
      onViewModeChange?.(mode);
      if (viewModeProp == null) setInternalViewMode(mode);
    },
    [onViewModeChange, viewModeProp],
  );

  const [rows, setRows] = useState<AvailabilityCalendarEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [reloadToken, setReloadToken] = useState(0);

  const [entityNodes, setEntityNodes] = useState<OrgDesignerNodeRow[]>([]);
  const [teamOptions, setTeamOptions] = useState<{ id: number; label: string }[]>([]);
  const [certWarnings, setCertWarnings] = useState<Map<number, CertWarning>>(new Map());

  const [selectedPersonId, setSelectedPersonId] = useState<number | null>(null);
  const [selectedBlockId, setSelectedBlockId] = useState<number | null>(null);
  const [personBlocks, setPersonBlocks] = useState<PersonnelAvailabilityBlock[]>([]);
  const [allBlocks, setAllBlocks] = useState<PersonnelAvailabilityBlock[]>([]);
  const [blocksLoading, setBlocksLoading] = useState(false);
  const [blocksError, setBlocksError] = useState<string | null>(null);
  /** People whose blocks of the horizon are not known (read failed or page incomplete). */
  const [unknownPeople, setUnknownPeople] = useState<Set<number>>(new Set());
  const [firstBlockError, setFirstBlockError] = useState<unknown>(null);

  const [blockDialogOpen, setBlockDialogOpen] = useState(false);
  const [blockDialogMode, setBlockDialogMode] = useState<"create" | "edit">("create");
  const [editingBlock, setEditingBlock] = useState<PersonnelAvailabilityBlock | null>(null);
  const [createSeedPersonnelId, setCreateSeedPersonnelId] = useState<number | null>(null);
  const [createSeedStart, setCreateSeedStart] = useState("");
  const [createSeedEnd, setCreateSeedEnd] = useState("");

  const [visibleStart, setVisibleStart] = useState(() => Date.parse(`${dateFrom}T00:00:00`));
  const [visibleEnd, setVisibleEnd] = useState(() => Date.parse(`${dateTo}T23:59:59`));

  useEffect(() => {
    if (entityId != null) setFilterEntityId(entityId);
  }, [entityId]);

  useEffect(() => {
    setVisibleStart(Date.parse(`${dateFrom}T00:00:00`));
    setVisibleEnd(Date.parse(`${dateTo}T23:59:59`));
  }, [dateFrom, dateTo]);

  useEffect(() => {
    let cancelled = false;
    void getOrgDesignerSnapshot()
      .then((snap) => {
        if (cancelled) return;
        const nodes = snap.nodes.filter((n) => n.status === "active");
        const entityCandidates = nodes.filter((n) => n.active_binding_count > 0);
        setEntityNodes(
          entityCandidates.length > 0 ? entityCandidates : nodes.filter((n) => n.can_own_work),
        );
      })
      .catch(() => {
        if (!cancelled) setEntityNodes([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const loadCalendar = useCallback(() => {
    setReloadToken((v) => v + 1);
  }, []);

  const openCreateDialog = useCallback((personId?: number | null, date?: string) => {
    setBlockDialogMode("create");
    setEditingBlock(null);
    setCreateSeedPersonnelId(personId ?? null);
    if (date) {
      setCreateSeedStart(`${date}T${AVAILABILITY_BLOCK_SEED_START_LOCAL}`);
      setCreateSeedEnd(`${date}T${AVAILABILITY_BLOCK_SEED_END_LOCAL}`);
    } else {
      setCreateSeedStart("");
      setCreateSeedEnd("");
    }
    setBlockDialogOpen(true);
  }, []);

  useImperativeHandle(
    ref,
    () => ({
      refresh: loadCalendar,
      openCreateBlock: () => openCreateDialog(),
      setViewMode,
      getViewMode: () => viewMode,
    }),
    [loadCalendar, openCreateDialog, setViewMode, viewMode],
  );

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listAvailabilityCalendar({
      date_from: dateFrom,
      date_to: dateTo,
      entity_id: filterEntityId,
      team_id: filterTeamId,
      personnel_id: filterPersonId,
      include_inactive: false,
    })
      .then((data) => {
        if (!cancelled) setRows(data);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [dateFrom, dateTo, filterEntityId, filterTeamId, filterPersonId, reloadToken]);

  useEffect(() => {
    let cancelled = false;
    void listTeamCapacitySummary({
      date_from: dateFrom,
      date_to: dateTo,
      entity_id: filterEntityId,
      include_inactive: false,
    })
      .then((data) => {
        if (cancelled) return;
        setTeamOptions(
          data.map((row) => ({
            id: row.team_id,
            label: `${row.team_name} (${row.team_code})`,
          })),
        );
      })
      .catch(() => {
        if (!cancelled) setTeamOptions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [dateFrom, dateTo, filterEntityId]);

  useEffect(() => {
    let cancelled = false;
    void listCertificationExpiryDrilldown(CERT_LOOKAHEAD_DAYS, filterEntityId ?? undefined)
      .then((rows) => {
        if (cancelled) return;
        const map = new Map<number, CertWarning>();
        const today = toLocalIsoDate(new Date());
        for (const row of rows) {
          const expires = row.expires_at?.slice(0, 10) ?? "";
          if (!expires) continue;
          map.set(row.personnel_id, expires < today ? "expired" : "expiring");
        }
        setCertWarnings(map);
      })
      .catch(() => {
        if (!cancelled) setCertWarnings(new Map());
      });
    return () => {
      cancelled = true;
    };
  }, [filterEntityId, reloadToken]);

  const dates = useMemo(() => eachDateInclusive(dateFrom, dateTo), [dateFrom, dateTo]);
  const people = useMemo(() => buildPersonRows(rows), [rows]);

  const personOptions = useMemo(
    () =>
      people.map((p) => ({
        id: p.personnel_id,
        label: `${p.full_name} (${p.employee_code})`,
      })),
    [people],
  );

  const filteredPeople = useMemo(() => {
    return people.filter((person) => {
      if (criticalOnly) {
        const hasCritical = dates.some((d) => {
          const cell = person.cells.get(d);
          return cell ? deriveCellState(cell) === "critical" : false;
        });
        if (!hasCritical) return false;
      }
      if (filterBlockType) {
        // Filtered after blocks load via timeline items; keep person if any day blocked when type filter active
        // Person-level block-type filter applied when allBlocks known
      }
      return true;
    });
  }, [people, criticalOnly, dates, filterBlockType]);

  useEffect(() => {
    let cancelled = false;
    if (filteredPeople.length === 0) {
      setAllBlocks([]);
      setUnknownPeople(new Set());
      setFirstBlockError(null);
      return;
    }
    void (async () => {
      const flat: PersonnelAvailabilityBlock[] = [];
      const unknown = new Set<number>();
      let firstError: unknown = null;
      for (let i = 0; i < filteredPeople.length; i += BLOCK_READ_CONCURRENCY) {
        const batch = filteredPeople.slice(i, i + BLOCK_READ_CONCURRENCY);
        const results = await Promise.allSettled(
          batch.map((p) => listPersonnelAvailabilityBlocks(p.personnel_id, BLOCKS_PER_PERSON)),
        );
        if (cancelled) return;
        results.forEach((res, k) => {
          const person = batch[k];
          if (!person) return;
          if (res.status === "rejected") {
            unknown.add(person.personnel_id);
            firstError ??= res.reason;
            return;
          }
          if (pageMayHideHorizon(res.value, dateFrom)) unknown.add(person.personnel_id);
          flat.push(...res.value.filter((b) => inHorizon(b, dateFrom, dateTo)));
        });
      }
      if (cancelled) return;
      setAllBlocks(flat);
      setUnknownPeople(unknown);
      setFirstBlockError(firstError);
    })();
    return () => {
      cancelled = true;
    };
  }, [filteredPeople, dateFrom, dateTo, reloadToken]);

  const peopleForView = useMemo(() => {
    if (!filterBlockType) return filteredPeople;
    const ids = new Set(
      allBlocks.filter((b) => b.block_type === filterBlockType).map((b) => b.personnel_id),
    );
    return filteredPeople.filter((p) => ids.has(p.personnel_id));
  }, [filteredPeople, filterBlockType, allBlocks]);

  const selectedPerson = useMemo(
    () => peopleForView.find((p) => p.personnel_id === selectedPersonId) ?? null,
    [peopleForView, selectedPersonId],
  );

  const selectedBlock = useMemo(
    () => allBlocks.find((b) => b.id === selectedBlockId) ?? null,
    [allBlocks, selectedBlockId],
  );

  useEffect(() => {
    if (selectedPersonId == null) {
      setPersonBlocks([]);
      return;
    }
    let cancelled = false;
    setBlocksLoading(true);
    setBlocksError(null);
    void listPersonnelAvailabilityBlocks(selectedPersonId, BLOCKS_PER_PERSON)
      .then((blocks) => {
        if (cancelled) return;
        setPersonBlocks(blocks.filter((b) => inHorizon(b, dateFrom, dateTo)));
        if (pageMayHideHorizon(blocks, dateFrom)) setBlocksError(t("availability.partialBlocks"));
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          // Never an empty list (= "available"): the inspector shows why nothing is listed.
          setPersonBlocks([]);
          setBlocksError(appErrorReason(err));
        }
      })
      .finally(() => {
        if (!cancelled) setBlocksLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedPersonId, dateFrom, dateTo, reloadToken, t]);

  const groups: ResourceTimelineGroup[] = useMemo(
    () =>
      peopleForView.map((p) => ({
        id: p.personnel_id,
        title: (
          <button
            type="button"
            className="flex h-full w-full min-w-0 flex-col justify-center gap-0 text-left leading-tight hover:opacity-90"
            onClick={() => {
              setSelectedPersonId(p.personnel_id);
              setSelectedBlockId(null);
            }}
          >
            <span className="truncate text-xs font-medium leading-4 text-text-primary">
              {formatPersonLabel(p.full_name)}
            </span>
            <span className="flex min-w-0 items-center gap-1">
              <span className="truncate font-mono text-[11px] leading-4 text-text-muted">
                {p.employee_code}
              </span>
              {unknownPeople.has(p.personnel_id) ? (
                <Badge variant="destructive" className="h-4 shrink-0 px-1 text-[10px] leading-none">
                  {t("availability.badges.unknown")}
                </Badge>
              ) : null}
              {certWarnings.get(p.personnel_id) === "expired" ? (
                <Badge variant="destructive" className="h-4 shrink-0 px-1 text-[10px] leading-none">
                  {t("availability.badges.certExpired")}
                </Badge>
              ) : certWarnings.get(p.personnel_id) === "expiring" ? (
                <Badge variant="secondary" className="h-4 shrink-0 px-1 text-[10px] leading-none">
                  {t("availability.badges.certExpiring")}
                </Badge>
              ) : null}
            </span>
          </button>
        ),
      })),
    [peopleForView, certWarnings, unknownPeople, t],
  );

  const items: ResourceTimelineItem[] = useMemo(() => {
    const personIds = new Set(peopleForView.map((p) => p.personnel_id));
    // A person whose blocks are not known is covered by one "unknown" bar: never shown free.
    const unknownBars: ResourceTimelineItem[] = peopleForView
      .filter((p) => unknownPeople.has(p.personnel_id))
      .map((p) => ({
        id: UNKNOWN_ITEM_ID_BASE - p.personnel_id,
        group: p.personnel_id,
        title: t("availability.unknownBar"),
        start_time: Date.parse(`${dateFrom}T00:00:00`),
        end_time: Date.parse(`${dateTo}T23:59:59`),
        state: "blocked" as const,
      }));
    const blockBars = allBlocks
      .filter((b) => personIds.has(b.personnel_id))
      .filter((b) => (filterBlockType ? b.block_type === filterBlockType : true))
      .map((b) => {
        const start = parseBlockTimeMs(b.start_at) ?? 0;
        const end = parseBlockTimeMs(b.end_at) ?? start + 3600000;
        return {
          id: b.id,
          group: b.personnel_id,
          title: absenceBlockTitle(b, lists, t),
          start_time: start,
          end_time: Math.max(end, start + 30 * 60 * 1000),
          state: absenceBlockState(b, lists),
        };
      });
    return [...unknownBars, ...blockBars];
  }, [allBlocks, peopleForView, filterBlockType, unknownPeople, dateFrom, dateTo, t, lists]);

  const applyHorizonPreset = (days: number) => {
    const from = toLocalIsoDate(new Date());
    setDateFrom(from);
    setDateTo(addDaysLocal(from, days - 1));
  };

  const resetFilters = () => {
    const from = toLocalIsoDate(new Date());
    setDateFrom(from);
    setDateTo(addDaysLocal(from, AVAILABILITY_DEFAULT_HORIZON_DAYS - 1));
    setFilterEntityId(entityId ?? null);
    setFilterTeamId(null);
    setFilterPersonId(null);
    setFilterBlockType(null);
    setCriticalOnly(false);
    setSelectedPersonId(null);
    setSelectedBlockId(null);
  };

  const filterDefs: SmartFilterDef[] = useMemo(
    () => [
      {
        id: "entity",
        kind: "select",
        label: t("filters.entity"),
        value: filterEntityId != null ? String(filterEntityId) : null,
        options: entityNodes.map((n) => ({
          value: String(n.node_id),
          label: n.code ? `${n.code} — ${n.name}` : n.name,
        })),
        onChange: (v) => setFilterEntityId(v ? Number(v) : null),
      },
      {
        id: "team",
        kind: "select",
        label: t("filters.team"),
        value: filterTeamId != null ? String(filterTeamId) : null,
        options: teamOptions.map((o) => ({ value: String(o.id), label: o.label })),
        onChange: (v) => setFilterTeamId(v ? Number(v) : null),
      },
      {
        id: "person",
        kind: "select",
        label: t("availability.filters.person"),
        value: filterPersonId != null ? String(filterPersonId) : null,
        options: personOptions.map((o) => ({ value: String(o.id), label: o.label })),
        onChange: (v) => setFilterPersonId(v ? Number(v) : null),
      },
      ...(canFilterByType
        ? [
            {
              id: "blockType",
              kind: "select" as const,
              label: t("availability.filters.blockType"),
              value: filterBlockType,
              options: lists.values("absence_types").map((v) => ({
                value: v.code,
                label: v.review_required
                  ? `${lists.text("absence_types", v.code)} · ${t("common:listValue.review")}`
                  : lists.text("absence_types", v.code),
              })),
              onChange: (v: string | null) => setFilterBlockType(v || null),
            },
          ]
        : []),
      {
        id: "critical",
        kind: "select",
        label: t("availability.filters.critical"),
        value: criticalOnly ? CRITICAL_FILTER_VALUE : null,
        options: [{ value: CRITICAL_FILTER_VALUE, label: t("availability.filters.criticalOnly") }],
        onChange: (v) => setCriticalOnly(v === CRITICAL_FILTER_VALUE),
      },
    ],
    [
      t,
      lists,
      filterEntityId,
      filterTeamId,
      filterPersonId,
      filterBlockType,
      canFilterByType,
      criticalOnly,
      entityNodes,
      teamOptions,
      personOptions,
    ],
  );

  const detailOpen = selectedPerson != null;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-text-muted">{t("availability.from")}</span>
        <DateField
          className="h-8 w-auto text-sm"
          value={dateFrom}
          onChange={(v) => setDateFrom(v)}
        />
        <span className="text-xs text-text-muted">{t("availability.to")}</span>
        <DateField className="h-8 w-auto text-sm" value={dateTo} onChange={(v) => setDateTo(v)} />
        {AVAILABILITY_HORIZON_PRESETS.map((days) => (
          <Button
            key={days}
            type="button"
            size="sm"
            variant="outline"
            className="h-8"
            onClick={() => applyHorizonPreset(days)}
          >
            {t("availability.horizonPreset", { days })}
          </Button>
        ))}
      </div>

      <SmartFilterBar
        searchPlaceholder=""
        searchValue=""
        onSearchChange={() => undefined}
        showSearch={false}
        filters={filterDefs}
        resultCount={peopleForView.length}
        onReset={resetFilters}
      />

      <div className="flex flex-wrap gap-1.5">
        {viewMode === "chronology"
          ? RESOURCE_TIMELINE_BLOCK_LEGEND_STATES.map((state) => (
              <span
                key={state}
                className={cn(
                  "rounded border px-1.5 py-0.5 text-[10px]",
                  RESOURCE_TIMELINE_STATE_CLASS[state],
                )}
              >
                {t(`availability.legend.${state}`)}
              </span>
            ))
          : (Object.keys(CELL_STATE_CLASS) as Array<keyof typeof CELL_STATE_CLASS>).map((state) => (
              <span
                key={state}
                className={cn("rounded border px-1.5 py-0.5 text-[10px]", CELL_STATE_CLASS[state])}
              >
                {t(`availability.cell.${state}`)}
              </span>
            ))}
      </div>

      {unknownPeople.size > 0 ? (
        <ErrorState
          error={firstBlockError ?? t("availability.partialBlocks")}
          title={t("availability.unknownTitle", { count: unknownPeople.size })}
          onRetry={loadCalendar}
          className="py-4"
        />
      ) : null}

      <div className={cn(mfLayout.moduleWorkspaceSplit, "min-h-0 flex-1")}>
        <div
          className={cn(
            "flex min-h-0 min-w-0 flex-col",
            viewMode === "chronology" ? "overflow-hidden" : "overflow-auto",
            detailOpen ? "w-[55%] border-r border-surface-border pr-3" : "w-full",
          )}
        >
          {viewMode === "chronology" ? (
            <ResourceTimeline
              groups={groups}
              items={items}
              horizonStart={Date.parse(`${dateFrom}T00:00:00`)}
              horizonEnd={Date.parse(`${dateTo}T23:59:59`)}
              visibleTimeStart={visibleStart}
              visibleTimeEnd={visibleEnd}
              onVisibleTimeChange={(s, e) => {
                setVisibleStart(s);
                setVisibleEnd(e);
              }}
              onItemSelect={(id) => {
                const block = allBlocks.find((b) => b.id === id);
                if (!block) return;
                setSelectedBlockId(block.id);
                setSelectedPersonId(block.personnel_id);
              }}
              onGroupClick={(groupId) => {
                setSelectedPersonId(Number(groupId));
                setSelectedBlockId(null);
              }}
              {...(selectedBlockId != null ? { selectedItemIds: [selectedBlockId] } : {})}
              loading={loading}
              error={error != null ? appErrorReason(error) : null}
              emptyTitle={t("availability.empty")}
              className="min-h-0 flex-1"
            />
          ) : (
            <div className="space-y-2 overflow-auto text-sm">
              {error != null ? (
                <ErrorState error={error} onRetry={loadCalendar} />
              ) : loading ? (
                <p className="text-text-muted">{t("common.loading")}</p>
              ) : peopleForView.length === 0 ? (
                <p className="text-text-muted">{t("availability.empty")}</p>
              ) : (
                peopleForView.map((person) => (
                  <button
                    key={person.personnel_id}
                    type="button"
                    className="w-full rounded-md border border-surface-border px-3 py-2 text-left hover:bg-surface-2"
                    onClick={() => {
                      setSelectedPersonId(person.personnel_id);
                      setSelectedBlockId(null);
                    }}
                  >
                    <div className="font-medium">{formatPersonLabel(person.full_name)}</div>
                    <div className="text-xs text-text-muted">{person.employee_code}</div>
                    {unknownPeople.has(person.personnel_id) ? (
                      <div className="mt-1 text-xs font-medium text-tone-danger-fg">
                        {t("availability.unknownBar")}
                      </div>
                    ) : null}
                    <div className="mt-1 flex flex-wrap gap-1">
                      {dates.map((d) => {
                        const cell = person.cells.get(d);
                        if (!cell) return null;
                        const state = deriveCellState(cell);
                        return (
                          <span
                            key={d}
                            className={cn(
                              "rounded border px-1 text-[10px]",
                              CELL_STATE_CLASS[state],
                            )}
                          >
                            {d.slice(5)} · {t(`availability.cell.${state}`)}
                          </span>
                        );
                      })}
                    </div>
                  </button>
                ))
              )}
            </div>
          )}
        </div>

        {detailOpen && selectedPerson ? (
          <div className="min-w-[280px] flex-1 overflow-auto pl-3">
            <AvailabilityInspectorWorkspace
              person={selectedPerson}
              selectedBlock={selectedBlock}
              personBlocks={personBlocks}
              blocksLoading={blocksLoading}
              blocksError={blocksError}
              dateFrom={dateFrom}
              dateTo={dateTo}
              certWarning={certWarnings.get(selectedPerson.personnel_id) ?? null}
              onClose={() => {
                setSelectedPersonId(null);
                setSelectedBlockId(null);
              }}
              onSelectBlock={setSelectedBlockId}
              onCreateBlock={(personnelId, date) => openCreateDialog(personnelId, date)}
              onEditBlock={(block) => {
                setBlockDialogMode("edit");
                setEditingBlock(block);
                setBlockDialogOpen(true);
              }}
              {...(onOpenPersonnel ? { onOpenPersonnel } : {})}
              onBlocksChanged={loadCalendar}
            />
          </div>
        ) : null}
      </div>

      <AvailabilityBlockDialog
        open={blockDialogOpen}
        onOpenChange={setBlockDialogOpen}
        mode={blockDialogMode}
        editingBlock={editingBlock}
        initialPersonnelId={createSeedPersonnelId}
        initialStartLocal={createSeedStart}
        initialEndLocal={createSeedEnd}
        personOptions={people.map((p) => ({
          id: p.personnel_id,
          label: `${p.full_name} (${p.employee_code})`,
        }))}
        onSaved={() => {
          setBlockDialogOpen(false);
          loadCalendar();
        }}
      />
    </div>
  );
});
