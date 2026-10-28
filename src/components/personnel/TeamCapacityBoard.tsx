import type { ColumnDef } from "@tanstack/react-table";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { CrestIconPicker } from "@/components/personnel/CrestIconPicker";
import { CrewBadge } from "@/components/personnel/CrewBadge";
import { TeamDetailWorkspace } from "@/components/personnel/TeamDetailWorkspace";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { mfInput, mfLayout } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorReason } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import { getOrgDesignerSnapshot } from "@/services/org-designer-service";
import {
  createTeam,
  listActiveTeams,
  listTeamCapacitySummary,
  updateTeam,
} from "@/services/personnel-service";
import type { OrgDesignerNodeRow, TeamCapacitySummaryRow, TeamSummary } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export type TeamCapacityBoardHandle = {
  refresh: () => void;
  openCreate: () => void;
};

interface TeamCapacityBoardProps {
  entityId?: number | null;
  onOpenPersonnel?: ((personnelId: number) => void) | undefined;
  /** Team opened on its capacity tab (planning's "set up the team's Classe horaire" link). */
  openTeamId?: number | null;
}

/** Capacity figures are null when the summary has no row for the team: unknown, not 0. */
type CrewTableRow = TeamSummary & {
  member_count: number | null;
  lead_count: number | null;
  total_available_minutes: number | null;
  total_blocked_minutes: number | null;
  avg_availability_ratio: number | null;
};

const TEAM_TYPES = ["maintenance", "inspection", "planning", "contractor", "hse"] as const;

function toIsoDate(value: Date): string {
  return value.toISOString().slice(0, 10);
}

export const TeamCapacityBoard = forwardRef<TeamCapacityBoardHandle, TeamCapacityBoardProps>(
  function TeamCapacityBoard({ entityId, onOpenPersonnel, openTeamId }, ref) {
    const { t } = useTranslation("personnel");
    const { can } = usePermissions();
    const canManage = can(P.PER_MANAGE);

    const [rows, setRows] = useState<TeamCapacitySummaryRow[]>([]);
    const [crews, setCrews] = useState<TeamSummary[]>([]);
    const [loading, setLoading] = useState(false);
    const { formatNumber, formatPercent } = useFormatters();
    const [error, setError] = useState<unknown>(null);
    const [reloadToken, setReloadToken] = useState(0);
    const [dateFrom, setDateFrom] = useState(() => toIsoDate(new Date()));
    const [dateTo, setDateTo] = useState(() => {
      const d = new Date();
      d.setDate(d.getDate() + 6);
      return toIsoDate(d);
    });
    const [filterEntityId, setFilterEntityId] = useState<number | null>(entityId ?? null);
    const [entityNodes, setEntityNodes] = useState<OrgDesignerNodeRow[]>([]);
    const [searchInput, setSearchInput] = useState("");
    const [searchQuery, setSearchQuery] = useState("");

    const [dialogOpen, setDialogOpen] = useState(false);
    const [editingCrew, setEditingCrew] = useState<TeamSummary | null>(null);
    const [formCode, setFormCode] = useState("");
    const [formName, setFormName] = useState("");
    const [formType, setFormType] = useState<string>("maintenance");
    const [formNodeId, setFormNodeId] = useState<number | null>(null);
    const [formStatus, setFormStatus] = useState("active");
    const [formDescription, setFormDescription] = useState("");
    const [formIconKey, setFormIconKey] = useState("");
    const [formAccentColor, setFormAccentColor] = useState("");
    const [formError, setFormError] = useState<string | null>(null);
    const [formSaving, setFormSaving] = useState(false);

    const [detailTeamId, setDetailTeamId] = useState<number | null>(openTeamId ?? null);

    useEffect(() => {
      if (entityId != null) setFilterEntityId(entityId);
    }, [entityId]);

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

    const refresh = useCallback(() => {
      setReloadToken((v) => v + 1);
    }, []);

    const openCreate = useCallback(() => {
      setEditingCrew(null);
      setFormCode("");
      setFormName("");
      setFormType("maintenance");
      setFormNodeId(filterEntityId);
      setFormStatus("active");
      setFormDescription("");
      setFormIconKey("");
      setFormAccentColor("");
      setFormError(null);
      setDialogOpen(true);
    }, [filterEntityId]);

    useImperativeHandle(ref, () => ({ refresh, openCreate }), [refresh, openCreate]);

    useEffect(() => {
      let cancelled = false;
      setLoading(true);
      setError(null);
      void Promise.all([
        listTeamCapacitySummary({
          date_from: dateFrom,
          date_to: dateTo,
          entity_id: filterEntityId,
          include_inactive: false,
        }),
        listActiveTeams({
          entity_id: filterEntityId,
          include_inactive: true,
        }),
      ])
        .then(([capacity, crewList]) => {
          if (cancelled) return;
          setRows(capacity);
          setCrews(crewList);
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
    }, [dateFrom, dateTo, filterEntityId, reloadToken]);

    const capacityByTeam = useMemo(() => {
      const map = new Map<number, TeamCapacitySummaryRow>();
      for (const row of rows) map.set(row.team_id, row);
      return map;
    }, [rows]);

    const tableRows = useMemo<CrewTableRow[]>(() => {
      const q = searchQuery.trim().toLowerCase();
      return crews
        .filter(
          (row) =>
            !q ||
            row.name.toLowerCase().includes(q) ||
            row.code.toLowerCase().includes(q) ||
            (row.node_name ?? "").toLowerCase().includes(q) ||
            (row.node_code ?? "").toLowerCase().includes(q),
        )
        .map((crew) => {
          const capacity = capacityByTeam.get(crew.id);
          return {
            ...crew,
            member_count: capacity?.member_count ?? null,
            lead_count: capacity?.lead_count ?? null,
            total_available_minutes: capacity?.total_available_minutes ?? null,
            total_blocked_minutes: capacity?.total_blocked_minutes ?? null,
            avg_availability_ratio: capacity?.avg_availability_ratio ?? null,
          };
        });
    }, [crews, searchQuery, capacityByTeam]);

    const resetFilters = useCallback(() => {
      const from = toIsoDate(new Date());
      const to = new Date();
      to.setDate(to.getDate() + 6);
      setDateFrom(from);
      setDateTo(toIsoDate(to));
      setFilterEntityId(entityId ?? null);
      setSearchInput("");
      setSearchQuery("");
    }, [entityId]);

    const filterDefs = useMemo<SmartFilterDef[]>(
      () => [
        {
          id: "entity",
          kind: "select",
          label: t("filters.entity"),
          options: entityNodes.map((n) => ({
            value: String(n.node_id),
            label: `${n.code} — ${n.name}`,
          })),
          value: filterEntityId != null ? String(filterEntityId) : null,
          onChange: (val) => setFilterEntityId(val ? Number(val) : null),
        },
      ],
      [t, entityNodes, filterEntityId],
    );

    const openEdit = (crew: TeamSummary) => {
      setEditingCrew(crew);
      setFormCode(crew.code);
      setFormName(crew.name);
      setFormType(crew.team_type);
      setFormNodeId(crew.primary_node_id);
      setFormStatus(crew.status);
      setFormDescription(crew.description ?? "");
      setFormIconKey(crew.icon_key ?? "");
      setFormAccentColor(crew.accent_color ?? "");
      setFormError(null);
      setDialogOpen(true);
    };

    const openDetail = (crew: CrewTableRow) => {
      setDetailTeamId((prev) => (prev === crew.id ? null : crew.id));
    };

    const submitCrew = async () => {
      setFormError(null);
      if (!formCode.trim() || !formName.trim()) {
        setFormError(t("crews.form.errors.required"));
        return;
      }
      if (formNodeId == null) {
        setFormError(t("crews.form.errors.siteRequired"));
        return;
      }
      setFormSaving(true);
      try {
        if (editingCrew) {
          await updateTeam({
            id: editingCrew.id,
            code: formCode.trim(),
            name: formName.trim(),
            team_type: formType,
            primary_node_id: formNodeId,
            status: formStatus,
            description: formDescription.trim() || null,
            icon_key: formIconKey,
            accent_color: formAccentColor,
          });
        } else {
          await createTeam({
            code: formCode.trim(),
            name: formName.trim(),
            team_type: formType,
            primary_node_id: formNodeId,
            status: formStatus,
            description: formDescription.trim() || null,
            icon_key: formIconKey || null,
            accent_color: formAccentColor || null,
          });
        }
        setDialogOpen(false);
        refresh();
      } catch (err) {
        setFormError(appErrorReason(err));
      } finally {
        setFormSaving(false);
      }
    };

    const unknownOr = useCallback(
      (n: number | null) =>
        n == null ? (
          <span className="text-text-muted" title={t("crews.capacityUnknown")}>
            —
          </span>
        ) : (
          formatNumber(n)
        ),
      [formatNumber, t],
    );

    const columns = useMemo<ColumnDef<CrewTableRow>[]>(
      () => [
        {
          id: "crest",
          header: "",
          cell: ({ row }) => (
            <CrewBadge
              code={row.original.code}
              name={row.original.name}
              iconKey={row.original.icon_key}
              accentColor={row.original.accent_color}
              size="sm"
            />
          ),
        },
        {
          accessorKey: "code",
          header: t("crews.list.code"),
          cell: ({ row }) => <span className="font-mono text-xs">{row.original.code}</span>,
        },
        {
          accessorKey: "name",
          header: t("crews.list.name"),
          cell: ({ row }) => <span className="font-medium">{row.original.name}</span>,
        },
        {
          id: "site",
          header: t("crews.form.site"),
          cell: ({ row }) => {
            const { node_code, node_name } = row.original;
            if (node_code && node_name) return `${node_code} — ${node_name}`;
            return node_name ?? node_code ?? "—";
          },
        },
        {
          accessorKey: "team_type",
          header: t("crews.form.type"),
          cell: ({ row }) => (
            <Badge variant="outline" className="text-[10px]">
              {t(`crews.types.${row.original.team_type}`, row.original.team_type)}
            </Badge>
          ),
        },
        {
          accessorKey: "status",
          header: t("crews.form.status"),
          cell: ({ row }) => (
            <Badge variant="secondary" className="text-[10px]">
              {t(`crews.status.${row.original.status}`, row.original.status)}
            </Badge>
          ),
        },
        {
          accessorKey: "member_count",
          header: t("crews.list.members"),
          cell: ({ row }) => unknownOr(row.original.member_count),
        },
        {
          accessorKey: "lead_count",
          header: t("crews.list.leads"),
          cell: ({ row }) => unknownOr(row.original.lead_count),
        },
        {
          accessorKey: "total_available_minutes",
          header: t("crews.list.availableMin"),
          cell: ({ row }) => unknownOr(row.original.total_available_minutes),
        },
        {
          accessorKey: "total_blocked_minutes",
          header: t("crews.list.blockedMin"),
          cell: ({ row }) => unknownOr(row.original.total_blocked_minutes),
        },
        {
          id: "ratio",
          header: t("crews.list.availabilityPct"),
          cell: ({ row }) => {
            const ratio = row.original.avg_availability_ratio;
            return ratio == null ? "—" : formatPercent(Math.round(ratio * 100));
          },
        },
        {
          id: "actions",
          header: "",
          cell: ({ row }) =>
            canManage ? (
              <Button
                size="sm"
                variant="ghost"
                onClick={(e) => {
                  e.stopPropagation();
                  openEdit(row.original);
                }}
              >
                {t("crews.actions.edit")}
              </Button>
            ) : null,
        },
      ],
      [t, canManage, unknownOr, formatPercent],
    );

    const detailCapacity = detailTeamId != null ? (capacityByTeam.get(detailTeamId) ?? null) : null;

    return (
      <div className="flex min-h-0 flex-1 flex-col space-y-4">
        <SmartFilterBar
          searchPlaceholder={t("capacity.filters.searchPlaceholder")}
          searchValue={searchInput}
          onSearchInputChange={setSearchInput}
          onSearchChange={setSearchQuery}
          filters={filterDefs}
          resultCount={tableRows.length}
          onReset={resetFilters}
        />

        <div className="flex flex-wrap items-end gap-3">
          <label className="text-sm">
            <span className="mb-1 block text-text-muted">{t("availability.from")}</span>
            <DateField value={dateFrom} onChange={(v) => setDateFrom(v)} className="w-44" />
          </label>
          <label className="text-sm">
            <span className="mb-1 block text-text-muted">{t("availability.to")}</span>
            <DateField value={dateTo} onChange={(v) => setDateTo(v)} className="w-44" />
          </label>
        </div>

        {error != null ? (
          <ErrorState error={error} title={t("crews.loadError")} onRetry={refresh} />
        ) : null}
        {loading ? <div className="text-sm text-text-muted">{t("common.loading")}</div> : null}

        {!loading && tableRows.length === 0 ? (
          <PermissionGate
            permission={P.PER_MANAGE}
            fallback={<EmptyState title={t("crews.empty")} />}
          >
            <EmptyState
              title={t("crews.empty")}
              actionLabel={t("crews.actions.create")}
              onAction={openCreate}
            />
          </PermissionGate>
        ) : null}

        {!loading && tableRows.length > 0 ? (
          <div className={cn(mfLayout.moduleWorkspaceSplit, "min-h-0 flex-1")}>
            <div
              className={cn(
                "flex min-h-0 flex-col overflow-auto border-r border-surface-border",
                detailTeamId != null ? "w-[55%] min-w-[360px]" : "w-full",
              )}
            >
              <DataTable
                columns={columns}
                data={tableRows}
                pageSize={20}
                onRowClick={(row) => openDetail(row)}
                isRowSelected={(row) => row.id === detailTeamId}
              />
            </div>
            {detailTeamId != null ? (
              <div className="min-w-[300px] flex-1 overflow-hidden">
                <TeamDetailWorkspace
                  teamId={detailTeamId}
                  dateFrom={dateFrom}
                  dateTo={dateTo}
                  capacity={detailCapacity}
                  canManage={canManage}
                  onClose={() => setDetailTeamId(null)}
                  onEdit={(crew) => {
                    openEdit(crew);
                  }}
                  onOpenPersonnel={onOpenPersonnel}
                  onRosterChanged={refresh}
                  {...(detailTeamId === openTeamId ? { initialTab: "capacity" } : {})}
                />
              </div>
            ) : null}
          </div>
        ) : null}

        <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <DialogContent className="max-w-lg">
            <DialogHeader>
              <DialogTitle>
                {editingCrew ? t("crews.form.editTitle") : t("crews.form.createTitle")}
              </DialogTitle>
            </DialogHeader>
            <div className="grid gap-3 py-2">
              <div className="space-y-1.5">
                <Label>{t("crews.form.code")}</Label>
                <Input value={formCode} onChange={(e) => setFormCode(e.target.value)} />
              </div>
              <div className="space-y-1.5">
                <Label>{t("crews.form.name")}</Label>
                <Input value={formName} onChange={(e) => setFormName(e.target.value)} />
              </div>
              <div className="space-y-1.5">
                <Label>{t("crews.form.type")}</Label>
                <select
                  className={mfInput.nativeSelect}
                  value={formType}
                  onChange={(e) => setFormType(e.target.value)}
                >
                  {TEAM_TYPES.map((type) => (
                    <option key={type} value={type}>
                      {t(`crews.types.${type}`)}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-1.5">
                <Label>{t("crews.form.site")}</Label>
                <select
                  className={mfInput.nativeSelect}
                  value={formNodeId ?? ""}
                  onChange={(e) => setFormNodeId(e.target.value ? Number(e.target.value) : null)}
                >
                  <option value="">—</option>
                  {entityNodes.map((n) => (
                    <option key={n.node_id} value={n.node_id}>
                      {n.code} — {n.name}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-1.5">
                <Label>{t("crews.form.status")}</Label>
                <select
                  className={mfInput.nativeSelect}
                  value={formStatus}
                  onChange={(e) => setFormStatus(e.target.value)}
                >
                  <option value="active">{t("crews.status.active")}</option>
                  <option value="inactive">{t("crews.status.inactive")}</option>
                  <option value="disbanded">{t("crews.status.disbanded")}</option>
                </select>
              </div>
              <div className="space-y-1.5">
                <Label>{t("crews.form.description")}</Label>
                <Input
                  value={formDescription}
                  onChange={(e) => setFormDescription(e.target.value)}
                />
              </div>
              <CrestIconPicker
                iconKey={formIconKey}
                accentColor={formAccentColor}
                onIconKeyChange={setFormIconKey}
                onAccentColorChange={setFormAccentColor}
              />
              {formError ? <div className="text-sm text-destructive">{formError}</div> : null}
            </div>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setDialogOpen(false)}>
                {t("create.cancel")}
              </Button>
              <Button type="button" onClick={() => void submitCrew()} disabled={formSaving}>
                {formSaving ? t("common.loading") : t("crews.form.save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    );
  },
);
