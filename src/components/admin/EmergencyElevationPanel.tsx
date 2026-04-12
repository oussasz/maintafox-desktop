import { type ColumnDef } from "@tanstack/react-table";
import { Clock, ShieldAlert, ShieldX } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useToast } from "@/hooks/use-toast";
import {
  grantEmergencyElevation,
  listEmergencyGrants,
  listRoles,
  listUsers,
  revokeEmergencyElevation,
} from "@/services/rbac-service";
import type { EmergencyGrantView, RoleWithPermissions, UserWithRoles } from "@shared/ipc-types";

const SCOPE_TYPES = ["global", "site", "department"] as const;

// ── Countdown component ──────────────────────────────────────────────────

function Countdown({
  expiresAt,
  t,
  onExpired,
}: {
  expiresAt: string;
  t: (k: string, d: string) => string;
  onExpired: () => void;
}) {
  const [remaining, setRemaining] = useState(() => {
    const diff = new Date(expiresAt).getTime() - Date.now();
    return Math.max(0, Math.floor(diff / 1000));
  });

  const onExpiredRef = useRef(onExpired);
  onExpiredRef.current = onExpired;

  useEffect(() => {
    if (remaining <= 0) {
      onExpiredRef.current();
      return;
    }
    const timer = setInterval(() => {
      const diff = new Date(expiresAt).getTime() - Date.now();
      const secs = Math.max(0, Math.floor(diff / 1000));
      setRemaining(secs);
      if (secs <= 0) {
        clearInterval(timer);
        onExpiredRef.current();
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [expiresAt, remaining]);

  const hours = Math.floor(remaining / 3600);
  const minutes = Math.floor((remaining % 3600) / 60);
  const seconds = remaining % 60;

  return (
    <span className="font-mono text-xs tabular-nums">
      {hours > 0 && (
        <>
          {hours}
          {t("emergency.countdown.hours", "h")}{" "}
        </>
      )}
      {minutes}
      {t("emergency.countdown.minutes", "min")} {String(seconds).padStart(2, "0")}
      {t("emergency.countdown.seconds", "s")}
    </span>
  );
}

export function EmergencyElevationPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { toast } = useToast();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [grants, setGrants] = useState<EmergencyGrantView[]>([]);
  const [users, setUsers] = useState<UserWithRoles[]>([]);
  const [roles, setRoles] = useState<RoleWithPermissions[]>([]);
  const [loading, setLoading] = useState(true);

  // Grant dialog state
  const [showGrant, setShowGrant] = useState(false);
  const [grantUserId, setGrantUserId] = useState("");
  const [grantRoleId, setGrantRoleId] = useState("");
  const [grantScopeType, setGrantScopeType] = useState("global");
  const [grantScopeRef, setGrantScopeRef] = useState("");
  const [grantReason, setGrantReason] = useState("");
  const [grantMinutes, setGrantMinutes] = useState("60");

  // Revoke dialog state
  const [revokeTarget, setRevokeTarget] = useState<EmergencyGrantView | null>(null);

  const fetchData = useCallback(async () => {
    try {
      const [grantsData, usersData, rolesData] = await Promise.all([
        listEmergencyGrants(),
        listUsers({}),
        listRoles(),
      ]);
      setGrants(grantsData);
      setUsers(usersData);
      setRoles(rolesData);
    } catch {
      toast({
        title: t("emergency.errors.loadFailed", "Failed to load grants"),
        variant: "destructive",
      });
    } finally {
      setLoading(false);
    }
  }, [t, toast]);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  // When a countdown expires, move grant to expired state locally
  const handleCountdownExpired = useCallback((assignmentId: number) => {
    setGrants((prev) =>
      prev.map((g) => (g.assignment_id === assignmentId ? { ...g, is_expired: true } : g)),
    );
  }, []);

  // ── Grant ───────────────────────────────────────────────────────────────

  const resetGrantForm = useCallback(() => {
    setGrantUserId("");
    setGrantRoleId("");
    setGrantScopeType("global");
    setGrantScopeRef("");
    setGrantReason("");
    setGrantMinutes("60");
  }, []);

  const handleGrant = useCallback(async () => {
    const minutes = Number(grantMinutes);
    if (minutes < 1 || minutes > 480) return;
    if (grantReason.length < 20) return;

    const now = new Date();
    now.setMinutes(now.getMinutes() + minutes);
    const expiresAt = now.toISOString();

    try {
      await withStepUp(() =>
        grantEmergencyElevation({
          user_id: Number(grantUserId),
          role_id: Number(grantRoleId),
          scope_type: grantScopeType,
          scope_reference: grantScopeType !== "global" && grantScopeRef ? grantScopeRef : null,
          reason: grantReason,
          expires_at: expiresAt,
        }),
      );
      toast({ title: t("emergency.granted", "Emergency access granted"), variant: "success" });
      setShowGrant(false);
      resetGrantForm();
      void fetchData();
    } catch (err) {
      toast({
        title: err instanceof Error ? err.message : t("emergency.errors.grantFailed"),
        variant: "destructive",
      });
    }
  }, [
    grantUserId,
    grantRoleId,
    grantScopeType,
    grantScopeRef,
    grantReason,
    grantMinutes,
    fetchData,
    resetGrantForm,
    t,
    toast,
    withStepUp,
  ]);

  const canGrant =
    grantUserId &&
    grantRoleId &&
    grantReason.length >= 20 &&
    Number(grantMinutes) >= 1 &&
    Number(grantMinutes) <= 480;

  // ── Revoke ──────────────────────────────────────────────────────────────

  const handleRevoke = useCallback(async () => {
    if (!revokeTarget) return;
    try {
      await withStepUp(() =>
        revokeEmergencyElevation({ assignment_id: revokeTarget.assignment_id }),
      );
      toast({
        title: t("emergency.revokedSuccess", "Emergency access revoked"),
        variant: "success",
      });
      setRevokeTarget(null);
      void fetchData();
    } catch {
      toast({
        title: t("emergency.errors.revokeFailed", "Failed to revoke access"),
        variant: "destructive",
      });
    }
  }, [revokeTarget, fetchData, t, toast, withStepUp]);

  // ── Table columns ───────────────────────────────────────────────────────

  const columns: ColumnDef<EmergencyGrantView>[] = useMemo(
    () => [
      {
        accessorKey: "username",
        header: t("emergency.columns.username", "User"),
        cell: ({ row }) => <span className="font-medium">{row.original.username}</span>,
      },
      {
        accessorKey: "role_name",
        header: t("emergency.columns.role", "Role"),
      },
      {
        id: "scope",
        header: t("emergency.columns.scope", "Scope"),
        cell: ({ row }) => (
          <span>
            {row.original.scope_type}
            {row.original.scope_reference && (
              <span className="ml-1 text-text-muted">({row.original.scope_reference})</span>
            )}
          </span>
        ),
      },
      {
        accessorKey: "emergency_reason",
        header: t("emergency.columns.reason", "Reason"),
        cell: ({ row }) => (
          <span
            className="max-w-[200px] truncate text-xs"
            title={row.original.emergency_reason ?? ""}
          >
            {row.original.emergency_reason ?? "—"}
          </span>
        ),
      },
      {
        id: "expiry",
        header: t("emergency.columns.expiresAt", "Expires"),
        cell: ({ row }) => {
          if (row.original.is_expired) {
            return (
              <Badge variant="secondary" className="text-[10px] opacity-60">
                {t("emergency.badges.expired", "Expired")}
              </Badge>
            );
          }
          if (!row.original.emergency_expires_at) return "—";
          return (
            <div className="flex items-center gap-1.5">
              <Clock className="h-3 w-3 text-orange-500" />
              <Countdown
                expiresAt={row.original.emergency_expires_at}
                t={t}
                onExpired={() => handleCountdownExpired(row.original.assignment_id)}
              />
            </div>
          );
        },
      },
      {
        accessorKey: "assigned_by_username",
        header: t("emergency.columns.grantedBy", "Granted By"),
        cell: ({ row }) => row.original.assigned_by_username ?? "—",
      },
      {
        id: "status",
        header: t("emergency.columns.status", "Status"),
        cell: ({ row }) =>
          row.original.is_expired ? (
            <Badge variant="secondary" className="text-[10px] opacity-60">
              {t("emergency.badges.expired", "Expired")}
            </Badge>
          ) : (
            <Badge variant="default" className="bg-orange-500 text-[10px]">
              {t("emergency.badges.active", "Active")}
            </Badge>
          ),
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) => {
          if (row.original.is_expired || !can("adm.users")) return null;
          return (
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive hover:text-destructive"
              onClick={() => setRevokeTarget(row.original)}
            >
              <ShieldX className="mr-1 h-3.5 w-3.5" />
              {t("emergency.revoke", "Revoke")}
            </Button>
          );
        },
      },
    ],
    [t, can, handleCountdownExpired],
  );

  // Separate active and expired for visual grouping
  const sortedGrants = useMemo(
    () => [...grants].sort((a, b) => Number(a.is_expired) - Number(b.is_expired)),
    [grants],
  );

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-text-primary">
          {t("emergency.title", "Emergency Elevations")}
        </h2>
        {can("adm.users") && (
          <Button
            size="sm"
            onClick={() => {
              resetGrantForm();
              setShowGrant(true);
            }}
          >
            <ShieldAlert className="mr-1.5 h-4 w-4" />
            {t("emergency.grant", "Emergency Access")}
          </Button>
        )}
      </div>

      {/* Table */}
      <DataTable<EmergencyGrantView, unknown>
        columns={columns}
        data={sortedGrants}
        isLoading={loading}
        skeletonRows={4}
        pageSize={20}
      />

      {/* ── Grant Dialog ─────────────────────────────────────────────────── */}
      <Dialog open={showGrant} onOpenChange={(v) => !v && setShowGrant(false)}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("emergency.grantTitle", "Grant Emergency Access")}</DialogTitle>
            <DialogDescription>{t("emergency.grantDesc")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            {/* User */}
            <div className="space-y-1.5">
              <Label>{t("emergency.fields.user", "User")}</Label>
              <Select value={grantUserId} onValueChange={setGrantUserId}>
                <SelectTrigger>
                  <SelectValue placeholder={t("emergency.fields.selectUser", "Choose a user")} />
                </SelectTrigger>
                <SelectContent>
                  {users
                    .filter((u) => u.is_active)
                    .map((u) => (
                      <SelectItem key={u.id} value={String(u.id)}>
                        {u.username}
                        {u.display_name ? ` (${u.display_name})` : ""}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            </div>

            {/* Role */}
            <div className="space-y-1.5">
              <Label>{t("emergency.fields.role", "Role")}</Label>
              <Select value={grantRoleId} onValueChange={setGrantRoleId}>
                <SelectTrigger>
                  <SelectValue placeholder={t("emergency.fields.selectRole", "Choose a role")} />
                </SelectTrigger>
                <SelectContent>
                  {roles.map((r) => (
                    <SelectItem key={r.id} value={String(r.id)}>
                      {r.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Scope */}
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>{t("emergency.fields.scopeType", "Scope Type")}</Label>
                <Select value={grantScopeType} onValueChange={setGrantScopeType}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {SCOPE_TYPES.map((s) => (
                      <SelectItem key={s} value={s}>
                        {s}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              {grantScopeType !== "global" && (
                <div className="space-y-1.5">
                  <Label>{t("emergency.fields.scopeReference", "Scope Reference")}</Label>
                  <Input value={grantScopeRef} onChange={(e) => setGrantScopeRef(e.target.value)} />
                </div>
              )}
            </div>

            {/* Reason */}
            <div className="space-y-1.5">
              <Label>{t("emergency.fields.reason", "Reason (min. 20 characters)")}</Label>
              <Textarea
                value={grantReason}
                onChange={(e) => setGrantReason(e.target.value)}
                placeholder={t("emergency.fields.reasonPlaceholder")}
                rows={3}
              />
              {grantReason.length > 0 && grantReason.length < 20 && (
                <p className="text-xs text-destructive">
                  {t("emergency.errors.reasonTooShort", "Reason must be at least 20 characters")}
                </p>
              )}
            </div>

            {/* Duration */}
            <div className="space-y-1.5">
              <Label>{t("emergency.fields.expiryMinutes", "Duration (minutes)")}</Label>
              <Input
                type="number"
                min={1}
                max={480}
                value={grantMinutes}
                onChange={(e) => setGrantMinutes(e.target.value)}
              />
              <p className="text-xs text-text-muted">
                {t("emergency.fields.expiryRange", "1 to 480 minutes (8h max)")}
              </p>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowGrant(false)}>
              {t("common.cancel", "Cancel")}
            </Button>
            <Button onClick={() => void handleGrant()} disabled={!canGrant}>
              {t("emergency.grant", "Grant")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Revoke Dialog ────────────────────────────────────────────────── */}
      <Dialog open={!!revokeTarget} onOpenChange={(v) => !v && setRevokeTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {t("emergency.revokeConfirm", "Revoke this emergency access?")}
            </DialogTitle>
            <DialogDescription>{t("emergency.revokeConfirmDesc")}</DialogDescription>
          </DialogHeader>
          <p className="text-sm text-text-secondary">
            {revokeTarget?.username} — {revokeTarget?.role_name}
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRevokeTarget(null)}>
              {t("common.cancel", "Cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void handleRevoke()}>
              {t("emergency.revoke", "Revoke")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {StepUpDialogElement}
    </div>
  );
}
