import { type ColumnDef } from "@tanstack/react-table";
import { RefreshCw, ShieldX } from "lucide-react";
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
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useToast } from "@/hooks/use-toast";
import { listActiveSessions, revokeSession } from "@/services/rbac-service";
import type { SessionSummary } from "@shared/ipc-types";

const AUTO_REFRESH_MS = 30_000;

function TrustBadge({ status, t }: { status: string; t: (k: string, d: string) => string }) {
  if (status === "trusted") {
    return (
      <Badge variant="default" className="text-[10px]">
        {t("sessions.badges.trusted", "Trusted")}
      </Badge>
    );
  }
  if (status === "revoked") {
    return (
      <Badge variant="destructive" className="text-[10px]">
        {t("sessions.badges.revoked", "Revoked")}
      </Badge>
    );
  }
  return (
    <Badge variant="secondary" className="text-[10px]">
      {t("sessions.badges.unknown", "Unknown")}
    </Badge>
  );
}

export function SessionVisibilityPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { toast } = useToast();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [revokeTarget, setRevokeTarget] = useState<SessionSummary | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchSessions = useCallback(async () => {
    try {
      const data = await listActiveSessions();
      setSessions(data);
    } catch {
      toast({
        title: t("sessions.errors.loadFailed", "Failed to load sessions"),
        variant: "destructive",
      });
    } finally {
      setLoading(false);
    }
  }, [t, toast]);

  // Initial load + auto-refresh
  useEffect(() => {
    void fetchSessions();
    timerRef.current = setInterval(() => void fetchSessions(), AUTO_REFRESH_MS);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [fetchSessions]);

  const handleRevoke = useCallback(
    async (session: SessionSummary) => {
      try {
        await withStepUp(() => revokeSession(session.session_id));
        toast({
          title: t("sessions.revoked", "Session revoked"),
          variant: "success",
        });
        setRevokeTarget(null);
        void fetchSessions();
      } catch {
        toast({
          title: t("sessions.errors.revokeFailed", "Failed to revoke session"),
          variant: "destructive",
        });
      }
    },
    [fetchSessions, t, toast, withStepUp],
  );

  const columns: ColumnDef<SessionSummary>[] = useMemo(
    () => [
      {
        accessorKey: "username",
        header: t("sessions.columns.username", "User"),
        cell: ({ row }) => (
          <div className="flex items-center gap-2">
            <span className="font-medium">{row.original.username}</span>
            {row.original.is_current_session && (
              <Badge variant="outline" className="text-[10px] border-primary text-primary">
                {t("sessions.badges.currentSession", "Current session")}
              </Badge>
            )}
          </div>
        ),
      },
      {
        accessorKey: "device_name",
        header: t("sessions.columns.deviceName", "Device"),
        cell: ({ row }) => row.original.device_name ?? "—",
      },
      {
        id: "trust",
        header: t("sessions.columns.trustStatus", "Trust"),
        cell: ({ row }) => <TrustBadge status={row.original.device_trust_status} t={t} />,
      },
      {
        accessorKey: "session_started_at",
        header: t("sessions.columns.sessionStart", "Started"),
        cell: ({ row }) => {
          try {
            return new Date(row.original.session_started_at).toLocaleString();
          } catch {
            return row.original.session_started_at;
          }
        },
      },
      {
        accessorKey: "last_activity_at",
        header: t("sessions.columns.lastActivity", "Last Activity"),
        cell: ({ row }) => {
          const v = row.original.last_activity_at;
          if (!v) return "—";
          try {
            return new Date(v).toLocaleString();
          } catch {
            return v;
          }
        },
      },
      {
        id: "roles",
        header: t("sessions.columns.roles", "Roles"),
        cell: ({ row }) => (
          <div className="flex flex-wrap gap-1">
            {row.original.current_role_names.map((r) => (
              <Badge key={r} variant="secondary" className="text-[10px]">
                {r}
              </Badge>
            ))}
          </div>
        ),
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) => {
          if (row.original.is_current_session || !can("adm.users")) return null;
          return (
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive hover:text-destructive"
              onClick={(e) => {
                e.stopPropagation();
                setRevokeTarget(row.original);
              }}
            >
              <ShieldX className="mr-1 h-3.5 w-3.5" />
              {t("sessions.revoke", "Revoke")}
            </Button>
          );
        },
      },
    ],
    [t, can],
  );

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-text-primary">
            {t("sessions.title", "Active Sessions")}
          </h2>
          <p className="text-xs text-text-muted">
            {t("sessions.autoRefresh", "Auto-refresh every 30s")}
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={() => void fetchSessions()}>
          <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
          {t("common.close", "Refresh")}
        </Button>
      </div>

      {/* Table */}
      <DataTable<SessionSummary, unknown>
        columns={columns}
        data={sessions}
        isLoading={loading}
        skeletonRows={5}
        pageSize={20}
      />

      {/* Revoke confirmation dialog */}
      <Dialog open={!!revokeTarget} onOpenChange={(v) => !v && setRevokeTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("sessions.revokeConfirm", "Revoke this session?")}</DialogTitle>
            <DialogDescription>
              {t("sessions.revokeConfirmDesc", "The user will be disconnected immediately.")}
            </DialogDescription>
          </DialogHeader>
          <p className="text-sm text-text-secondary">
            {revokeTarget?.username} — {revokeTarget?.device_name ?? "—"}
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRevokeTarget(null)}>
              {t("common.cancel", "Cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => revokeTarget && void handleRevoke(revokeTarget)}
            >
              {t("sessions.revoke", "Revoke")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {StepUpDialogElement}
    </div>
  );
}
