import { AlertTriangle, Lock, Plus, Shield, Trash2, Wand2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { mfChip, mfPermissionDomainChip } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useToast } from "@/hooks/use-toast";
import {
  listRoles,
  getRole,
  createRole,
  updateRole,
  deleteRole,
  listRoleTemplates,
  simulateAccess,
  getMyPermissions,
  validateRolePermissions,
} from "@/services/rbac-service";
import type { RoleValidationResult } from "@shared/ipc-types";
import type {
  RoleWithPermissions,
  RoleDetail,
  RoleTemplate,
  SimulateAccessInput,
  SimulateAccessResult,
  CreateRoleInput,
  UpdateRoleInput,
} from "@shared/ipc-types";

// ── Helpers ─────────────────────────────────────────────────────────────────

/** Group permission names by domain prefix (e.g. "adm", "di", "eq"). */
function groupByDomain(permissions: string[]): Record<string, string[]> {
  const groups: Record<string, string[]> = {};
  for (const p of [...permissions].sort()) {
    const domain = p.split(".")[0] ?? "_";
    (groups[domain] ??= []).push(p);
  }
  return groups;
}

/** Compact domain chips for a role's permission set. */
function DomainChips({
  permissions,
  totalPermissions,
}: {
  permissions: string[];
  totalPermissions: number;
}) {
  const { t } = useTranslation("admin");

  // Full access shortcut
  if (totalPermissions > 0 && permissions.length >= totalPermissions) {
    return (
      <span
        className={`inline-flex items-center rounded-full px-2 py-0.5 text-[9px] ${mfChip.fullAccess}`}
      >
        {t("roles.fullAccess", "Accès complet")}
      </span>
    );
  }

  if (permissions.length === 0) {
    return null;
  }

  const grouped = groupByDomain(permissions);
  const entries = Object.entries(grouped).sort((a, b) => b[1].length - a[1].length);
  const MAX_CHIPS = 5;
  const visible = entries.slice(0, MAX_CHIPS);
  const overflow = entries.length - MAX_CHIPS;

  return (
    <div className="flex flex-wrap gap-0.5">
      {visible.map(([domain, perms]) => (
        <span
          key={domain}
          className={`inline-flex items-center rounded-full px-1.5 py-0 text-[9px] font-semibold leading-4 ${mfPermissionDomainChip[domain] ?? mfChip.neutral}`}
        >
          {domain.toUpperCase()} ({perms.length})
        </span>
      ))}
      {overflow > 0 && (
        <span
          className={`inline-flex items-center rounded-full px-1.5 py-0 text-[9px] font-semibold leading-4 ${mfChip.neutral}`}
          title={entries
            .slice(MAX_CHIPS)
            .map(([d, p]) => `${d.toUpperCase()} (${p.length})`)
            .join(", ")}
        >
          +{overflow}
        </span>
      )}
    </div>
  );
}

// ── Permission tree ─────────────────────────────────────────────────────────

function PermissionTree({
  allPermissions,
  activePermissions,
  isSystem,
  dependencyWarnings,
  onToggle,
}: {
  allPermissions: string[];
  activePermissions: Set<string>;
  isSystem: boolean;
  dependencyWarnings: string[];
  onToggle: (permission: string, enabled: boolean) => void;
}) {
  const { t } = useTranslation("admin");
  const grouped = useMemo(() => groupByDomain(allPermissions), [allPermissions]);
  const warningSet = useMemo(() => new Set(dependencyWarnings), [dependencyWarnings]);

  return (
    <div className="space-y-4">
      {Object.entries(grouped).map(([domain, perms]) => (
        <div key={domain} className="space-y-1.5">
          <h4 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            <Shield className="h-3.5 w-3.5" />
            {domain}
          </h4>
          <div className="space-y-1 pl-5">
            {perms.map((perm) => {
              const enabled = activePermissions.has(perm);
              const hasWarning = warningSet.has(perm);
              return (
                <div
                  key={perm}
                  className="flex items-center justify-between rounded-md px-2 py-1 hover:bg-surface-2"
                >
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-xs">{perm}</span>
                    {hasWarning && <AlertTriangle className="h-3.5 w-3.5 text-orange-500" />}
                  </div>
                  {isSystem ? (
                    <Lock className="h-3.5 w-3.5 text-text-secondary" />
                  ) : (
                    <Switch
                      checked={enabled}
                      onCheckedChange={(checked) => onToggle(perm, checked)}
                      aria-label={t("roles.togglePermission", { perm })}
                    />
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Create-from-template dialog ─────────────────────────────────────────────

function CreateRoleDialog({
  open,
  onClose,
  templates,
  onCreated,
  withStepUp,
}: {
  open: boolean;
  onClose: () => void;
  templates: RoleTemplate[];
  onCreated: () => void;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
}) {
  const { t } = useTranslation("admin");
  const { toast } = useToast();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [templateId, setTemplateId] = useState<string>("");
  const [submitting, setSubmitting] = useState(false);

  const selectedTemplate = templates.find((t) => String(t.id) === templateId);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setSubmitting(true);
    try {
      let permissionNames: string[] = [];
      if (selectedTemplate) {
        try {
          permissionNames = JSON.parse(selectedTemplate.module_set_json) as string[];
        } catch {
          permissionNames = [];
        }
      }
      const input: CreateRoleInput = {
        name: name.trim(),
        description: description.trim() || null,
        permission_names: permissionNames,
      };
      await withStepUp(() => createRole(input));
      toast({ title: t("roles.created", "Rôle créé"), variant: "success" });
      onCreated();
      onClose();
      setName("");
      setDescription("");
      setTemplateId("");
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      if (!msg.includes("cancelled")) {
        toast({
          title: msg || t("roles.errors.createFailed", "Erreur de création"),
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
          <DialogTitle>{t("roles.create.title", "Créer un rôle")}</DialogTitle>
          <DialogDescription>
            {t(
              "roles.create.description",
              "Définissez un nom et sélectionnez éventuellement un modèle.",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("roles.fields.name", "Nom")}</label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Technicien terrain"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("roles.fields.description", "Description")}
            </label>
            <Input value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
          {templates.length > 0 && (
            <div className="space-y-1.5">
              <label className="text-sm font-medium">{t("roles.create.template", "Modèle")}</label>
              <Select
                value={templateId || "__none__"}
                onValueChange={(v) => setTemplateId(v === "__none__" ? "" : v)}
              >
                <SelectTrigger>
                  <SelectValue placeholder={t("roles.create.noTemplate", "Sans modèle")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">
                    {t("roles.create.noTemplate", "Sans modèle")}
                  </SelectItem>
                  {templates.map((tmpl) => (
                    <SelectItem key={tmpl.id} value={String(tmpl.id)}>
                      {tmpl.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel", "Annuler")}
          </Button>
          <Button onClick={handleCreate} disabled={!name.trim() || submitting}>
            {t("roles.create.confirm", "Créer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Simulate-access dialog ──────────────────────────────────────────────────

function SimulateDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation("admin");
  const [userId, setUserId] = useState("");
  const [scopeType, setScopeType] = useState("global");
  const [result, setResult] = useState<SimulateAccessResult | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSimulate = async () => {
    if (!userId) return;
    setLoading(true);
    try {
      const input: SimulateAccessInput = { user_id: Number(userId), scope_type: scopeType };
      const r = await simulateAccess(input);
      setResult(r);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v) {
          onClose();
          setResult(null);
        }
      }}
    >
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("roles.simulate.title", "Simuler l'accès")}</DialogTitle>
          <DialogDescription>
            {t(
              "roles.simulate.description",
              "Visualisez les permissions effectives d'un utilisateur dans un périmètre donné.",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="flex items-end gap-3 py-2">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("roles.simulate.userId", "ID utilisateur")}
            </label>
            <Input
              value={userId}
              onChange={(e) => setUserId(e.target.value)}
              className="w-28"
              type="number"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("roles.simulate.scope", "Périmètre")}</label>
            <Select value={scopeType} onValueChange={setScopeType}>
              <SelectTrigger className="w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="global">{t("scope.global", "Global")}</SelectItem>
                <SelectItem value="site">{t("scope.site", "Site")}</SelectItem>
                <SelectItem value="department">{t("scope.department", "Département")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <Button onClick={handleSimulate} disabled={!userId || loading}>
            {t("roles.simulate.run", "Simuler")}
          </Button>
        </div>

        {result && (
          <div className="max-h-64 space-y-3 overflow-y-auto rounded-md border border-surface-border p-3 text-sm">
            {result.dependency_warnings.length > 0 && (
              <div className="space-y-1">
                <h4 className="flex items-center gap-1 font-medium text-orange-600">
                  <AlertTriangle className="h-4 w-4" />
                  {t("roles.simulate.warnings", "Avertissements de dépendance")}
                </h4>
                <ul className="list-inside list-disc text-xs text-orange-600">
                  {result.dependency_warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              </div>
            )}
            {result.blocked_by.length > 0 && (
              <div className="space-y-1">
                <h4 className="flex items-center gap-1 font-medium text-red-600">
                  <Lock className="h-4 w-4" />
                  {t("roles.simulate.blocked", "Bloqué par")}
                </h4>
                <ul className="list-inside list-disc text-xs text-red-600">
                  {result.blocked_by.map((b) => (
                    <li key={b}>{b}</li>
                  ))}
                </ul>
              </div>
            )}
            <div className="flex flex-wrap gap-1">
              {Object.entries(result.permissions).map(([perm, granted]) => (
                <Badge key={perm} variant={granted ? "default" : "outline"} className="text-[10px]">
                  {perm}
                </Badge>
              ))}
            </div>
          </div>
        )}

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => {
              onClose();
              setResult(null);
            }}
          >
            {t("common.close", "Fermer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Main panel ──────────────────────────────────────────────────────────────

export function RoleEditorPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { toast } = useToast();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  // Data
  const [roles, setRoles] = useState<RoleWithPermissions[]>([]);
  const [templates, setTemplates] = useState<RoleTemplate[]>([]);
  const [allPermissions, setAllPermissions] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  // Selected role
  const [selectedRoleId, setSelectedRoleId] = useState<number | null>(null);
  const [roleDetail, setRoleDetail] = useState<RoleDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  // Dialogs
  const [showCreate, setShowCreate] = useState(false);
  const [showSimulate, setShowSimulate] = useState(false);

  // Pending permission changes for non-system roles
  const [pendingAdd, setPendingAdd] = useState<Set<string>>(new Set());
  const [pendingRemove, setPendingRemove] = useState<Set<string>>(new Set());

  // Real-time dependency validation (SP06-F02)
  const [validationResult, setValidationResult] = useState<RoleValidationResult | null>(null);
  const [validating, setValidating] = useState(false);

  const fetchRoles = useCallback(async () => {
    setLoading(true);
    try {
      const [r, tmpl, perms] = await Promise.all([
        listRoles(),
        listRoleTemplates(),
        getMyPermissions(),
      ]);
      setRoles(r);
      setTemplates(tmpl);
      // Collect all unique permission names from all roles as the "universe"
      const allPerms = new Set<string>();
      for (const role of r) for (const p of role.permissions) allPerms.add(p);
      for (const p of perms) allPerms.add(p.name);
      setAllPermissions([...allPerms].sort());
    } catch {
      toast({
        title: t("roles.errors.loadFailed", "Erreur de chargement des rôles"),
        variant: "destructive",
      });
    } finally {
      setLoading(false);
    }
  }, [toast, t]);

  useEffect(() => {
    void fetchRoles();
  }, [fetchRoles]);

  // Load detail when selecting a role
  useEffect(() => {
    if (selectedRoleId === null) {
      setRoleDetail(null);
      return;
    }
    setDetailLoading(true);
    setPendingAdd(new Set());
    setPendingRemove(new Set());
    getRole(selectedRoleId)
      .then(setRoleDetail)
      .catch((err) => {
        console.error("getRole failed:", err);
        toast({
          title: t("roles.errors.loadDetailFailed", "Impossible de charger le rôle"),
          description: err instanceof Error ? err.message : String(err),
          variant: "destructive",
        });
        setRoleDetail(null);
      })
      .finally(() => setDetailLoading(false));
  }, [selectedRoleId, t, toast]);

  // Computed active permissions (original + pending)
  const activePermissions = useMemo(() => {
    const base = new Set(roleDetail?.role.permissions ?? []);
    for (const p of pendingAdd) base.add(p);
    for (const p of pendingRemove) base.delete(p);
    return base;
  }, [roleDetail, pendingAdd, pendingRemove]);

  const hasPendingChanges = pendingAdd.size > 0 || pendingRemove.size > 0;
  const hasHardDepErrors =
    validationResult != null && validationResult.missing_hard_deps.length > 0;

  // Run validation whenever active permissions change
  useEffect(() => {
    if (!roleDetail || roleDetail.role.is_system) {
      setValidationResult(null);
      return;
    }
    const permArray = [...activePermissions];
    if (permArray.length === 0) {
      setValidationResult(null);
      return;
    }
    let cancelled = false;
    setValidating(true);
    validateRolePermissions(permArray)
      .then((result) => {
        if (!cancelled) setValidationResult(result);
      })
      .catch(() => {
        if (!cancelled) setValidationResult(null);
      })
      .finally(() => {
        if (!cancelled) setValidating(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activePermissions, roleDetail]);

  const handleToggle = (perm: string, enabled: boolean) => {
    const original = new Set(roleDetail?.role.permissions ?? []);
    if (enabled) {
      if (original.has(perm)) {
        setPendingRemove((prev) => {
          const n = new Set(prev);
          n.delete(perm);
          return n;
        });
      } else {
        setPendingAdd((prev) => new Set(prev).add(perm));
      }
    } else {
      if (original.has(perm)) {
        setPendingRemove((prev) => new Set(prev).add(perm));
      } else {
        setPendingAdd((prev) => {
          const n = new Set(prev);
          n.delete(perm);
          return n;
        });
      }
    }
  };

  const handleSave = async () => {
    if (!roleDetail || !hasPendingChanges) return;
    try {
      const input: UpdateRoleInput = {
        role_id: roleDetail.role.id,
        add_permissions: [...pendingAdd],
        remove_permissions: [...pendingRemove],
      };
      await withStepUp(() => updateRole(input));
      toast({ title: t("roles.saved", "Rôle mis à jour"), variant: "success" });
      setPendingAdd(new Set());
      setPendingRemove(new Set());
      // Refresh detail + list
      const [detail] = await Promise.all([getRole(roleDetail.role.id), fetchRoles()]);
      setRoleDetail(detail);
    } catch {
      toast({
        title: t("roles.errors.saveFailed", "Erreur de sauvegarde"),
        variant: "destructive",
      });
    }
  };

  const handleDelete = async (roleId: number) => {
    try {
      await withStepUp(() => deleteRole(roleId));
      toast({ title: t("roles.deleted", "Rôle supprimé"), variant: "success" });
      setSelectedRoleId(null);
      void fetchRoles();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      toast({
        title: t("roles.errors.deleteFailed", "Impossible de supprimer ce rôle"),
        description: msg,
        variant: "destructive",
      });
    }
  };

  return (
    <div className="flex gap-4">
      {/* Left: Role list */}
      <div className="w-72 shrink-0 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-text-primary">{t("roles.list", "Rôles")}</h3>
          <div className="flex gap-1">
            {can("adm.roles") && (
              <Button variant="ghost" size="sm" onClick={() => setShowCreate(true)}>
                <Plus className="h-4 w-4" />
              </Button>
            )}
            {can("adm.roles") && (
              <Button variant="ghost" size="sm" onClick={() => setShowSimulate(true)}>
                <Wand2 className="h-4 w-4" />
              </Button>
            )}
          </div>
        </div>

        {loading && (
          <p className="text-xs text-text-secondary">{t("common.loading", "Chargement…")}</p>
        )}

        <div className="space-y-1">
          {roles.map((role) => (
            <button
              key={role.id}
              type="button"
              onClick={() => setSelectedRoleId(role.id)}
              className={`flex w-full flex-col gap-1 rounded-md px-3 py-2 text-left text-sm transition-colors ${
                selectedRoleId === role.id
                  ? "bg-primary/10 text-primary"
                  : "text-text-primary hover:bg-surface-2"
              }`}
            >
              <div className="flex w-full items-center gap-2">
                {role.is_system ? (
                  <Lock className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
                ) : (
                  <Shield className="h-3.5 w-3.5 shrink-0 text-primary" />
                )}
                <span className="truncate">{role.name}</span>
                <Badge variant="secondary" className="ml-auto text-[10px]">
                  {role.permissions.length}
                </Badge>
              </div>
              <div className="pl-5">
                <DomainChips
                  permissions={role.permissions}
                  totalPermissions={allPermissions.length}
                />
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* Right: Permission editor */}
      <div className="min-w-0 flex-1 rounded-lg border border-surface-border bg-surface-1 p-4">
        {!roleDetail && !detailLoading && (
          <p className="text-sm text-text-secondary">
            {t("roles.selectRole", "Sélectionnez un rôle pour voir et modifier ses permissions.")}
          </p>
        )}

        {detailLoading && (
          <p className="text-sm text-text-secondary">{t("common.loading", "Chargement…")}</p>
        )}

        {roleDetail && !detailLoading && (
          <div className="space-y-4">
            {/* Header */}
            <div className="flex items-start justify-between">
              <div>
                <h3 className="text-lg font-semibold text-text-primary">{roleDetail.role.name}</h3>
                {roleDetail.role.description && (
                  <p className="text-sm text-text-secondary">{roleDetail.role.description}</p>
                )}
                <div className="mt-1 flex items-center gap-2">
                  <Badge variant={roleDetail.role.is_system ? "outline" : "secondary"}>
                    {roleDetail.role.is_system
                      ? t("roles.system", "Système")
                      : t("roles.custom", "Personnalisé")}
                  </Badge>
                  <Badge variant="outline">{roleDetail.role.status}</Badge>
                </div>
              </div>

              <div className="flex gap-2">
                {can("adm.roles") && hasPendingChanges && (
                  <Button size="sm" onClick={handleSave} disabled={hasHardDepErrors || validating}>
                    {t("roles.save", "Enregistrer")}
                  </Button>
                )}
                {can("adm.roles") && !roleDetail.role.is_system && (
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={() => handleDelete(roleDetail.role.id)}
                  >
                    <Trash2 className="mr-1.5 h-3.5 w-3.5" />
                    {t("roles.delete", "Supprimer")}
                  </Button>
                )}
              </div>
            </div>

            {/* Hard dependency errors (SP06-F02 real-time validation) */}
            {validationResult && validationResult.missing_hard_deps.length > 0 && (
              <div className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-800 dark:border-red-900 dark:bg-red-950 dark:text-red-200">
                <h4 className="flex items-center gap-1.5 font-medium">
                  <AlertTriangle className="h-4 w-4" />
                  {t(
                    "roles.hardDepErrors",
                    "Dépendances obligatoires manquantes — sauvegarde bloquée",
                  )}
                </h4>
                <ul className="mt-1 list-inside list-disc text-xs">
                  {validationResult.missing_hard_deps.map((d) => (
                    <li key={`${d.permission_name}-${d.required_permission_name}`}>
                      <strong>{d.permission_name}</strong> {t("roles.requires", "requiert")}{" "}
                      <strong>{d.required_permission_name}</strong>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {/* Warning dependencies (SP06-F02 real-time validation) */}
            {validationResult && validationResult.warn_deps.length > 0 && (
              <div className="rounded-md border border-orange-200 bg-orange-50 p-3 text-sm text-orange-800 dark:border-orange-900 dark:bg-orange-950 dark:text-orange-200">
                <h4 className="flex items-center gap-1.5 font-medium">
                  <AlertTriangle className="h-4 w-4" />
                  {t("roles.warnDeps", "Dépendances recommandées manquantes")}
                </h4>
                <ul className="mt-1 list-inside list-disc text-xs">
                  {validationResult.warn_deps.map((d) => (
                    <li key={`${d.permission_name}-${d.required_permission_name}`}>
                      <strong>{d.permission_name}</strong> {t("roles.suggests", "suggère")}{" "}
                      <strong>{d.required_permission_name}</strong>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {/* Legacy backend dependency warnings (from role load) */}
            {roleDetail.dependency_warnings.length > 0 && (
              <div className="rounded-md border border-orange-200 bg-orange-50 p-3 text-sm text-orange-800 dark:border-orange-900 dark:bg-orange-950 dark:text-orange-200">
                <h4 className="flex items-center gap-1.5 font-medium">
                  <AlertTriangle className="h-4 w-4" />
                  {t("roles.dependencyWarnings", "Avertissements de dépendance")}
                </h4>
                <ul className="mt-1 list-inside list-disc text-xs">
                  {roleDetail.dependency_warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              </div>
            )}

            {/* Permission tree */}
            <PermissionTree
              allPermissions={allPermissions}
              activePermissions={activePermissions}
              isSystem={roleDetail.role.is_system}
              dependencyWarnings={roleDetail.dependency_warnings}
              onToggle={handleToggle}
            />
          </div>
        )}
      </div>

      {/* Dialogs */}
      <CreateRoleDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        templates={templates}
        onCreated={() => void fetchRoles()}
        withStepUp={withStepUp}
      />
      <SimulateDialog open={showSimulate} onClose={() => setShowSimulate(false)} />
      {StepUpDialogElement}
    </div>
  );
}
