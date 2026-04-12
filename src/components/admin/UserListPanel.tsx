import type { ColumnDef } from "@tanstack/react-table";
import { Eye, EyeOff, Plus, ShieldCheck, UserX } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
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
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { usePermissions } from "@/hooks/use-permissions";
import { useToast } from "@/hooks/use-toast";
import {
  listUsers,
  getUser,
  createUser,
  deactivateUser,
  assignRoleScope,
  revokeRoleScope,
  listRoles,
} from "@/services/rbac-service";
import type {
  UserWithRoles,
  UserDetail,
  UserListFilter,
  CreateUserInput,
  AssignRoleScopeInput,
  RoleWithPermissions,
} from "@shared/ipc-types";

// ── Detail sheet ────────────────────────────────────────────────────────────

function UserDetailSheet({
  userId,
  open,
  onClose,
  onDeactivate,
}: {
  userId: number | null;
  open: boolean;
  onClose: () => void;
  onDeactivate: (id: number) => void;
}) {
  const { t } = useTranslation("admin");
  const [detail, setDetail] = useState<UserDetail | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!userId || !open) return;
    setLoading(true);
    getUser(userId)
      .then(setDetail)
      .catch(() => setDetail(null))
      .finally(() => setLoading(false));
  }, [userId, open]);

  return (
    <Sheet open={open} onOpenChange={(v) => !v && onClose()}>
      <SheetContent className="overflow-y-auto sm:max-w-lg">
        <SheetHeader>
          <SheetTitle>{detail?.user.display_name ?? detail?.user.username ?? "…"}</SheetTitle>
        </SheetHeader>

        {loading && (
          <p className="mt-4 text-sm text-text-secondary">{t("common.loading", "Chargement…")}</p>
        )}

        {detail && !loading && (
          <div className="mt-4 space-y-6 text-sm">
            {/* Identity */}
            <section className="space-y-2">
              <h4 className="font-medium text-text-primary">
                {t("users.detail.identity", "Identité")}
              </h4>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-1">
                <dt className="text-text-secondary">{t("users.fields.username", "Identifiant")}</dt>
                <dd>{detail.user.username}</dd>
                <dt className="text-text-secondary">
                  {t("users.fields.identityMode", "Mode d'authentification")}
                </dt>
                <dd>
                  <Badge variant="outline">{detail.user.identity_mode}</Badge>
                </dd>
                <dt className="text-text-secondary">{t("users.fields.status", "Statut")}</dt>
                <dd>
                  <Badge variant={detail.user.is_active ? "default" : "destructive"}>
                    {detail.user.is_active
                      ? t("users.active", "Actif")
                      : t("users.inactive", "Inactif")}
                  </Badge>
                </dd>
                <dt className="text-text-secondary">
                  {t("users.fields.lastSeen", "Dernière connexion")}
                </dt>
                <dd>{detail.user.last_seen_at ?? "—"}</dd>
              </dl>
            </section>

            {/* Role assignments */}
            <section className="space-y-2">
              <h4 className="font-medium text-text-primary">
                {t("users.detail.roles", "Rôles assignés")}
              </h4>
              {detail.scope_assignments.length === 0 && (
                <p className="text-text-secondary">
                  {t("users.detail.noRoles", "Aucun rôle assigné.")}
                </p>
              )}
              <ul className="space-y-1">
                {detail.user.roles.map((r) => (
                  <li
                    key={r.assignment_id}
                    className="flex items-center justify-between rounded-md border border-surface-border px-3 py-2"
                  >
                    <div className="flex items-center gap-2">
                      <ShieldCheck className="h-4 w-4 text-primary" />
                      <span className="font-medium">{r.role_name}</span>
                      <Badge variant="outline" className="text-[10px]">
                        {r.scope_type}
                      </Badge>
                      {r.is_emergency && (
                        <Badge variant="destructive" className="text-[10px]">
                          {t("users.emergency", "Urgence")}
                        </Badge>
                      )}
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        void revokeRoleScope(r.assignment_id).then(onClose);
                      }}
                    >
                      {t("users.actions.revoke", "Révoquer")}
                    </Button>
                  </li>
                ))}
              </ul>
            </section>

            {/* Effective permissions */}
            <section className="space-y-2">
              <h4 className="font-medium text-text-primary">
                {t("users.detail.permissions", "Permissions effectives")}
                <Badge variant="secondary" className="ml-2">
                  {detail.effective_permissions.length}
                </Badge>
              </h4>
              <div className="flex flex-wrap gap-1">
                {detail.effective_permissions.map((p) => (
                  <Badge key={p} variant="outline" className="text-[10px]">
                    {p}
                  </Badge>
                ))}
              </div>
            </section>

            {/* Actions */}
            {detail.user.is_active && (
              <div className="pt-2">
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => {
                    onDeactivate(detail.user.id);
                    onClose();
                  }}
                >
                  <UserX className="mr-1.5 h-4 w-4" />
                  {t("users.actions.deactivate", "Désactiver")}
                </Button>
              </div>
            )}
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}

// ── Password strength ───────────────────────────────────────────────────────

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

const STRENGTH_STYLES: Record<PasswordStrength, { bar: string; label: string }> = {
  weak: { bar: "bg-red-500 w-1/3", label: "Faible" },
  medium: { bar: "bg-amber-500 w-2/3", label: "Moyen" },
  strong: { bar: "bg-emerald-500 w-full", label: "Fort" },
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
      <p className="text-[10px] text-text-secondary">
        {t(`users.create.strength.${strength}`, style.label)}
      </p>
    </div>
  );
}

// ── Create-user dialog ──────────────────────────────────────────────────────

function CreateUserDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
}) {
  const { t } = useTranslation("admin");
  const { toast } = useToast();

  const [username, setUsername] = useState("");
  const [identityMode, setIdentityMode] = useState("local");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [forceChange, setForceChange] = useState(true);
  const [submitting, setSubmitting] = useState(false);

  const showPasswordFields = identityMode !== "sso";
  const passwordRequired = identityMode === "local";

  const passwordError = useMemo(() => {
    if (!password && !passwordRequired) return null;
    if (password.length > 0 && password.length < 8)
      return t("users.create.passwordTooShort", "8 caractères minimum");
    if (password && !/[A-Z]/.test(password))
      return t("users.create.passwordNeedUpper", "Une majuscule requise");
    if (password && !/[a-z]/.test(password))
      return t("users.create.passwordNeedLower", "Une minuscule requise");
    if (password && !/\d/.test(password))
      return t("users.create.passwordNeedDigit", "Un chiffre requis");
    return null;
  }, [password, passwordRequired, t]);

  const confirmError = useMemo(() => {
    if (confirmPassword && password !== confirmPassword)
      return t("users.create.passwordMismatch", "Les mots de passe ne correspondent pas");
    return null;
  }, [password, confirmPassword, t]);

  const canSubmit =
    username.trim().length > 0 &&
    !submitting &&
    (identityMode === "sso" ||
      (password.length >= 8 &&
        !passwordError &&
        confirmPassword.length > 0 &&
        password === confirmPassword));

  const handleSubmit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      const input: CreateUserInput = {
        username: username.trim(),
        identity_mode: identityMode,
        ...(showPasswordFields && password
          ? { initial_password: password, force_password_change: forceChange }
          : {}),
      };
      await createUser(input);
      toast({ title: t("users.create.success", "Utilisateur créé"), variant: "success" });
      onCreated();
      onClose();
      // Reset form
      setUsername("");
      setPassword("");
      setConfirmPassword("");
      setIdentityMode("local");
      setForceChange(true);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      toast({
        title: msg || t("users.create.error", "Erreur lors de la création"),
        variant: "destructive",
      });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("users.create.title", "Nouvel utilisateur")}</DialogTitle>
          <DialogDescription>
            {t(
              "users.create.description",
              "Créez un compte utilisateur et définissez son mode d'authentification.",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Username */}
          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("users.fields.username", "Identifiant")} *
            </label>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="jean.dupont"
              autoComplete="off"
            />
          </div>

          {/* Identity mode */}
          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("users.fields.identityMode", "Mode d'authentification")}
            </label>
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
                <SelectItem value="local">{t("users.filter.local", "Local")}</SelectItem>
                <SelectItem value="sso">{t("users.filter.sso", "SSO")}</SelectItem>
                <SelectItem value="hybrid">{t("users.filter.hybrid", "Hybride")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Password fields */}
          {showPasswordFields && (
            <>
              <div className="space-y-1.5">
                <label className="text-sm font-medium">
                  {t("users.create.password", "Mot de passe")}
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
                {passwordError && <p className="text-xs text-red-600">{passwordError}</p>}
                <PasswordStrengthBar password={password} />
              </div>

              <div className="space-y-1.5">
                <label className="text-sm font-medium">
                  {t("users.create.confirmPassword", "Confirmer le mot de passe")}
                </label>
                <Input
                  type="password"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  autoComplete="new-password"
                />
                {confirmError && <p className="text-xs text-red-600">{confirmError}</p>}
              </div>

              <div className="flex items-center gap-2">
                <Checkbox
                  id="force-change"
                  checked={forceChange}
                  onCheckedChange={(checked) => setForceChange(checked === true)}
                />
                <label htmlFor="force-change" className="text-sm">
                  {t("users.create.forceChange", "Forcer le changement au premier login")}
                </label>
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel", "Annuler")}
          </Button>
          <Button onClick={handleSubmit} disabled={!canSubmit}>
            {t("users.create.confirm", "Créer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Assign-role dialog ──────────────────────────────────────────────────────

function AssignRoleDialog({
  user,
  open,
  onClose,
  roles,
  onAssigned,
}: {
  user: UserWithRoles | null;
  open: boolean;
  onClose: () => void;
  roles: RoleWithPermissions[];
  onAssigned: () => void;
}) {
  const { t } = useTranslation("admin");
  const [selectedRole, setSelectedRole] = useState<string>("");
  const [scopeType, setScopeType] = useState("global");
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async () => {
    if (!user || !selectedRole) return;
    setSubmitting(true);
    try {
      const input: AssignRoleScopeInput = {
        user_id: user.id,
        role_id: Number(selectedRole),
        scope_type: scopeType,
      };
      await assignRoleScope(input);
      onAssigned();
      onClose();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t("users.assignRole.title", "Assigner un rôle")} — {user?.username}
          </DialogTitle>
          <DialogDescription>
            {t("users.assignRole.description", "Sélectionnez un rôle et un type de périmètre.")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.assignRole.role", "Rôle")}</label>
            <Select value={selectedRole} onValueChange={setSelectedRole}>
              <SelectTrigger>
                <SelectValue placeholder={t("users.assignRole.selectRole", "Choisir un rôle")} />
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

          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("users.assignRole.scope", "Périmètre")}
            </label>
            <Select value={scopeType} onValueChange={setScopeType}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="global">{t("scope.global", "Global")}</SelectItem>
                <SelectItem value="site">{t("scope.site", "Site")}</SelectItem>
                <SelectItem value="department">{t("scope.department", "Département")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel", "Annuler")}
          </Button>
          <Button onClick={handleSubmit} disabled={!selectedRole || submitting}>
            {t("users.assignRole.confirm", "Assigner")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Main panel ──────────────────────────────────────────────────────────────

export function UserListPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { toast } = useToast();

  // Data
  const [users, setUsers] = useState<UserWithRoles[]>([]);
  const [roles, setRoles] = useState<RoleWithPermissions[]>([]);
  const [loading, setLoading] = useState(true);

  // Filters
  const [search, setSearch] = useState("");
  const [activeFilter, setActiveFilter] = useState<string>("all");
  const [modeFilter, setModeFilter] = useState<string>("all");

  // Detail sheet
  const [detailUserId, setDetailUserId] = useState<number | null>(null);

  // Assign dialog
  const [assignTarget, setAssignTarget] = useState<UserWithRoles | null>(null);

  // Create dialog
  const [showCreate, setShowCreate] = useState(false);

  const buildFilter = useCallback((): UserListFilter => {
    const f: UserListFilter = {};
    if (search.trim()) f.search = search.trim();
    if (activeFilter === "active") f.is_active = true;
    if (activeFilter === "inactive") f.is_active = false;
    if (modeFilter !== "all") f.identity_mode = modeFilter;
    return f;
  }, [search, activeFilter, modeFilter]);

  const fetchUsers = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listUsers(buildFilter());
      setUsers(data);
    } catch {
      toast({
        title: t("users.errors.loadFailed", "Erreur de chargement des utilisateurs"),
        variant: "destructive",
      });
    } finally {
      setLoading(false);
    }
  }, [buildFilter, toast, t]);

  // Initial load + roles
  useEffect(() => {
    void fetchUsers();
    listRoles()
      .then(setRoles)
      .catch(() => {});
  }, [fetchUsers]);

  // Deactivate handler
  const handleDeactivate = useCallback(
    async (userId: number) => {
      try {
        await deactivateUser(userId);
        toast({ title: t("users.deactivated", "Utilisateur désactivé"), variant: "success" });
        void fetchUsers();
      } catch {
        toast({
          title: t("users.errors.deactivateFailed", "Erreur lors de la désactivation"),
          variant: "destructive",
        });
      }
    },
    [fetchUsers, toast, t],
  );

  // Columns
  const columns: ColumnDef<UserWithRoles>[] = useMemo(
    () => [
      {
        accessorKey: "username",
        header: t("users.columns.username", "Identifiant"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.username}</span>,
      },
      {
        accessorKey: "display_name",
        header: t("users.columns.displayName", "Nom affiché"),
        cell: ({ row }) => row.original.display_name ?? "—",
      },
      {
        accessorKey: "identity_mode",
        header: t("users.columns.identityMode", "Auth"),
        cell: ({ row }) => (
          <Badge variant="outline" className="text-[10px]">
            {row.original.identity_mode}
          </Badge>
        ),
      },
      {
        id: "status",
        header: t("users.columns.status", "Statut"),
        cell: ({ row }) => (
          <Badge variant={row.original.is_active ? "default" : "destructive"}>
            {row.original.is_active ? t("users.active", "Actif") : t("users.inactive", "Inactif")}
          </Badge>
        ),
      },
      {
        id: "roleCount",
        header: t("users.columns.roles", "Rôles"),
        cell: ({ row }) => <Badge variant="secondary">{row.original.roles.length}</Badge>,
      },
      {
        accessorKey: "last_seen_at",
        header: t("users.columns.lastSeen", "Dernière connexion"),
        cell: ({ row }) => (
          <span className="text-xs text-text-secondary">{row.original.last_seen_at ?? "—"}</span>
        ),
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) => (
          <div className="flex items-center gap-1">
            {can("adm.users") && (
              <Button
                variant="ghost"
                size="sm"
                onClick={(e) => {
                  e.stopPropagation();
                  setAssignTarget(row.original);
                }}
              >
                <ShieldCheck className="h-3.5 w-3.5" />
              </Button>
            )}
            {can("adm.users") && row.original.is_active && (
              <Button
                variant="ghost"
                size="sm"
                onClick={(e) => {
                  e.stopPropagation();
                  void handleDeactivate(row.original.id);
                }}
              >
                <UserX className="h-3.5 w-3.5 text-destructive" />
              </Button>
            )}
          </div>
        ),
      },
    ],
    [t, can, handleDeactivate],
  );

  return (
    <div className="space-y-4">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-3">
        <Input
          placeholder={t("users.search", "Rechercher un utilisateur…")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="h-9 w-64"
        />

        <Select value={activeFilter} onValueChange={setActiveFilter}>
          <SelectTrigger className="h-9 w-36">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("users.filter.all", "Tous")}</SelectItem>
            <SelectItem value="active">{t("users.filter.active", "Actifs")}</SelectItem>
            <SelectItem value="inactive">{t("users.filter.inactive", "Inactifs")}</SelectItem>
          </SelectContent>
        </Select>

        <Select value={modeFilter} onValueChange={setModeFilter}>
          <SelectTrigger className="h-9 w-36">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("users.filter.allModes", "Tous modes")}</SelectItem>
            <SelectItem value="local">{t("users.filter.local", "Local")}</SelectItem>
            <SelectItem value="sso">{t("users.filter.sso", "SSO")}</SelectItem>
            <SelectItem value="hybrid">{t("users.filter.hybrid", "Hybride")}</SelectItem>
          </SelectContent>
        </Select>

        <div className="ml-auto">
          {can("adm.users") && (
            <Button size="sm" onClick={() => setShowCreate(true)}>
              <Plus className="mr-1.5 h-4 w-4" />
              {t("users.create", "Nouvel utilisateur")}
            </Button>
          )}
        </div>
      </div>

      {/* Table */}
      <DataTable
        columns={columns}
        data={users}
        isLoading={loading}
        skeletonRows={8}
        pageSize={20}
        onRowClick={(row) => setDetailUserId(row.id)}
      />

      {/* Detail side-sheet */}
      <UserDetailSheet
        userId={detailUserId}
        open={detailUserId !== null}
        onClose={() => {
          setDetailUserId(null);
          void fetchUsers();
        }}
        onDeactivate={handleDeactivate}
      />

      {/* Assign role dialog */}
      <AssignRoleDialog
        user={assignTarget}
        open={assignTarget !== null}
        onClose={() => setAssignTarget(null)}
        roles={roles}
        onAssigned={() => void fetchUsers()}
      />

      {/* Create user dialog */}
      <CreateUserDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={() => void fetchUsers()}
      />
    </div>
  );
}
