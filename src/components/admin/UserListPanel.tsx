import type { ColumnDef } from "@tanstack/react-table";
import { ArrowRight, Loader2, LockOpen, Pencil, Plus, ShieldCheck, UserX } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { CreateUserDialog } from "@/components/admin/CreateUserDialog";
import { OnlinePresenceIndicator } from "@/components/admin/OnlinePresenceIndicator";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { PersonnelPickerCombobox } from "@/components/personnel/PersonnelCard";
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
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useToast } from "@/hooks/use-toast";
import { listPersonnel } from "@/services/personnel-service";
import {
  listUsers,
  getUser,
  deactivateUser,
  updateUser,
  assignRoleScope,
  revokeRoleScope,
  listRoles,
  unlockUserAccount,
} from "@/services/rbac-service";
import type {
  UserWithRoles,
  UserDetail,
  UserListFilter,
  UpdateUserInput,
  AssignRoleScopeInput,
  RoleWithPermissions,
  Personnel,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

// â”€â”€ Detail modal â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{detail?.user.display_name ?? detail?.user.username ?? "â€¦"}</DialogTitle>
        </DialogHeader>

        {loading && (
          <p className="mt-4 text-sm text-text-secondary">{t("common.loading", "Chargementâ€¦")}</p>
        )}

        {detail && !loading && (
          <div className="mt-4 space-y-6 text-sm">
            {/* Identity */}
            <section className="space-y-2">
              <h4 className="font-medium text-text-primary">
                {t("users.detail.identity", "IdentitÃ©")}
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
                  {t("users.fields.lastSeen", "DerniÃ¨re connexion")}
                </dt>
                <dd>{detail.user.last_seen_at ?? "â€”"}</dd>
              </dl>
            </section>

            {/* Role assignments */}
            <section className="space-y-2">
              <h4 className="font-medium text-text-primary">
                {t("users.detail.roles", "RÃ´les assignÃ©s")}
              </h4>
              {detail.scope_assignments.length === 0 && (
                <p className="text-text-secondary">
                  {t("users.detail.noRoles", "Aucun rÃ´le assignÃ©.")}
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
                      {t("users.actions.revoke", "RÃ©voquer")}
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
                  {t("users.actions.deactivate", "DÃ©sactiver")}
                </Button>
              </div>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

// â”€â”€ CreateUserDialog â€” imported from CreateUserDialog.tsx â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// â”€â”€ Assign-role dialog â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

function AssignRoleDialog({
  user,
  open,
  onClose,
  roles,
  onAssigned,
  withStepUp,
}: {
  user: UserWithRoles | null;
  open: boolean;
  onClose: () => void;
  roles: RoleWithPermissions[];
  onAssigned: () => void;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
}) {
  const { t } = useTranslation("admin");
  const [selectedRole, setSelectedRole] = useState<string>("");
  /** Must match backend `assign_role_scope` allowed values: tenant, entity, site, team, org_node */
  const [scopeType, setScopeType] = useState("tenant");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!open || !user) return;
    setSelectedRole("");
    setScopeType("tenant");
  }, [open, user]);

  /** Existing non-emergency assignment for the selected scope (same scope_type, prefer null ref). */
  const assignmentAtScope = useMemo(() => {
    if (!user) return null;
    const same = user.roles.filter((r) => r.scope_type === scopeType);
    if (same.length === 0) return null;
    const nonEm = same.filter((r) => !r.is_emergency);
    const pool = nonEm.length > 0 ? nonEm : same;
    return pool.find((r) => r.scope_reference == null) ?? pool[0];
  }, [user, scopeType]);

  const handleSubmit = async () => {
    if (!user || !selectedRole) return;
    setSubmitting(true);
    try {
      const input: AssignRoleScopeInput = {
        user_id: user.id,
        role_id: Number(selectedRole),
        scope_type: scopeType,
      };
      await withStepUp(() => assignRoleScope(input));
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
            {t("users.assignRole.title", "Assigner un rÃ´le")} â€” {user?.username}
          </DialogTitle>
          <DialogDescription>
            {t("users.assignRole.description", "SÃ©lectionnez un rÃ´le et un type de pÃ©rimÃ¨tre.")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("users.assignRole.scope", "PÃ©rimÃ¨tre")}
            </label>
            <Select value={scopeType} onValueChange={setScopeType}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="tenant">{t("scope.global", "Global")}</SelectItem>
                <SelectItem value="site">{t("scope.site", "Site")}</SelectItem>
                <SelectItem value="org_node">{t("scope.department", "DÃ©partement")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">
              {t("users.assignRole.replacement", "Ancien rÃ´le â†’ nouveau rÃ´le")}
            </label>
            <p className="text-xs text-text-secondary">
              {t(
                "users.assignRole.replacementHint",
                "Pour ce pÃ©rimÃ¨tre, une seule attribution Ã  la fois : le nouveau rÃ´le remplace lâ€™actuel (y compris une Ã©lÃ©vation dâ€™urgence Ã  ce niveau).",
              )}
            </p>
            <div
              className="flex min-h-[2.5rem] flex-col gap-2 rounded-md border border-surface-border bg-surface-1/50 p-3 sm:flex-row sm:items-stretch"
              aria-label={t("users.assignRole.replacement", "Ancien rÃ´le â†’ nouveau rÃ´le")}
            >
              <div className="flex min-w-0 flex-1 flex-col justify-center gap-1 sm:pr-1">
                <span className="text-xs text-text-secondary">
                  {t("users.assignRole.current", "RÃ´le actuel")}
                </span>
                {assignmentAtScope ? (
                  <span className="font-medium text-foreground">
                    {assignmentAtScope.role_name}
                    {assignmentAtScope.is_emergency ? (
                      <span className="ml-1 text-xs font-normal text-amber-700">
                        {t("users.assignRole.emergencyBadge", "(urgence)")}
                      </span>
                    ) : null}
                  </span>
                ) : (
                  <span className="text-sm text-text-secondary">
                    {t("users.assignRole.noRoleAtScope", "Aucun")}
                  </span>
                )}
              </div>
              <div
                className="flex flex-shrink-0 items-center justify-center self-center sm:self-auto"
                aria-hidden
              >
                <ArrowRight className="h-5 w-5 text-text-secondary sm:h-6 sm:w-6" strokeWidth={2} />
              </div>
              <div className="flex min-w-0 flex-1 flex-col gap-1.5 sm:pl-1">
                <span className="text-xs text-text-secondary">
                  {t("users.assignRole.newLabel", "Nouveau rÃ´le")}
                </span>
                <Select value={selectedRole} onValueChange={setSelectedRole}>
                  <SelectTrigger className="w-full min-w-0">
                    <SelectValue
                      placeholder={t("users.assignRole.selectRole", "Choisir un rÃ´le")}
                    />
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
            </div>
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

// â”€â”€ Edit-user dialog â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

function EditUserDialog({
  user,
  open,
  onClose,
  onSaved,
}: {
  user: UserWithRoles | null;
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("admin");
  const { toast } = useToast();
  const { can } = usePermissions();
  const canViewPersonnel = can(P.PER_VIEW);
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [isActive, setIsActive] = useState(true);
  const [forcePasswordChange, setForcePasswordChange] = useState(false);
  const [personnelId, setPersonnelId] = useState<number | null>(null);
  const [personnelItems, setPersonnelItems] = useState<Personnel[]>([]);
  const [personnelLoading, setPersonnelLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open || !user) return;
    setUsername(user.username);
    setDisplayName(user.display_name ?? "");
    setEmail(user.email ?? "");
    setPhone(user.phone ?? "");
    setIsActive(user.is_active);
    setForcePasswordChange(user.force_password_change);
    setPersonnelId(user.personnel_id ?? null);
  }, [open, user]);

  useEffect(() => {
    if (!open || !canViewPersonnel) return;
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
            title: t("users.create.personnelLoadError", "Could not load personnel list"),
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
  }, [open, canViewPersonnel, t, toast]);

  const emailError = useMemo(() => {
    const normalized = email.trim();
    if (!normalized) return null;
    const basicEmailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    if (!basicEmailRegex.test(normalized)) {
      return t("users.edit.invalidEmail", "Please enter a valid email address.");
    }
    return null;
  }, [email, t]);

  const phoneError = useMemo(() => {
    const normalized = phone.trim();
    if (!normalized) return null;
    const cleaned = normalized.replace(/[^\d+]/g, "");
    const candidate = cleaned.startsWith("00")
      ? `+${cleaned.slice(2)}`
      : cleaned.startsWith("+")
        ? cleaned
        : `+${cleaned}`;
    const digits = candidate.replace(/\D/g, "");
    if (!candidate.startsWith("+") || digits.length < 8 || digits.length > 15) {
      return t("users.edit.invalidPhone", "Please enter a valid phone number (E.164).");
    }
    return null;
  }, [phone, t]);

  const canSave = username.trim().length > 0 && !emailError && !phoneError && !saving;

  const handleSave = async () => {
    if (!user || !canSave) return;

    setSaving(true);
    try {
      const payload: UpdateUserInput = {
        user_id: user.id,
        username: username.trim(),
        display_name: displayName.trim() || "",
        email: email.trim() || "",
        phone: phone.trim() || "",
        ...(personnelId != null ? { personnel_id: personnelId } : {}),
        is_active: isActive,
        force_password_change: forcePasswordChange,
      };
      await updateUser(payload);

      toast({ title: t("users.edit.success", "User updated"), variant: "success" });
      onSaved();
      onClose();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      toast({
        title: msg || t("users.edit.error", "Failed to update user"),
        variant: "destructive",
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("users.edit.title", "Edit user")}</DialogTitle>
          <DialogDescription>
            {t(
              "users.edit.description",
              "Update account identity and flags. To change permissions, use Assign role (shield) on a user row.",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="grid grid-cols-1 gap-3 rounded-md border border-surface-border bg-surface-1/50 p-3 sm:grid-cols-2">
            <div>
              <p className="text-xs text-text-secondary">
                {t("users.fields.identityMode", "Authentication mode")}
              </p>
              <p className="text-sm font-medium">{user?.identity_mode ?? "â€”"}</p>
            </div>
            <div>
              <p className="text-xs text-text-secondary">
                {t("users.fields.lastSeen", "Last seen")}
              </p>
              <p className="text-sm font-medium">{user?.last_seen_at ?? "â€”"}</p>
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("users.fields.displayName", "Display name")}
            </label>
            <Input
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              disabled={saving}
              autoComplete="off"
            />
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <label className="text-sm font-medium">
                {t("users.fields.email", "Email Professionnel")}
              </label>
              <Input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                disabled={saving}
                placeholder="name@company.com"
                autoComplete="email"
              />
              {emailError && <p className="text-xs text-red-600">{emailError}</p>}
            </div>

            <div className="space-y-1.5">
              <label className="text-sm font-medium">
                {t("users.fields.phone", "NumÃ©ro de TÃ©lÃ©phone")}
              </label>
              <Input
                type="tel"
                value={phone}
                onChange={(e) => setPhone(e.target.value)}
                disabled={saving}
                placeholder="+33 6 12 34 56 78"
                autoComplete="tel"
              />
              {phoneError && <p className="text-xs text-red-600">{phoneError}</p>}
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.fields.username", "Username")}</label>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              disabled={saving}
              autoComplete="off"
            />
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("users.create.linkPersonnel", "Link to existing personnel")}
            </label>
            {canViewPersonnel ? (
              personnelLoading ? (
                <p className="text-xs text-text-secondary">
                  {t("common.loading", "Chargementâ€¦")}
                </p>
              ) : (
                <PersonnelPickerCombobox
                  items={personnelItems}
                  value={personnelId}
                  onChange={setPersonnelId}
                  disabled={saving}
                  placeholder={t(
                    "users.create.personnelSearchPlaceholder",
                    "Search by name or employee codeâ€¦",
                  )}
                />
              )
            ) : (
              <p className="text-xs text-amber-800 dark:text-amber-200">
                {t(
                  "users.create.noPersonnelPermission",
                  "Listing personnel requires the Â« per.view Â» permission. You can create the account without a link and associate it later.",
                )}
              </p>
            )}
          </div>

          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={isActive} onCheckedChange={setIsActive} disabled={saving} />
            {t("users.active", "Active")}
          </label>

          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={forcePasswordChange}
              onCheckedChange={setForcePasswordChange}
              disabled={saving}
            />
            {t("users.create.forceChange", "Force password change on first login")}
          </label>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            {t("common.cancel", "Cancel")}
          </Button>
          <Button onClick={handleSave} disabled={!canSave}>
            {saving ? (
              <>
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
                {t("common.saving", "Savingâ€¦")}
              </>
            ) : (
              t("common.save", "Save")
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// â”€â”€ Main panel â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export function UserListPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { toast } = useToast();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  // Data
  const [users, setUsers] = useState<UserWithRoles[]>([]);
  const [roles, setRoles] = useState<RoleWithPermissions[]>([]);
  const [loading, setLoading] = useState(true);

  // Filters â€” searchInput is immediate UI; search is debounced (via SmartFilterBar)
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [activeFilter, setActiveFilter] = useState<string>("all");
  const [modeFilter, setModeFilter] = useState<string>("all");

  // Detail sheet
  const [detailUserId, setDetailUserId] = useState<number | null>(null);

  // Assign dialog
  const [assignTarget, setAssignTarget] = useState<UserWithRoles | null>(null);
  const [editTarget, setEditTarget] = useState<UserWithRoles | null>(null);

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
        await withStepUp(() => deactivateUser(userId));
        toast({ title: t("users.deactivated", "Utilisateur dÃ©sactivÃ©"), variant: "success" });
        void fetchUsers();
      } catch (err) {
        // User explicitly cancelled the step-up dialog.
        if (err instanceof Error && err.message.includes("Step-up cancelled")) {
          return;
        }
        toast({
          title: t("users.errors.deactivateFailed", "Erreur lors de la dÃ©sactivation"),
          variant: "destructive",
        });
      }
    },
    [fetchUsers, toast, t, withStepUp],
  );

  // Unlock handler
  const handleUnlock = useCallback(
    async (userId: number) => {
      try {
        await withStepUp(() => unlockUserAccount(userId));
        toast({ title: t("users.unlocked", "Compte dÃ©verrouillÃ©"), variant: "success" });
        void fetchUsers();
      } catch {
        toast({
          title: t("users.errors.unlockFailed", "Erreur lors du dÃ©verrouillage"),
          variant: "destructive",
        });
      }
    },
    [fetchUsers, toast, t, withStepUp],
  );

  // Columns
  const columns: ColumnDef<UserWithRoles>[] = useMemo(
    () => [
      {
        accessorKey: "username",
        header: t("users.columns.username", "Identifiant"),
        cell: ({ row }) => (
          <div className="flex items-center gap-1.5">
            <OnlinePresenceIndicator userId={row.original.id} />
            <span className="font-mono text-xs">{row.original.username}</span>
          </div>
        ),
      },
      {
        accessorKey: "display_name",
        header: t("users.columns.displayName", "Nom affichÃ©"),
        cell: ({ row }) => row.original.display_name ?? "â€”",
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
        cell: ({ row }) => {
          const isLocked =
            row.original.locked_until != null &&
            row.original.locked_until > new Date().toISOString();
          return (
            <div className="flex items-center gap-1">
              <Badge variant={row.original.is_active ? "default" : "destructive"}>
                {row.original.is_active
                  ? t("users.active", "Actif")
                  : t("users.inactive", "Inactif")}
              </Badge>
              {isLocked && (
                <Badge
                  variant="outline"
                  className="border-amber-300 bg-amber-50 text-amber-700 text-[10px]"
                >
                  {t("users.locked", "VerrouillÃ©")}
                </Badge>
              )}
            </div>
          );
        },
      },
      {
        id: "roles",
        header: t("users.columns.roles", "RÃ´les"),
        cell: ({ row }) => {
          const roleRows = row.original.roles;
          if (roleRows.length === 0) {
            return <span className="text-xs text-text-muted">â€”</span>;
          }
          return (
            <div className="flex max-w-[14rem] flex-wrap gap-1">
              {roleRows.map((r) => (
                <Badge key={r.assignment_id} variant="outline" className="text-[10px] font-normal">
                  {r.role_name}
                </Badge>
              ))}
            </div>
          );
        },
      },
      {
        accessorKey: "last_seen_at",
        header: t("users.columns.lastSeen", "DerniÃ¨re connexion"),
        cell: ({ row }) => (
          <span className="text-xs text-text-secondary">{row.original.last_seen_at ?? "â€”"}</span>
        ),
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) => (
          <div className="flex items-center gap-1">
            {can(P.ADM_USERS) &&
              row.original.locked_until != null &&
              row.original.locked_until > new Date().toISOString() && (
                <Button
                  variant="ghost"
                  size="sm"
                  title={t("users.unlock", "DÃ©verrouiller")}
                  onClick={(e) => {
                    e.stopPropagation();
                    void handleUnlock(row.original.id);
                  }}
                >
                  <LockOpen className="h-3.5 w-3.5 text-amber-600" />
                </Button>
              )}
            {can(P.ADM_USERS) && (
              <Button
                variant="ghost"
                size="sm"
                title={t("common.edit", "Edit")}
                onClick={(e) => {
                  e.stopPropagation();
                  setEditTarget(row.original);
                }}
              >
                <Pencil className="h-3.5 w-3.5 text-primary" />
              </Button>
            )}
            {can(P.ADM_USERS) && (
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
            {can(P.ADM_USERS) && row.original.is_active && (
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
    [t, can, handleDeactivate, handleUnlock],
  );

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-start gap-3">
          <div className="min-w-0 flex-1">
            <SmartFilterBar
              searchPlaceholder={t("users.search", "Rechercher un utilisateurâ€¦")}
              searchValue={searchInput}
              onSearchInputChange={setSearchInput}
              onSearchChange={setSearch}
              filters={
                [
                  {
                    id: "active",
                    kind: "select",
                    label: t("users.filter.statusLabel", "Statut"),
                    options: [
                      { value: "active", label: t("users.filter.active", "Actifs") },
                      { value: "inactive", label: t("users.filter.inactive", "Inactifs") },
                    ],
                    value: activeFilter === "all" ? null : activeFilter,
                    onChange: (v) => setActiveFilter(v ?? "all"),
                    allLabel: t("users.filter.all", "Tous"),
                  },
                  {
                    id: "mode",
                    kind: "select",
                    label: t("users.filter.modeLabel", "Mode"),
                    options: [
                      { value: "local", label: t("users.filter.local", "Local") },
                      { value: "sso", label: t("users.filter.sso", "SSO") },
                      { value: "hybrid", label: t("users.filter.hybrid", "Hybride") },
                    ],
                    value: modeFilter === "all" ? null : modeFilter,
                    onChange: (v) => setModeFilter(v ?? "all"),
                    allLabel: t("users.filter.allModes", "Tous modes"),
                  },
                ] satisfies SmartFilterDef[]
              }
              resultCount={users.length}
              onReset={() => {
                setSearchInput("");
                setSearch("");
                setActiveFilter("all");
                setModeFilter("all");
              }}
              className="border-0 px-0 py-0"
            />
          </div>
          {can(P.ADM_USERS) && (
            <Button size="sm" className="shrink-0 mt-0.5" onClick={() => setShowCreate(true)}>
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
        withStepUp={withStepUp}
      />

      <EditUserDialog
        user={editTarget}
        open={editTarget !== null}
        onClose={() => setEditTarget(null)}
        onSaved={() => void fetchUsers()}
      />

      {/* Create user dialog */}
      <CreateUserDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={() => void fetchUsers()}
      />

      {/* Step-up re-authentication dialog (for unlock) */}
      {StepUpDialogElement}
    </div>
  );
}
