import type { ColumnDef } from "@tanstack/react-table";
import { ChevronRight, Plus, Zap } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { CreateUserDialog } from "@/components/admin/CreateUserDialog";
import { EditUserDialog } from "@/components/admin/EditUserDialog";
import { OnlinePresenceIndicator } from "@/components/admin/OnlinePresenceIndicator";
import { useScopeLabel } from "@/components/admin/ScopeNodeSelect";
import { TempPasswordDialog } from "@/components/admin/TempPasswordDialog";
import { UserDetailDialog } from "@/components/admin/UserDetailDialog";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { Button } from "@/components/ui/button";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { listAssignableRoles, listAssignableScopeNodes, listUsers } from "@/services/rbac-service";
import { useSessionStore } from "@/store/session-store";
import type {
  AssignableRoleSummary,
  AssignableScopeNode,
  UserListFilter,
  UserWithRoles,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const STATUS_VALUES = ["active", "inactive", "locked"] as const;
type StatusValue = (typeof STATUS_VALUES)[number];

/**
 * Accounts of the plant: filter by status (active, disabled, locked), role, site and sign-in
 * mode; open an account for its roles per scope, sessions, permissions and history. Accounts
 * are never deleted (their history must stay readable): they are disabled with a reason.
 */
export function UserListPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { formatDateTime } = useFormatters();
  const scopeLabel = useScopeLabel();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const currentUserId = useSessionStore((s) => s.info?.user_id ?? null);

  const [users, setUsers] = useState<UserWithRoles[]>([]);
  const [roles, setRoles] = useState<AssignableRoleSummary[]>([]);
  const [nodes, setNodes] = useState<AssignableScopeNode[]>([]);
  const [loading, setLoading] = useState(true);

  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<StatusValue | null>(null);
  const [roleFilter, setRoleFilter] = useState<string | null>(null);
  const [siteFilter, setSiteFilter] = useState<string | null>(null);
  const [modeFilter, setModeFilter] = useState<string | null>(null);

  const [detailUserId, setDetailUserId] = useState<number | null>(null);
  const [editTarget, setEditTarget] = useState<UserWithRoles | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  // Show-once temporary password after an admin reset (never persisted beyond React state).
  const [tempPassword, setTempPassword] = useState<string | null>(null);

  const filter = useMemo((): UserListFilter => {
    const f: UserListFilter = {};
    if (search.trim()) f.search = search.trim();
    if (status) f.status = status;
    if (roleFilter) f.role_id = Number(roleFilter);
    if (siteFilter) f.org_node_id = Number(siteFilter);
    if (modeFilter) f.identity_mode = modeFilter;
    return f;
  }, [search, status, roleFilter, siteFilter, modeFilter]);

  const fetchUsers = useCallback(async () => {
    setLoading(true);
    try {
      setUsers(await listUsers(filter));
    } catch {
      // The IPC layer reports the error.
    } finally {
      setLoading(false);
    }
  }, [filter]);

  useEffect(() => {
    void fetchUsers();
  }, [fetchUsers]);

  useEffect(() => {
    listAssignableRoles()
      .then(setRoles)
      .catch(() => setRoles([]));
    listAssignableScopeNodes()
      .then(setNodes)
      .catch(() => setNodes([]));
  }, []);

  const columns: ColumnDef<UserWithRoles>[] = useMemo(
    () => [
      {
        accessorKey: "username",
        header: t("console.columns.user"),
        cell: ({ row }) => (
          <div className="flex items-center gap-2">
            <OnlinePresenceIndicator userId={row.original.id} />
            <div className="min-w-0">
              <div className="truncate font-medium text-text-primary">
                {row.original.display_name ?? row.original.username}
              </div>
              <div className="truncate font-mono text-[11px] text-text-secondary">
                {row.original.username}
              </div>
            </div>
          </div>
        ),
      },
      {
        id: "status",
        header: t("console.columns.status"),
        cell: ({ row }) => (
          <div className="flex flex-wrap items-center gap-1">
            <StatusChip
              label={
                row.original.is_active ? t("console.status.active") : t("console.status.disabled")
              }
              tone={row.original.is_active ? "success" : "neutral"}
              dot
            />
            {row.original.is_locked ? (
              <StatusChip label={t("console.status.locked")} tone="attention" dot />
            ) : null}
          </div>
        ),
      },
      {
        id: "roles",
        header: t("console.columns.roles"),
        cell: ({ row }) => {
          const roleRows = row.original.roles;
          if (roleRows.length === 0) {
            return <span className="text-xs text-text-muted">{t("console.noRole")}</span>;
          }
          return (
            <ul className="flex max-w-[22rem] flex-col gap-0.5 text-xs">
              {roleRows.map((r) => (
                <li key={r.assignment_id} className="flex items-center gap-1">
                  {r.is_emergency ? <Zap className="h-3 w-3 text-accent" aria-hidden /> : null}
                  <span className="font-medium text-text-primary">{r.role_name}</span>
                  <span className="text-text-secondary">
                    · {scopeLabel(r.scope_type, r.scope_label, r.scope_reference)}
                  </span>
                </li>
              ))}
            </ul>
          );
        },
      },
      {
        id: "mode",
        header: t("console.columns.signIn"),
        cell: ({ row }) => (
          <span className="text-xs text-text-secondary">
            {t(`console.identityMode.${row.original.identity_mode}`, row.original.identity_mode)}
          </span>
        ),
      },
      {
        accessorKey: "last_login_at",
        header: t("console.columns.lastLogin"),
        cell: ({ row }) => (
          <span className="text-xs text-text-secondary">
            {row.original.last_login_at
              ? formatDateTime(row.original.last_login_at)
              : t("console.never")}
          </span>
        ),
      },
      {
        id: "open",
        header: "",
        cell: ({ row }) => (
          <Button
            variant="ghost"
            size="sm"
            aria-label={t("console.openUser", { name: row.original.username })}
            onClick={(e) => {
              e.stopPropagation();
              setDetailUserId(row.original.id);
            }}
          >
            <ChevronRight className="h-4 w-4" aria-hidden />
          </Button>
        ),
      },
    ],
    [t, formatDateTime, scopeLabel],
  );

  const filters: SmartFilterDef[] = [
    {
      id: "status",
      kind: "select",
      label: t("console.filters.status"),
      options: STATUS_VALUES.map((v) => ({
        value: v,
        label: t(`console.status.${v === "inactive" ? "disabled" : v}`),
      })),
      value: status,
      onChange: (v) => setStatus((v as StatusValue | null) ?? null),
      allLabel: t("console.filters.all"),
    },
    {
      id: "role",
      kind: "select",
      label: t("console.filters.role"),
      options: roles.map((r) => ({ value: String(r.id), label: r.name })),
      value: roleFilter,
      onChange: setRoleFilter,
      allLabel: t("console.filters.allRoles"),
    },
    {
      id: "site",
      kind: "select",
      label: t("console.filters.site"),
      options: nodes.map((n) => ({
        value: String(n.id),
        label: `${" ".repeat(Math.max(0, n.depth - (nodes[0]?.depth ?? 0)))}${n.name}`,
      })),
      value: siteFilter,
      onChange: setSiteFilter,
      allLabel: t("console.filters.allSites"),
    },
    {
      id: "mode",
      kind: "select",
      label: t("console.filters.signIn"),
      options: ["local", "sso", "hybrid"].map((m) => ({
        value: m,
        label: t(`console.identityMode.${m}`),
      })),
      value: modeFilter,
      onChange: setModeFilter,
      allLabel: t("console.filters.all"),
    },
  ];

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1">
          <SmartFilterBar
            searchPlaceholder={t("console.searchUsers")}
            searchValue={searchInput}
            onSearchInputChange={setSearchInput}
            onSearchChange={setSearch}
            filters={filters}
            resultCount={users.length}
            onReset={() => {
              setSearchInput("");
              setSearch("");
              setStatus(null);
              setRoleFilter(null);
              setSiteFilter(null);
              setModeFilter(null);
            }}
            className="border-0 px-0 py-0"
          />
        </div>
        {can(P.ADM_USERS) ? (
          <Button size="sm" className="mt-0.5 shrink-0" onClick={() => setShowCreate(true)}>
            <Plus className="mr-1.5 h-4 w-4" aria-hidden />
            {t("users.create")}
          </Button>
        ) : null}
      </div>
      {siteFilter ? (
        <p className="text-xs text-text-secondary">{t("console.filters.siteHint")}</p>
      ) : null}

      <DataTable
        columns={columns}
        data={users}
        isLoading={loading}
        skeletonRows={8}
        pageSize={20}
        onRowClick={(row) => setDetailUserId(row.id)}
      />

      <UserDetailDialog
        userId={detailUserId}
        open={detailUserId !== null}
        onClose={() => setDetailUserId(null)}
        onChanged={() => void fetchUsers()}
        onEdit={(u) => setEditTarget(u)}
        onTemporaryPassword={setTempPassword}
        withStepUp={withStepUp}
        roles={roles}
        nodes={nodes}
        currentUserId={currentUserId}
      />

      <EditUserDialog
        user={editTarget}
        open={editTarget !== null}
        onClose={() => setEditTarget(null)}
        onSaved={() => void fetchUsers()}
      />

      <TempPasswordDialog
        open={tempPassword !== null}
        temporaryPassword={tempPassword}
        onClose={() => setTempPassword(null)}
      />

      <CreateUserDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={() => void fetchUsers()}
      />

      {StepUpDialogElement}
    </div>
  );
}
