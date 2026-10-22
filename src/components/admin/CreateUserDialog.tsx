/**
 * CreateUserDialog — extracted from UserListPanel for reuse across the app.
 *
 * When `lockPersonnel` is true and `initialPersonnelId` is set, the personnel
 * link is pre-filled and the user cannot change it (used from PersonnelCreateDialog).
 */
import { Eye, EyeOff } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PLANT_SCOPE, ScopeNodeSelect, type ScopeChoice } from "@/components/admin/ScopeNodeSelect";
import { PersonnelPickerCombobox } from "@/components/personnel/PersonnelCard";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { usePermissions } from "@/hooks/use-permissions";
import { isStepUpCancelled, useStepUp } from "@/hooks/use-step-up";
import { appErrorReason } from "@/lib/app-error";
import { listPersonnel } from "@/services/personnel-service";
import { createUser, listAssignableRoles, listAssignableScopeNodes } from "@/services/rbac-service";
import { pushAppToast } from "@/store/app-toast-store";
import type {
  AssignableRoleSummary,
  AssignableScopeNode,
  CreateUserInput,
  Personnel,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

// ── Password strength ─────────────────────────────────────────────────────

type PasswordStrength = "weak" | "medium" | "strong";

function computeStrength(password: string): PasswordStrength {
  if (password.length < 8) return "weak";
  const hasUpper = /[A-Z]/.test(password);
  const hasLower = /[a-z]/.test(password);
  const hasDigit = /\d/.test(password);
  const hasSpecial = /[^A-Za-z0-9]/.test(password);
  const score = [hasUpper, hasLower, hasDigit, hasSpecial].filter(Boolean).length;
  if (score >= 4 && password.length >= 12) return "strong";
  if (score >= 3 && password.length >= 8) return "medium";
  return "weak";
}

const STRENGTH_STYLES: Record<PasswordStrength, { bar: string }> = {
  weak: { bar: "bg-tone-danger-fg w-1/3" },
  medium: { bar: "bg-tone-warning-fg w-2/3" },
  strong: { bar: "bg-tone-success-fg w-full" },
};

function PasswordStrengthBar({ password }: { password: string }) {
  const { t } = useTranslation("admin");
  if (!password) return null;
  const strength = computeStrength(password);
  const style = STRENGTH_STYLES[strength];
  return (
    <div className="space-y-1">
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface-2">
        <div className={`h-full rounded-full transition-all ${style.bar}`} />
      </div>
      <p className="text-[10px] text-text-secondary">{t(`users.create.strength.${strength}`)}</p>
    </div>
  );
}

// ── CreateUserDialog ──────────────────────────────────────────────────────

export interface CreateUserDialogProps {
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
  /** Pre-fill and optionally lock the personnel link (e.g. from PersonnelCreateDialog). */
  initialPersonnelId?: number | null;
  /** When true, the personnel picker is read-only. */
  lockPersonnel?: boolean;
}

export function CreateUserDialog({
  open,
  onClose,
  onCreated,
  initialPersonnelId,
  lockPersonnel = false,
}: CreateUserDialogProps) {
  const { t } = useTranslation("admin");
  // The global toast host shows them (a local useToast list was never rendered here).
  const toast = pushAppToast;
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const canViewPersonnel = can(P.PER_VIEW);

  const [username, setUsername] = useState("");
  const [identityMode, setIdentityMode] = useState("local");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [forceChange, setForceChange] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [personnelId, setPersonnelId] = useState<number | null>(initialPersonnelId ?? null);
  const [personnelItems, setPersonnelItems] = useState<Personnel[]>([]);
  const [personnelLoading, setPersonnelLoading] = useState(false);
  const [assignableRoles, setAssignableRoles] = useState<AssignableRoleSummary[]>([]);
  const [rolesLoading, setRolesLoading] = useState(false);
  const [rolesLoadError, setRolesLoadError] = useState<string | null>(null);
  const [roleIdStr, setRoleIdStr] = useState("");
  const [scopeNodes, setScopeNodes] = useState<AssignableScopeNode[]>([]);
  const [scope, setScope] = useState<ScopeChoice>(PLANT_SCOPE);

  const loadAssignableRoles = useCallback(async () => {
    setRolesLoading(true);
    setRolesLoadError(null);
    try {
      const [list, nodes] = await Promise.all([listAssignableRoles(), listAssignableScopeNodes()]);
      setAssignableRoles(list);
      setScopeNodes(nodes);
    } catch {
      setAssignableRoles([]);
      setScopeNodes([]);
      const msg = t("users.create.rolesLoadError");
      setRolesLoadError(msg);
      toast({ title: msg, variant: "destructive" });
    } finally {
      setRolesLoading(false);
    }
  }, [t, toast]);

  useEffect(() => {
    if (!open) {
      setAssignableRoles([]);
      setRoleIdStr("");
      setScope(PLANT_SCOPE);
      setRolesLoadError(null);
      return;
    }
    void loadAssignableRoles();
  }, [open, loadAssignableRoles]);

  // Initialize personnelId from prop when dialog opens
  useEffect(() => {
    if (open) {
      setPersonnelId(initialPersonnelId ?? null);
    }
  }, [open, initialPersonnelId]);

  useEffect(() => {
    if (!open) {
      if (!lockPersonnel) setPersonnelId(null);
      return;
    }
    if (!canViewPersonnel || lockPersonnel) return;
    let cancelled = false;
    setPersonnelLoading(true);
    void listPersonnel({ limit: 2000, offset: 0 })
      .then((page) => {
        if (!cancelled) setPersonnelItems(page.items);
      })
      .catch(() => {
        if (!cancelled) {
          setPersonnelItems([]);
          toast({
            title: t("users.create.personnelLoadError"),
            variant: "destructive",
          });
        }
      })
      .finally(() => {
        if (!cancelled) setPersonnelLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, canViewPersonnel, lockPersonnel, t, toast]);

  const showPasswordFields = identityMode !== "sso";
  const passwordRequired = identityMode === "local";

  const passwordError = useMemo(() => {
    if (!password && !passwordRequired) return null;
    if (password.length > 0 && password.length < 8) return t("users.create.passwordTooShort");
    if (password && !/[A-Z]/.test(password)) return t("users.create.passwordNeedUpper");
    if (password && !/[a-z]/.test(password)) return t("users.create.passwordNeedLower");
    if (password && !/\d/.test(password)) return t("users.create.passwordNeedDigit");
    return null;
  }, [password, passwordRequired, t]);

  const confirmError = useMemo(() => {
    if (confirmPassword && password !== confirmPassword) return t("users.create.passwordMismatch");
    return null;
  }, [password, confirmPassword, t]);

  const roleIdParsed = roleIdStr ? Number.parseInt(roleIdStr, 10) : Number.NaN;
  const roleSelectionValid =
    roleIdStr.length > 0 &&
    Number.isFinite(roleIdParsed) &&
    roleIdParsed > 0 &&
    assignableRoles.some((r) => r.id === roleIdParsed);

  const canSubmit =
    username.trim().length > 0 &&
    roleSelectionValid &&
    !rolesLoading &&
    !submitting &&
    (identityMode === "sso" ||
      (password.length >= 8 &&
        !passwordError &&
        confirmPassword.length > 0 &&
        password === confirmPassword));

  const handleSubmit = async () => {
    if (!canSubmit) return;
    const rid = Number.parseInt(roleIdStr, 10);
    if (!Number.isFinite(rid) || rid <= 0 || !assignableRoles.some((r) => r.id === rid)) {
      toast({
        title: t("users.create.roleRequired"),
        variant: "destructive",
      });
      return;
    }
    setSubmitting(true);
    try {
      const input: CreateUserInput = {
        username: username.trim(),
        identity_mode: identityMode,
        role_id: rid,
        ...(scope !== PLANT_SCOPE ? { scope_node_id: Number.parseInt(scope, 10) } : {}),
        ...(personnelId != null ? { personnel_id: personnelId } : {}),
        ...(showPasswordFields && password
          ? { initial_password: password, force_password_change: forceChange }
          : {}),
      };
      await withStepUp(() => createUser(input));
      toast({ title: t("users.create.success"), variant: "success" });
      onCreated();
      onClose();
      setUsername("");
      setPassword("");
      setConfirmPassword("");
      setIdentityMode("local");
      setForceChange(true);
      if (!lockPersonnel) setPersonnelId(null);
      setRoleIdStr("");
      setScope(PLANT_SCOPE);
    } catch (err) {
      if (!isStepUpCancelled(err)) {
        toast({
          title: appErrorReason(err) || t("users.create.error"),
          variant: "destructive",
        });
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("users.create.title")}</DialogTitle>
          <DialogDescription>{t("users.create.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Username */}
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.fields.username")} *</label>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t("users.create.usernamePlaceholder")}
              autoComplete="off"
            />
          </div>

          {/* Initial tenant role (required) */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between gap-2">
              <label className="text-sm font-medium" htmlFor="create-user-role">
                {t("users.create.roleLabel")} *
              </label>
              {rolesLoadError ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 shrink-0 text-xs"
                  disabled={rolesLoading}
                  onClick={() => void loadAssignableRoles()}
                >
                  {t("users.create.rolesRetry")}
                </Button>
              ) : null}
            </div>
            <p className="text-xs text-text-secondary">{t("users.create.roleHint")}</p>
            {rolesLoading ? (
              <p className="text-xs text-text-secondary">{t("common.loading")}</p>
            ) : rolesLoadError ? (
              <p className="text-xs text-tone-danger-fg" role="alert">
                {rolesLoadError}
              </p>
            ) : assignableRoles.length === 0 ? (
              <p className="rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-xs text-tone-warning-fg">
                {t("users.create.noRolesAvailable")}
              </p>
            ) : (
              <Select value={roleIdStr} onValueChange={setRoleIdStr} disabled={submitting}>
                <SelectTrigger id="create-user-role" className="w-full">
                  <SelectValue placeholder={t("users.create.rolePlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {assignableRoles.map((r) => (
                    <SelectItem
                      key={r.id}
                      value={String(r.id)}
                      title={r.description ?? undefined}
                      textValue={r.name}
                    >
                      {r.is_system ? `${r.name} (${t("users.create.roleSystemBadge")})` : r.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </div>

          {/* Where the role applies: the whole plant, or one site / node and below */}
          <div className="space-y-1.5">
            <label className="text-sm font-medium" htmlFor="create-user-scope">
              {t("users.create.scopeLabel")} *
            </label>
            <p className="text-xs text-text-secondary">{t("users.create.scopeHint")}</p>
            <ScopeNodeSelect
              id="create-user-scope"
              nodes={scopeNodes}
              value={scope}
              onChange={setScope}
              disabled={submitting || rolesLoading}
            />
          </div>

          {/* Optional link to personnel */}
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.create.linkPersonnel")}</label>
            <p className="text-xs text-text-secondary">{t("users.create.linkPersonnelHint")}</p>
            {lockPersonnel ? (
              <p className="rounded-md border bg-surface-2 px-3 py-2 text-xs text-text-secondary">
                {personnelId != null
                  ? t("users.create.personnelLocked", {
                      id: personnelId,
                    })
                  : t("users.create.personnelNone")}
              </p>
            ) : canViewPersonnel ? (
              personnelLoading ? (
                <p className="text-xs text-text-secondary">{t("common.loading")}</p>
              ) : (
                <PersonnelPickerCombobox
                  items={personnelItems}
                  value={personnelId}
                  onChange={setPersonnelId}
                  disabled={submitting}
                  placeholder={t("users.create.personnelSearchPlaceholder")}
                />
              )
            ) : (
              <p className="text-xs text-tone-warning-fg">
                {t("users.create.noPersonnelPermission")}
              </p>
            )}
          </div>

          {/* Identity mode */}
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.fields.identityMode")}</label>
            <Select
              value={identityMode}
              onValueChange={(v) => {
                setIdentityMode(v);
                setPassword("");
                setConfirmPassword("");
              }}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="local">{t("users.filter.local")}</SelectItem>
                <SelectItem value="sso">{t("users.filter.sso")}</SelectItem>
                <SelectItem value="hybrid">{t("users.filter.hybrid")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Password fields */}
          {showPasswordFields && (
            <>
              <div className="space-y-1.5">
                <label className="text-sm font-medium">
                  {t("users.create.password")}
                  {passwordRequired && " *"}
                </label>
                <div className="relative">
                  <Input
                    type={showPassword ? "text" : "password"}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    autoComplete="new-password"
                    className="pr-10"
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword(!showPassword)}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-text-secondary hover:text-text-primary"
                  >
                    {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
                {passwordError && <p className="text-xs text-tone-danger-fg">{passwordError}</p>}
                <PasswordStrengthBar password={password} />
              </div>

              <div className="space-y-1.5">
                <label className="text-sm font-medium">{t("users.create.confirmPassword")}</label>
                <Input
                  type="password"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  autoComplete="new-password"
                />
                {confirmError && <p className="text-xs text-tone-danger-fg">{confirmError}</p>}
              </div>

              <div className="flex items-center gap-2">
                <Checkbox
                  id="force-change"
                  checked={forceChange}
                  onCheckedChange={(checked) => setForceChange(checked === true)}
                />
                <label htmlFor="force-change" className="text-sm">
                  {t("users.create.forceChange")}
                </label>
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button onClick={() => void handleSubmit()} disabled={!canSubmit}>
            {t("users.create.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
      {StepUpDialogElement}
    </Dialog>
  );
}
