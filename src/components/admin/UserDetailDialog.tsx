import {
  KeyRound,
  LockOpen,
  Monitor,
  Pencil,
  Plus,
  ShieldCheck,
  UserCheck,
  UserX,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { AdminAuditTimeline } from "@/components/admin/AdminAuditTimeline";
import { AssignRoleDialog } from "@/components/admin/AssignRoleDialog";
import { useScopeLabel } from "@/components/admin/ScopeNodeSelect";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import {
  adminSetUserPassword,
  deactivateUser,
  getUser,
  listActiveSessions,
  reactivateUser,
  revokeEmergencyElevation,
  revokeRoleScope,
  revokeSession,
  unlockUserAccount,
} from "@/services/rbac-service";
import type {
  AssignableRoleSummary,
  AssignableScopeNode,
  RoleAssignmentSummary,
  SessionSummary,
  UserDetail,
  UserWithRoles,
} from "@shared/ipc-types";

interface UserDetailDialogProps {
  userId: number | null;
  open: boolean;
  onClose: () => void;
  /** Something changed (list must refresh). */
  onChanged: () => void;
  onEdit: (user: UserWithRoles) => void;
  onTemporaryPassword: (password: string) => void;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
  roles: AssignableRoleSummary[];
  nodes: AssignableScopeNode[];
  /** The role / site lists failed to load: assigning a role is not offered. */
  choicesUnavailable?: boolean;
  currentUserId: number | null;
}

type PendingAction =
  | { kind: "disable" }
  | { kind: "enable" }
  | { kind: "reset" }
  | { kind: "unlock" }
  | { kind: "revoke"; assignment: RoleAssignmentSummary }
  | { kind: "session"; session: SessionSummary };

/** Module prefix of a permission code (`ot.view` → `ot`). */
function moduleOf(permission: string): string {
  return permission.split(".")[0] ?? permission;
}

/** One account: identity and state, roles per scope, open sessions, permissions, history. */
export function UserDetailDialog({
  userId,
  open,
  onClose,
  onChanged,
  onEdit,
  onTemporaryPassword,
  withStepUp,
  roles,
  nodes,
  choicesUnavailable = false,
  currentUserId,
}: UserDetailDialogProps) {
  const { t } = useTranslation("admin");
  const { formatDateTime, formatDate } = useFormatters();
  const scopeLabel = useScopeLabel();
  const [detail, setDetail] = useState<UserDetail | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState("overview");
  const [pending, setPending] = useState<PendingAction | null>(null);
  const [reason, setReason] = useState("");
  const [assignOpen, setAssignOpen] = useState(false);
  const [historyKey, setHistoryKey] = useState(0);

  const load = useCallback(async () => {
    if (userId == null) return;
    setLoading(true);
    try {
      const [d, s] = await Promise.all([getUser(userId), listActiveSessions(userId)]);
      setDetail(d);
      setSessions(s);
    } catch {
      setDetail(null);
      setSessions([]);
    } finally {
      setLoading(false);
    }
  }, [userId]);

  useEffect(() => {
    if (!open || userId == null) return;
    setTab("overview");
    void load();
  }, [open, userId, load]);

  const afterChange = useCallback(async () => {
    await load();
    setHistoryKey((k) => k + 1);
    onChanged();
  }, [load, onChanged]);

  const user = detail?.user ?? null;
  const isSelf = user != null && currentUserId === user.id;

  const permissionsByModule = useMemo(() => {
    const groups = new Map<string, string[]>();
    for (const p of detail?.effective_permissions ?? []) {
      const m = moduleOf(p);
      groups.set(m, [...(groups.get(m) ?? []), p]);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [detail]);

  const openAction = (action: PendingAction) => {
    setReason("");
    setPending(action);
  };

  const runPending = async () => {
    if (!pending || !user) return;
    switch (pending.kind) {
      case "disable":
        await withStepUp(() => deactivateUser({ user_id: user.id, reason: reason.trim() }));
        break;
      case "enable":
        await withStepUp(() => reactivateUser({ user_id: user.id, reason: reason.trim() }));
        break;
      case "unlock":
        await withStepUp(() => unlockUserAccount(user.id));
        break;
      case "reset": {
        const result = await withStepUp(() =>
          adminSetUserPassword({ user_id: user.id, temporary_password: null }),
        );
        onTemporaryPassword(result.temporary_password);
        break;
      }
      case "revoke": {
        const note = reason.trim() || undefined;
        if (pending.assignment.is_emergency) {
          await revokeEmergencyElevation({
            assignment_id: pending.assignment.assignment_id,
            reason: note ?? null,
          });
        } else {
          await withStepUp(() => revokeRoleScope(pending.assignment.assignment_id, note));
        }
        break;
      }
      case "session":
        await withStepUp(() => revokeSession(pending.session.session_id));
        break;
    }
    await afterChange();
  };

  const reasonRequired = pending?.kind === "disable" || pending?.kind === "enable";
  const reasonOffered = reasonRequired || pending?.kind === "revoke";

  const confirmText = (): { title: string; description: string; label: string } => {
    const name = user?.display_name ?? user?.username ?? "";
    switch (pending?.kind) {
      case "disable":
        return {
          title: t("console.user.disableTitle", { name }),
          description: t("console.user.disableBody"),
          label: t("console.user.disable"),
        };
      case "enable":
        return {
          title: t("console.user.enableTitle", { name }),
          description: t("console.user.enableBody"),
          label: t("console.user.enable"),
        };
      case "unlock":
        return {
          title: t("console.user.unlockTitle", { name }),
          description: t("console.user.unlockBody"),
          label: t("console.user.unlock"),
        };
      case "reset":
        return {
          title: t("console.user.resetTitle", { name }),
          description: t("console.user.resetBody"),
          label: t("console.user.reset"),
        };
      case "revoke":
        return {
          title: t("console.roles.revokeTitle", { role: pending.assignment.role_name }),
          description: t("console.roles.revokeBody"),
          label: t("console.roles.revoke"),
        };
      case "session":
        return {
          title: t("console.sessions.revokeTitle"),
          description: t("console.sessions.revokeBody"),
          label: t("console.sessions.revoke"),
        };
      default:
        return { title: "", description: "", label: "" };
    }
  };
  const confirm = confirmText();

  const validity = (r: RoleAssignmentSummary): string | null => {
    if (r.is_emergency && r.emergency_expires_at) {
      return t("console.roles.emergencyUntil", { date: formatDateTime(r.emergency_expires_at) });
    }
    if (r.valid_from && r.valid_to) {
      return t("console.roles.validBetween", {
        from: formatDate(r.valid_from),
        to: formatDate(r.valid_to),
      });
    }
    if (r.valid_from) return t("console.roles.validFrom", { date: formatDate(r.valid_from) });
    if (r.valid_to) return t("console.roles.validUntil", { date: formatDate(r.valid_to) });
    return null;
  };

  return (
    <>
      <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
        <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle>{user?.display_name ?? user?.username ?? "…"}</DialogTitle>
            <DialogDescription>
              {user ? (
                <span className="flex flex-wrap items-center gap-2">
                  <span className="font-mono text-xs">{user.username}</span>
                  <StatusChip
                    label={
                      user.is_active ? t("console.status.active") : t("console.status.disabled")
                    }
                    tone={user.is_active ? "success" : "neutral"}
                    dot
                  />
                  {user.is_locked ? (
                    <StatusChip label={t("console.status.locked")} tone="attention" dot />
                  ) : null}
                  {user.force_password_change ? (
                    <StatusChip label={t("console.status.mustChangePassword")} tone="info" dot />
                  ) : null}
                </span>
              ) : (
                t("common.loading")
              )}
            </DialogDescription>
          </DialogHeader>

          {loading && !detail ? (
            <p className="py-6 text-sm text-text-secondary">{t("common.loading")}</p>
          ) : null}

          {user && detail ? (
            <Tabs value={tab} onValueChange={setTab} className="mt-2">
              <TabsList>
                <TabsTrigger value="overview">{t("console.user.tabs.overview")}</TabsTrigger>
                <TabsTrigger value="roles">
                  {t("console.user.tabs.roles", { count: user.roles.length })}
                </TabsTrigger>
                <TabsTrigger value="sessions">
                  {t("console.user.tabs.sessions", { count: sessions.length })}
                </TabsTrigger>
                <TabsTrigger value="permissions">{t("console.user.tabs.permissions")}</TabsTrigger>
                <TabsTrigger value="history">{t("console.user.tabs.history")}</TabsTrigger>
              </TabsList>

              <TabsContent value="overview" className="space-y-5 pt-4 text-sm">
                <dl className="grid grid-cols-1 gap-x-6 gap-y-2 sm:grid-cols-2">
                  <div>
                    <dt className="text-xs text-text-secondary">
                      {t("users.fields.identityMode")}
                    </dt>
                    <dd>{t(`console.identityMode.${user.identity_mode}`, user.identity_mode)}</dd>
                  </div>
                  <div>
                    <dt className="text-xs text-text-secondary">{t("console.user.lastLogin")}</dt>
                    <dd>
                      {user.last_login_at ? formatDateTime(user.last_login_at) : t("console.never")}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-xs text-text-secondary">{t("users.fields.email")}</dt>
                    <dd>{user.email ?? "—"}</dd>
                  </div>
                  <div>
                    <dt className="text-xs text-text-secondary">{t("users.fields.phone")}</dt>
                    <dd>{user.phone ?? "—"}</dd>
                  </div>
                  <div>
                    <dt className="text-xs text-text-secondary">
                      {t("console.user.lastActivity")}
                    </dt>
                    <dd>{user.last_seen_at ? formatDateTime(user.last_seen_at) : "—"}</dd>
                  </div>
                  {user.is_locked && user.locked_until ? (
                    <div>
                      <dt className="text-xs text-text-secondary">
                        {t("console.user.lockedUntil")}
                      </dt>
                      <dd>{formatDateTime(user.locked_until)}</dd>
                    </div>
                  ) : null}
                </dl>

                <div className="flex flex-wrap gap-2 border-t border-surface-border pt-4">
                  <Button variant="outline" size="sm" onClick={() => onEdit(user)}>
                    <Pencil className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                    {t("console.user.edit")}
                  </Button>
                  {user.identity_mode !== "sso" && user.is_active ? (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => openAction({ kind: "reset" })}
                    >
                      <KeyRound className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                      {t("console.user.reset")}
                    </Button>
                  ) : null}
                  {user.is_locked ? (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => openAction({ kind: "unlock" })}
                    >
                      <LockOpen className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                      {t("console.user.unlock")}
                    </Button>
                  ) : null}
                  {user.is_active ? (
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={isSelf}
                      title={isSelf ? t("console.user.cannotDisableSelf") : undefined}
                      onClick={() => openAction({ kind: "disable" })}
                    >
                      <UserX className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                      {t("console.user.disable")}
                    </Button>
                  ) : (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => openAction({ kind: "enable" })}
                    >
                      <UserCheck className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                      {t("console.user.enable")}
                    </Button>
                  )}
                </div>
                <p className="text-xs text-text-secondary">{t("console.user.noDeleteHint")}</p>
              </TabsContent>

              <TabsContent value="roles" className="space-y-3 pt-4 text-sm">
                <div className="flex items-center justify-between">
                  <p className="text-xs text-text-secondary">{t("console.roles.scopeHint")}</p>
                  <Button
                    size="sm"
                    onClick={() => setAssignOpen(true)}
                    disabled={!user.is_active || choicesUnavailable}
                    {...(choicesUnavailable ? { title: t("console.choicesLoadError") } : {})}
                  >
                    <Plus className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                    {t("console.roles.assign")}
                  </Button>
                </div>
                {user.roles.length === 0 ? (
                  <p className="text-text-secondary">{t("users.detail.noRoles")}</p>
                ) : (
                  <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
                    {user.roles.map((r) => {
                      const until = validity(r);
                      return (
                        <li
                          key={r.assignment_id}
                          className="flex flex-wrap items-center justify-between gap-2 px-3 py-2"
                        >
                          <div className="flex min-w-0 flex-wrap items-center gap-2">
                            {r.is_emergency ? (
                              <Zap className="h-4 w-4 text-accent" aria-hidden />
                            ) : (
                              <ShieldCheck className="h-4 w-4 text-primary" aria-hidden />
                            )}
                            <span className="font-medium">{r.role_name}</span>
                            <StatusChip
                              label={scopeLabel(r.scope_type, r.scope_label, r.scope_reference)}
                              tone={r.scope_type === "tenant" ? "info" : "neutral"}
                            />
                            {r.is_emergency ? (
                              <StatusChip label={t("console.roles.emergency")} tone="attention" />
                            ) : null}
                            {until ? (
                              <span className="text-xs text-text-secondary">{until}</span>
                            ) : null}
                          </div>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => openAction({ kind: "revoke", assignment: r })}
                          >
                            {t("console.roles.revoke")}
                          </Button>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </TabsContent>

              <TabsContent value="sessions" className="space-y-3 pt-4 text-sm">
                <p className="text-xs text-text-secondary">{t("console.sessions.hint")}</p>
                {sessions.length === 0 ? (
                  <p className="text-text-secondary">{t("console.sessions.none")}</p>
                ) : (
                  <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
                    {sessions.map((s) => (
                      <li
                        key={s.session_id}
                        className="flex flex-wrap items-center justify-between gap-2 px-3 py-2"
                      >
                        <div className="flex items-center gap-2">
                          <Monitor className="h-4 w-4 text-text-secondary" aria-hidden />
                          <span>{s.device_name ?? t("console.sessions.unknownDevice")}</span>
                          <span className="text-xs text-text-secondary">
                            {t("console.sessions.started", {
                              date: formatDateTime(s.session_started_at),
                            })}
                            {s.last_activity_at
                              ? ` · ${t("console.sessions.lastActivity", { date: formatDateTime(s.last_activity_at) })}`
                              : ""}
                          </span>
                          {s.is_current_session ? (
                            <StatusChip label={t("console.sessions.current")} tone="info" />
                          ) : null}
                        </div>
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={s.is_current_session}
                          onClick={() => openAction({ kind: "session", session: s })}
                        >
                          {t("console.sessions.revoke")}
                        </Button>
                      </li>
                    ))}
                  </ul>
                )}
              </TabsContent>

              <TabsContent value="permissions" className="space-y-3 pt-4 text-sm">
                <p className="text-xs text-text-secondary">
                  {t("console.permissions.hint", { count: detail.effective_permissions.length })}
                </p>
                {permissionsByModule.map(([module, perms]) => (
                  <div key={module} className="space-y-1">
                    <h4 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
                      {t(`permissions.domainLabels.${module}`, module.toUpperCase())}
                    </h4>
                    <div className="flex flex-wrap gap-1">
                      {perms.map((p) => (
                        <span
                          key={p}
                          className="rounded border border-surface-border bg-surface-1 px-1.5 py-0.5 font-mono text-[11px]"
                        >
                          {p}
                        </span>
                      ))}
                    </div>
                  </div>
                ))}
              </TabsContent>

              <TabsContent value="history" className="pt-4">
                <AdminAuditTimeline
                  key={historyKey}
                  fixedFilter={{ target_user_id: user.id }}
                  compact
                />
              </TabsContent>
            </Tabs>
          ) : null}
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={pending !== null}
        onOpenChange={(v) => !v && setPending(null)}
        title={confirm.title}
        description={confirm.description}
        confirmLabel={confirm.label}
        destructive={
          pending?.kind === "disable" || pending?.kind === "revoke" || pending?.kind === "session"
        }
        confirmDisabled={reasonRequired && reason.trim().length === 0}
        onConfirm={runPending}
      >
        {reasonOffered ? (
          <div className="space-y-1.5">
            <Label htmlFor="admin-action-reason">
              {reasonRequired ? t("console.reason.required") : t("console.reason.optional")}
            </Label>
            <Textarea
              id="admin-action-reason"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              maxLength={1000}
              rows={3}
            />
            <p className="text-xs text-text-secondary">{t("console.reason.ledgerHint")}</p>
          </div>
        ) : null}
      </ConfirmDialog>

      <AssignRoleDialog
        user={user}
        open={assignOpen}
        onClose={() => setAssignOpen(false)}
        roles={roles}
        nodes={nodes}
        onAssigned={() => void afterChange()}
        withStepUp={withStepUp}
      />
    </>
  );
}
