/**
 * TeamDetailWorkspace — split-pane crew detail (Directory/Equipment pattern).
 * Tabs: Overview, Roster, Capacity, Skills (heatmap), Succession.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import {
  DetailSectionCard,
  EntityDetailHeader,
  ENTITY_DETAIL_HEADER_ACTION_BTN,
} from "@/components/detail";
import { CrewBadge } from "@/components/personnel/CrewBadge";
import { TeamSkillsHeatmap } from "@/components/personnel/TeamSkillsHeatmap";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { formatOrDash, formatPersonLabel } from "@/lib/display";
import {
  getTeam,
  listSkillsMatrix,
  listTeamMembers,
  scanSuccessionRisk,
  setTeamLead,
} from "@/services/personnel-service";
import { listTeamCapacityLoad } from "@/services/planning-service";
import type {
  SkillMatrixRow,
  SuccessionRiskRow,
  TeamCapacityLoad,
  TeamCapacitySummaryRow,
  TeamMemberRow,
  TeamSummary,
} from "@shared/ipc-types";

export type TeamDetailWorkspaceProps = {
  teamId: number;
  dateFrom: string;
  dateTo: string;
  capacity: TeamCapacitySummaryRow | null;
  onClose: () => void;
  onEdit?: ((team: TeamSummary) => void) | undefined;
  onOpenPersonnel?: ((personnelId: number) => void) | undefined;
  /** Called after roster lead changes so the parent can refresh lead_count. */
  onRosterChanged?: (() => void) | undefined;
  canManage?: boolean | undefined;
};

export function TeamDetailWorkspace({
  teamId,
  dateFrom,
  dateTo,
  capacity,
  onClose,
  onEdit,
  onOpenPersonnel,
  onRosterChanged,
  canManage = false,
}: TeamDetailWorkspaceProps) {
  const { t } = useTranslation("personnel");
  const [team, setTeam] = useState<TeamSummary | null>(null);
  const [members, setMembers] = useState<TeamMemberRow[]>([]);
  const [skills, setSkills] = useState<SkillMatrixRow[]>([]);
  const [risks, setRisks] = useState<SuccessionRiskRow[]>([]);
  const [dailyLoad, setDailyLoad] = useState<TeamCapacityLoad[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [skillsError, setSkillsError] = useState<string | null>(null);
  const [capacityNote, setCapacityNote] = useState<string | null>(null);
  const [tab, setTab] = useState("overview");
  const [leadBusyId, setLeadBusyId] = useState<number | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    setSkillsError(null);
    setCapacityNote(null);
    try {
      const [crew, roster, matrix, succession] = await Promise.all([
        getTeam(teamId),
        listTeamMembers(teamId),
        listSkillsMatrix({ team_id: teamId, include_inactive: false }).catch((err: unknown) => {
          setSkillsError(err instanceof Error ? err.message : String(err));
          return [] as SkillMatrixRow[];
        }),
        scanSuccessionRisk(null, teamId),
      ]);
      setTeam(crew);
      setMembers(roster);
      setSkills(matrix);
      setRisks(succession);

      try {
        const periodStart = `${dateFrom}T00:00:00Z`;
        const periodEnd = `${dateTo}T23:59:59Z`;
        const loadRows = await listTeamCapacityLoad(periodStart, periodEnd, teamId);
        setDailyLoad(loadRows.filter((r) => r.team_id === teamId));
      } catch {
        setDailyLoad([]);
        setCapacityNote(t("crews.detail.capacityLoadUnavailable"));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setTeam(null);
    } finally {
      setLoading(false);
    }
  }, [teamId, dateFrom, dateTo, t]);

  useEffect(() => {
    setTab("overview");
    void load();
  }, [load]);

  const handleLeadToggle = useCallback(
    async (member: TeamMemberRow, nextIsLead: boolean) => {
      setLeadBusyId(member.personnel_id);
      setError(null);
      try {
        await setTeamLead({
          team_id: teamId,
          personnel_id: member.personnel_id,
          is_lead: nextIsLead,
        });
        await load();
        onRosterChanged?.();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLeadBusyId(null);
      }
    },
    [teamId, load, onRosterChanged],
  );

  const siteLabel = useMemo(() => {
    if (!team) return "—";
    if (team.node_code && team.node_name) return `${team.node_code} — ${team.node_name}`;
    return team.node_name ?? team.node_code ?? "—";
  }, [team]);

  const memberColumns = useMemo<ColumnDef<TeamMemberRow>[]>(() => {
    const cols: ColumnDef<TeamMemberRow>[] = [
      {
        accessorKey: "employee_code",
        header: t("list.columns.code"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.employee_code}</span>,
      },
      {
        accessorKey: "full_name",
        header: t("list.columns.fullName"),
        cell: ({ row }) => (
          <span className="font-medium">{formatPersonLabel(row.original.full_name)}</span>
        ),
      },
      {
        accessorKey: "position_name",
        header: t("list.columns.position"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {formatOrDash(row.original.position_name)}
          </span>
        ),
      },
      {
        id: "lead",
        header: t("crews.detail.lead"),
        cell: ({ row }) =>
          row.original.is_lead ? (
            <Badge variant="secondary" className="text-[10px]">
              {t("crews.detail.lead")}
            </Badge>
          ) : (
            <span className="text-xs text-text-muted">—</span>
          ),
      },
      {
        accessorKey: "role_code",
        header: t("crews.detail.role"),
        cell: ({ row }) => <span className="text-xs">{row.original.role_code}</span>,
      },
      {
        accessorKey: "allocation_percent",
        header: t("crews.detail.allocation"),
        cell: ({ row }) => (
          <span className="text-xs">{Math.round(row.original.allocation_percent)}%</span>
        ),
      },
    ];

    if (canManage) {
      cols.push({
        id: "leadActions",
        header: t("list.columns.actions"),
        cell: ({ row }) => {
          const member = row.original;
          const isLead = Boolean(member.is_lead);
          const busy = leadBusyId === member.personnel_id;
          return (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-7 text-xs"
              disabled={busy || leadBusyId != null}
              onClick={(e) => {
                e.stopPropagation();
                void handleLeadToggle(member, !isLead);
              }}
            >
              {isLead ? t("crews.actions.clearLead") : t("crews.actions.setLead")}
            </Button>
          );
        },
      });
    }

    return cols;
  }, [t, canManage, leadBusyId, handleLeadToggle]);

  const riskColumns = useMemo<ColumnDef<SuccessionRiskRow>[]>(
    () => [
      {
        accessorKey: "full_name",
        header: t("list.columns.fullName"),
        cell: ({ row }) => formatPersonLabel(row.original.full_name),
      },
      {
        accessorKey: "position_name",
        header: t("list.columns.position"),
        cell: ({ row }) => formatOrDash(row.original.position_name),
      },
      {
        accessorKey: "risk_level",
        header: t("crews.detail.riskLevel"),
        cell: ({ row }) => (
          <Badge variant="outline" className="text-[10px]">
            {row.original.risk_level}
          </Badge>
        ),
      },
      {
        accessorKey: "coverage_count",
        header: t("crews.detail.coverageCount"),
      },
      {
        accessorKey: "reason",
        header: t("crews.detail.reason"),
        cell: ({ row }) => (
          <span className="max-w-[240px] truncate text-xs text-text-muted">
            {row.original.reason}
          </span>
        ),
      },
    ],
    [t],
  );

  const loadColumns = useMemo<ColumnDef<TeamCapacityLoad>[]>(
    () => [
      {
        accessorKey: "work_date",
        header: t("crews.detail.workDate"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.work_date}</span>,
      },
      {
        accessorKey: "available_hours",
        header: t("crews.detail.availableHours"),
        cell: ({ row }) => row.original.available_hours.toFixed(1),
      },
      {
        accessorKey: "committed_hours",
        header: t("crews.detail.committedHours"),
        cell: ({ row }) => row.original.committed_hours.toFixed(1),
      },
      {
        accessorKey: "utilization_ratio",
        header: t("crews.detail.utilization"),
        cell: ({ row }) => `${Math.round(row.original.utilization_ratio * 100)}%`,
      },
    ],
    [t],
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="border-b border-surface-border px-4 py-3">
        {team ? (
          <EntityDetailHeader
            code={team.code}
            designation={team.name}
            qrSlot={
              <CrewBadge
                code={team.code}
                name={team.name}
                iconKey={team.icon_key}
                accentColor={team.accent_color}
                size="lg"
              />
            }
            statusSlot={
              <div className="flex flex-wrap gap-1.5">
                <Badge variant="outline" className="text-[10px]">
                  {t(`crews.types.${team.team_type}`, team.team_type)}
                </Badge>
                <Badge variant="secondary" className="text-[10px]">
                  {t(`crews.status.${team.status}`, team.status)}
                </Badge>
              </div>
            }
            primaryActionsSlot={
              <>
                {canManage && onEdit ? (
                  <Button size="sm" variant="outline" onClick={() => onEdit(team)}>
                    {t("crews.actions.edit")}
                  </Button>
                ) : null}
                <Button
                  variant="outline"
                  size="sm"
                  className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                  onClick={onClose}
                  aria-label={t("detail.actions.close")}
                >
                  <X className="h-3.5 w-3.5" />
                  {t("detail.actions.close")}
                </Button>
              </>
            }
          />
        ) : (
          <p className="text-sm text-text-muted">
            {loading ? t("common.loading") : t("crews.detail.notFound")}
          </p>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {error ? <div className="mb-3 text-sm text-destructive">{error}</div> : null}
        {team ? (
          <Tabs value={tab} onValueChange={setTab}>
            <TabsList className="mb-3 w-full justify-start">
              <TabsTrigger value="overview" className="text-xs">
                {t("crews.detail.tabs.overview")}
              </TabsTrigger>
              <TabsTrigger value="roster" className="text-xs">
                {t("crews.detail.tabs.roster")}
              </TabsTrigger>
              <TabsTrigger value="capacity" className="text-xs">
                {t("crews.detail.tabs.capacity")}
              </TabsTrigger>
              <TabsTrigger value="skills" className="text-xs">
                {t("crews.detail.tabs.skills")}
              </TabsTrigger>
              <TabsTrigger value="succession" className="text-xs">
                {t("crews.detail.tabs.succession")}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="overview" className="space-y-3">
              <DetailSectionCard title={t("crews.detail.tabs.overview")}>
                <dl className="grid grid-cols-1 gap-2 text-sm sm:grid-cols-2">
                  <div>
                    <dt className="text-xs text-text-muted">{t("crews.form.site")}</dt>
                    <dd>{siteLabel}</dd>
                  </div>
                  <div>
                    <dt className="text-xs text-text-muted">{t("crews.form.description")}</dt>
                    <dd>{formatOrDash(team.description)}</dd>
                  </div>
                </dl>
              </DetailSectionCard>
              <DetailSectionCard title={t("crews.detail.horizonKpis")}>
                {capacity ? (
                  <dl className="grid grid-cols-2 gap-2 text-sm sm:grid-cols-4">
                    <div>
                      <dt className="text-xs text-text-muted">{t("crews.list.members")}</dt>
                      <dd className="font-medium">{capacity.member_count}</dd>
                    </div>
                    <div>
                      <dt className="text-xs text-text-muted">{t("crews.list.leads")}</dt>
                      <dd className="font-medium">{capacity.lead_count}</dd>
                    </div>
                    <div>
                      <dt className="text-xs text-text-muted">{t("crews.list.availableMin")}</dt>
                      <dd className="font-medium">{capacity.total_available_minutes}</dd>
                    </div>
                    <div>
                      <dt className="text-xs text-text-muted">{t("crews.list.blockedMin")}</dt>
                      <dd className="font-medium">{capacity.total_blocked_minutes}</dd>
                    </div>
                  </dl>
                ) : (
                  <p className="text-sm text-text-muted">{t("crews.noCapacityInHorizon")}</p>
                )}
              </DetailSectionCard>
            </TabsContent>

            <TabsContent value="roster">
              {members.length === 0 ? (
                <EmptyState title={t("crews.detail.emptyRoster")} />
              ) : (
                <DataTable
                  columns={memberColumns}
                  data={members}
                  pageSize={10}
                  {...(onOpenPersonnel
                    ? {
                        onRowClick: (row: TeamMemberRow) => onOpenPersonnel(row.personnel_id),
                      }
                    : {})}
                />
              )}
            </TabsContent>

            <TabsContent value="capacity" className="space-y-3">
              <DetailSectionCard title={t("crews.detail.horizonKpis")}>
                {capacity ? (
                  <p className="text-sm">
                    {t("crews.detail.capacitySummary", {
                      available: capacity.total_available_minutes,
                      blocked: capacity.total_blocked_minutes,
                      ratio: Math.round((capacity.avg_availability_ratio ?? 0) * 100),
                    })}
                  </p>
                ) : (
                  <p className="text-sm text-text-muted">{t("crews.noCapacityInHorizon")}</p>
                )}
                {capacityNote ? (
                  <p className="mt-2 text-xs text-text-muted">{capacityNote}</p>
                ) : null}
              </DetailSectionCard>
              {dailyLoad.length > 0 ? (
                <DataTable columns={loadColumns} data={dailyLoad} pageSize={10} />
              ) : null}
            </TabsContent>

            <TabsContent value="skills">
              <TeamSkillsHeatmap rows={skills} loading={loading} error={skillsError} />
            </TabsContent>

            <TabsContent value="succession">
              {risks.length === 0 ? (
                <EmptyState title={t("crews.detail.emptySuccession")} />
              ) : (
                <DataTable
                  columns={riskColumns}
                  data={risks}
                  pageSize={10}
                  {...(onOpenPersonnel
                    ? {
                        onRowClick: (row: SuccessionRiskRow) => onOpenPersonnel(row.personnel_id),
                      }
                    : {})}
                />
              )}
            </TabsContent>
          </Tabs>
        ) : null}
      </div>
    </div>
  );
}
