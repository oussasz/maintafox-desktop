import {
  AlertTriangle,
  ArrowLeftRight,
  Copy,
  KeyRound,
  Link2,
  Lock,
  Plus,
  Search,
  ShieldAlert,
  Trash2,
  Users,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
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
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { cn } from "@/lib/utils";
import {
  cloneRole,
  createRole,
  deleteRole,
  getRoleEditorCatalog,
  listRoleTemplates,
  listRoles,
  updateRole,
  validateRolePermissions,
} from "@/services/rbac-service";
import { pushAppToast } from "@/store/app-toast-store";
import type {
  PermissionDependencyRow,
  PermissionWithSystem,
  RoleEditorCatalog,
  RoleTemplate,
  RoleValidationResult,
  RoleWithPermissions,
} from "@shared/ipc-types";
import { P, ROLE_TEMPLATES } from "@shared/rbac/permissions.generated";

/** Module of a permission code (`ot.view` → `ot`). */
function moduleOf(permission: string): string {
  return permission.split(".")[0] ?? permission;
}

function useModuleLabel() {
  const { t } = useTranslation("admin");
  return (module: string): string =>
    String(t(`permissions.domainLabels.${module}`, module.toUpperCase()));
}

// ── Create (empty or from a template) ───────────────────────────────────────

/** Permissions of a stored role template; null when its stored list is unreadable. */
export function templatePermissions(
  template: Pick<RoleTemplate, "module_set_json">,
): string[] | null {
  try {
    const parsed: unknown = JSON.parse(template.module_set_json);
    if (!Array.isArray(parsed) || !parsed.every((p) => typeof p === "string")) return null;
    return parsed as string[];
  } catch {
    return null;
  }
}

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
  onCreated: (id: number) => void;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
}) {
  const { t } = useTranslation("admin");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [templateId, setTemplateId] = useState("__none__");
  const [submitting, setSubmitting] = useState(false);
  const [templateUnreadable, setTemplateUnreadable] = useState(false);

  useEffect(() => {
    if (!open) return;
    setName("");
    setDescription("");
    setTemplateId("__none__");
    setTemplateUnreadable(false);
  }, [open]);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setTemplateUnreadable(false);
    let permissionNames: string[] = [];
    const template = templates.find((tmpl) => String(tmpl.id) === templateId);
    if (template) {
      const fromGenerated = ROLE_TEMPLATES[template.name];
      const fromStored = fromGenerated ? [...fromGenerated] : templatePermissions(template);
      if (fromStored == null) {
        // Never create the role without the template's permissions.
        setTemplateUnreadable(true);
        return;
      }
      permissionNames = fromStored;
    }
    setSubmitting(true);
    try {
      const created = await withStepUp(() =>
        createRole({
          name: name.trim(),
          description: description.trim() || null,
          permission_names: permissionNames,
        }),
      );
      onCreated(created.id);
      onClose();
    } catch {
      // Reported by the IPC layer; a cancelled step-up keeps the dialog open.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("roles.create.title")}</DialogTitle>
          <DialogDescription>{t("roles.create.description")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <Label htmlFor="new-role-name">{t("roles.fields.name")}</Label>
            <Input
              id="new-role-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("roles.create.namePlaceholder")}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="new-role-description">{t("roles.fields.description")}</Label>
            <Input
              id="new-role-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>
          {templates.length > 0 ? (
            <div className="space-y-1.5">
              <Label htmlFor="new-role-template">{t("roles.create.template")}</Label>
              <Select value={templateId} onValueChange={setTemplateId}>
                <SelectTrigger id="new-role-template">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("roles.create.noTemplate")}</SelectItem>
                  {templates.map((tmpl) => (
                    <SelectItem key={tmpl.id} value={String(tmpl.id)}>
                      {tmpl.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          ) : null}
          {templateUnreadable ? (
            <p
              role="alert"
              className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
            >
              {t("console.roleEditor.templateUnreadable")}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button onClick={() => void handleCreate()} disabled={!name.trim() || submitting}>
            {t("roles.create.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Clone ───────────────────────────────────────────────────────────────────

function CloneRoleDialog({
  source,
  onClose,
  onCloned,
  withStepUp,
}: {
  source: RoleWithPermissions | null;
  onClose: () => void;
  onCloned: (id: number) => void;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
}) {
  const { t } = useTranslation("admin");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!source) return;
    setName(t("console.roleEditor.copyName", { name: source.name }));
    setDescription(source.description ?? "");
  }, [source, t]);

  const submit = async () => {
    if (!source || !name.trim()) return;
    setSubmitting(true);
    try {
      const created = await withStepUp(() =>
        cloneRole({
          source_role_id: source.id,
          name: name.trim(),
          description: description.trim() || null,
        }),
      );
      onCloned(created.id);
      onClose();
    } catch {
      // Reported by the IPC layer.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={source !== null} onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t("console.roleEditor.cloneTitle", { name: source?.name ?? "" })}
          </DialogTitle>
          <DialogDescription>
            {t("console.roleEditor.cloneBody", { count: source?.permissions.length ?? 0 })}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <Label htmlFor="clone-role-name">{t("roles.fields.name")}</Label>
            <Input id="clone-role-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="clone-role-description">{t("roles.fields.description")}</Label>
            <Input
              id="clone-role-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={!name.trim() || submitting}>
            {t("console.roleEditor.clone")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Compare ─────────────────────────────────────────────────────────────────

function CompareRolesDialog({
  base,
  roles,
  onClose,
}: {
  base: RoleWithPermissions | null;
  roles: RoleWithPermissions[];
  onClose: () => void;
}) {
  const { t } = useTranslation("admin");
  const moduleLabel = useModuleLabel();
  const [otherId, setOtherId] = useState("");

  useEffect(() => {
    setOtherId("");
  }, [base]);

  const other = roles.find((r) => String(r.id) === otherId) ?? null;
  const rows = useMemo(() => {
    if (!base || !other) return [];
    const a = new Set(base.permissions);
    const b = new Set(other.permissions);
    const modules = new Set([...a, ...b].map(moduleOf));
    return [...modules].sort().map((m) => {
      const onlyA = [...a].filter((p) => moduleOf(p) === m && !b.has(p)).sort();
      const onlyB = [...b].filter((p) => moduleOf(p) === m && !a.has(p)).sort();
      const both = [...a].filter((p) => moduleOf(p) === m && b.has(p)).length;
      return { module: m, onlyA, onlyB, both };
    });
  }, [base, other]);

  return (
    <Dialog open={base !== null} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>
            {t("console.roleEditor.compareTitle", { name: base?.name ?? "" })}
          </DialogTitle>
          <DialogDescription>{t("console.roleEditor.compareBody")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-1.5">
          <Label htmlFor="compare-with">{t("console.roleEditor.compareWith")}</Label>
          <Select value={otherId} onValueChange={setOtherId}>
            <SelectTrigger id="compare-with">
              <SelectValue placeholder={t("console.roleEditor.chooseRole")} />
            </SelectTrigger>
            <SelectContent>
              {roles
                .filter((r) => r.id !== base?.id)
                .map((r) => (
                  <SelectItem key={r.id} value={String(r.id)}>
                    {r.name}
                  </SelectItem>
                ))}
            </SelectContent>
          </Select>
        </div>
        {other && base ? (
          <table className="mt-3 w-full text-left text-xs">
            <thead className="border-b border-surface-border text-text-secondary">
              <tr>
                <th className="py-1.5 pr-2 font-medium">{t("console.roleEditor.module")}</th>
                <th className="py-1.5 pr-2 font-medium">
                  {t("console.roleEditor.onlyIn", { name: base.name })}
                </th>
                <th className="py-1.5 pr-2 font-medium">{t("console.roleEditor.common")}</th>
                <th className="py-1.5 font-medium">
                  {t("console.roleEditor.onlyIn", { name: other.name })}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-surface-border">
              {rows.map((r) => (
                <tr key={r.module} className="align-top">
                  <td className="py-1.5 pr-2 font-medium text-text-primary">
                    {moduleLabel(r.module)}
                  </td>
                  <td className="py-1.5 pr-2 font-mono">{r.onlyA.join(", ") || "—"}</td>
                  <td className="py-1.5 pr-2">{r.both}</td>
                  <td className="py-1.5 font-mono">{r.onlyB.join(", ") || "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : null}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Permission matrix of one role, grouped by module ────────────────────────

function PermissionMatrix({
  catalog,
  active,
  readOnly,
  onToggle,
  filter,
}: {
  catalog: RoleEditorCatalog;
  active: Set<string>;
  readOnly: boolean;
  onToggle: (permission: string, enabled: boolean) => void;
  filter: string;
}) {
  const { t } = useTranslation("admin");
  const moduleLabel = useModuleLabel();

  const requires = useMemo(() => {
    const map = new Map<string, PermissionDependencyRow[]>();
    for (const d of catalog.dependencies) {
      map.set(d.permission_name, [...(map.get(d.permission_name) ?? []), d]);
    }
    return map;
  }, [catalog.dependencies]);

  const groups = useMemo(() => {
    const q = filter.trim().toLowerCase();
    const map = new Map<string, PermissionWithSystem[]>();
    for (const p of catalog.permissions) {
      if (
        q &&
        !p.name.toLowerCase().includes(q) &&
        !(p.description ?? "").toLowerCase().includes(q)
      ) {
        continue;
      }
      const m = moduleOf(p.name);
      map.set(m, [...(map.get(m) ?? []), p]);
    }
    return [...map.entries()].sort(([a], [b]) => moduleLabel(a).localeCompare(moduleLabel(b)));
  }, [catalog.permissions, filter, moduleLabel]);

  if (groups.length === 0) {
    return <p className="text-sm text-text-secondary">{t("console.roleEditor.noMatch")}</p>;
  }

  return (
    <div className="space-y-3">
      {groups.map(([module, perms]) => {
        const granted = perms.filter((p) => active.has(p.name)).length;
        return (
          <details
            key={module}
            className="rounded-md border border-surface-border bg-surface-0"
            open={filter.trim() !== "" || granted > 0}
          >
            <summary className="flex cursor-pointer items-center justify-between gap-2 px-3 py-2 text-sm font-medium text-text-primary">
              <span>{moduleLabel(module)}</span>
              <span className="text-xs font-normal text-text-secondary">
                {t("console.roleEditor.grantedOf", { granted, total: perms.length })}
              </span>
            </summary>
            <ul className="divide-y divide-surface-border border-t border-surface-border">
              {perms.map((p) => {
                const deps = requires.get(p.name) ?? [];
                const enabled = active.has(p.name);
                const missing = enabled
                  ? deps.filter((d) => !active.has(d.required_permission_name))
                  : [];
                return (
                  <li key={p.name} className="flex items-start justify-between gap-3 px-3 py-2">
                    <div className="min-w-0 space-y-0.5">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="font-mono text-xs text-text-primary">{p.name}</span>
                        {p.is_dangerous ? (
                          <StatusChip
                            label={t("console.roleEditor.sensitive")}
                            tone="attention"
                            icon={ShieldAlert}
                          />
                        ) : null}
                        {p.requires_step_up ? (
                          <StatusChip
                            label={t("console.roleEditor.stepUp")}
                            tone="info"
                            icon={KeyRound}
                          />
                        ) : null}
                      </div>
                      {p.description ? (
                        <p className="text-xs text-text-secondary">{p.description}</p>
                      ) : null}
                      {deps.length > 0 ? (
                        <p className="flex flex-wrap items-center gap-1 text-[11px] text-text-secondary">
                          <Link2 className="h-3 w-3" aria-hidden />
                          {deps.map((d) => (
                            <span
                              key={d.id}
                              className={cn(
                                "font-mono",
                                missing.includes(d) && "font-semibold text-tone-attention-fg",
                              )}
                            >
                              {d.dependency_type === "hard"
                                ? t("console.roleEditor.requires", {
                                    perm: d.required_permission_name,
                                  })
                                : t("console.roleEditor.suggests", {
                                    perm: d.required_permission_name,
                                  })}
                            </span>
                          ))}
                        </p>
                      ) : null}
                    </div>
                    {readOnly ? (
                      enabled ? (
                        <StatusChip label={t("console.roleEditor.granted")} tone="success" />
                      ) : (
                        <span className="text-xs text-text-muted">—</span>
                      )
                    ) : (
                      <Switch
                        checked={enabled}
                        onCheckedChange={(checked) => onToggle(p.name, checked)}
                        aria-label={t("roles.togglePermission", { perm: p.name })}
                      />
                    )}
                  </li>
                );
              })}
            </ul>
          </details>
        );
      })}
    </div>
  );
}

// ── Main panel ──────────────────────────────────────────────────────────────

/**
 * Roles: permissions grouped by module with their declared dependencies, description edit,
 * clone, compare, retire. System roles are read-only (copy them to adapt). Every change needs
 * step-up and is written to the admin change ledger.
 */
export function RoleEditorPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  // The global toast host shows them (a local useToast list was never rendered here).
  const toast = pushAppToast;
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const canEdit = can(P.ADM_ROLES);

  const [roles, setRoles] = useState<RoleWithPermissions[]>([]);
  const [templates, setTemplates] = useState<RoleTemplate[]>([]);
  const [catalog, setCatalog] = useState<RoleEditorCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [pendingAdd, setPendingAdd] = useState<Set<string>>(new Set());
  const [pendingRemove, setPendingRemove] = useState<Set<string>>(new Set());
  const [description, setDescription] = useState("");
  const [validation, setValidation] = useState<RoleValidationResult | null>(null);
  const [validationError, setValidationError] = useState<unknown>(null);
  const [validationRun, setValidationRun] = useState(0);
  const [permissionFilter, setPermissionFilter] = useState("");
  const [saving, setSaving] = useState(false);

  const [showCreate, setShowCreate] = useState(false);
  const [cloneSource, setCloneSource] = useState<RoleWithPermissions | null>(null);
  const [compareBase, setCompareBase] = useState<RoleWithPermissions | null>(null);
  const [retireTarget, setRetireTarget] = useState<RoleWithPermissions | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [r, tmpl, cat] = await Promise.all([
        listRoles(),
        listRoleTemplates(),
        getRoleEditorCatalog(),
      ]);
      setRoles(r);
      setTemplates(tmpl);
      setCatalog(cat);
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const selected = roles.find((r) => r.id === selectedId) ?? null;
  const holders = useMemo(() => {
    const m = new Map<number, number>();
    for (const h of catalog?.role_holders ?? []) m.set(h.role_id, h.active_users);
    return m;
  }, [catalog]);

  useEffect(() => {
    setPendingAdd(new Set());
    setPendingRemove(new Set());
    setDescription(selected?.description ?? "");
    setValidation(null);
  }, [selected]);

  const active = useMemo(() => {
    const base = new Set(selected?.permissions ?? []);
    for (const p of pendingAdd) base.add(p);
    for (const p of pendingRemove) base.delete(p);
    return base;
  }, [selected, pendingAdd, pendingRemove]);

  const permsChanged = pendingAdd.size > 0 || pendingRemove.size > 0;
  const descriptionChanged =
    selected != null && description.trim() !== (selected.description ?? "");
  const dirty = permsChanged || descriptionChanged;

  /** Role the user clicked while the open one has unsaved changes (asks before dropping them). */
  const [pendingSwitchId, setPendingSwitchId] = useState<number | null>(null);
  const requestSelect = (id: number) => {
    if (id === selectedId) return;
    if (dirty) setPendingSwitchId(id);
    else setSelectedId(id);
  };

  // Leaving the app or reloading with unsaved role changes asks the browser's own question.
  useEffect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => {
      e.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);

  useEffect(() => {
    if (!selected || selected.is_system || !permsChanged) {
      setValidation(null);
      setValidationError(null);
      return;
    }
    let cancelled = false;
    setValidationError(null);
    validateRolePermissions([...active])
      .then((r) => {
        if (!cancelled) setValidation(r);
      })
      .catch((err: unknown) => {
        // The server still checks dependencies on save; say the preview could not run.
        if (!cancelled) {
          setValidation(null);
          setValidationError(err);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [active, selected, permsChanged, validationRun]);

  const toggle = (perm: string, enabled: boolean) => {
    const original = new Set(selected?.permissions ?? []);
    const edit = (set: Set<string>, add: boolean) => {
      const n = new Set(set);
      if (add) n.add(perm);
      else n.delete(perm);
      return n;
    };
    if (enabled) {
      if (original.has(perm)) setPendingRemove((s) => edit(s, false));
      else setPendingAdd((s) => edit(s, true));
    } else if (original.has(perm)) {
      setPendingRemove((s) => edit(s, true));
    } else {
      setPendingAdd((s) => edit(s, false));
    }
  };

  const save = async () => {
    if (!selected || !dirty) return;
    setSaving(true);
    try {
      await withStepUp(() =>
        updateRole({
          role_id: selected.id,
          ...(descriptionChanged ? { description: description.trim() } : {}),
          ...(permsChanged
            ? { add_permissions: [...pendingAdd], remove_permissions: [...pendingRemove] }
            : {}),
        }),
      );
      toast({ title: t("roles.saved"), variant: "success" });
      await load();
    } catch {
      // Reported by the IPC layer.
    } finally {
      setSaving(false);
    }
  };

  const hardMissing = validation?.missing_hard_deps ?? [];
  const holderCount = selected ? (holders.get(selected.id) ?? 0) : 0;

  return (
    <div className="flex flex-col gap-4 lg:flex-row">
      <aside className="w-full shrink-0 space-y-3 lg:w-72" aria-label={t("roles.list")}>
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-text-primary">{t("roles.list")}</h3>
          {canEdit ? (
            <Button variant="outline" size="sm" onClick={() => setShowCreate(true)}>
              <Plus className="mr-1 h-3.5 w-3.5" aria-hidden />
              {t("console.roleEditor.new")}
            </Button>
          ) : null}
        </div>
        {loading ? <p className="text-xs text-text-secondary">{t("common.loading")}</p> : null}
        {loadError != null ? (
          <ErrorState
            error={loadError}
            title={t("console.roleEditor.loadError")}
            onRetry={() => void load()}
            retrying={loading}
          />
        ) : null}
        <ul className="space-y-1">
          {roles.map((role) => (
            <li key={role.id}>
              <button
                type="button"
                onClick={() => requestSelect(role.id)}
                aria-current={selectedId === role.id ? "true" : undefined}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors",
                  selectedId === role.id
                    ? "bg-primary/10 text-primary"
                    : "text-text-primary hover:bg-surface-2",
                )}
              >
                {role.is_system ? (
                  <Lock className="h-3.5 w-3.5 shrink-0 text-text-secondary" aria-hidden />
                ) : null}
                <span className="min-w-0 flex-1 truncate">{role.name}</span>
                <span
                  className="flex items-center gap-0.5 text-[11px] text-text-secondary"
                  title={t("console.roleEditor.holders")}
                >
                  <Users className="h-3 w-3" aria-hidden />
                  {holders.get(role.id) ?? 0}
                </span>
                <span className="text-[11px] text-text-secondary">· {role.permissions.length}</span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <section className="min-w-0 flex-1 rounded-lg border border-surface-border bg-surface-0 p-4">
        {!selected ? (
          <p className="text-sm text-text-secondary">{t("roles.selectRole")}</p>
        ) : catalog ? (
          <div className="space-y-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 space-y-1">
                <h3 className="text-lg font-semibold text-text-primary">{selected.name}</h3>
                <div className="flex flex-wrap items-center gap-2">
                  <StatusChip
                    label={selected.is_system ? t("roles.system") : t("roles.custom")}
                    tone={selected.is_system ? "neutral" : "info"}
                  />
                  <span className="text-xs text-text-secondary">
                    {t("console.roleEditor.holdersCount", { count: holderCount })} ·{" "}
                    {t("console.roleEditor.permissionsCount", { count: active.size })}
                  </span>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" size="sm" onClick={() => setCompareBase(selected)}>
                  <ArrowLeftRight className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                  {t("console.roleEditor.compare")}
                </Button>
                {canEdit ? (
                  <Button variant="outline" size="sm" onClick={() => setCloneSource(selected)}>
                    <Copy className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                    {t("console.roleEditor.clone")}
                  </Button>
                ) : null}
                {canEdit && !selected.is_system ? (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={holderCount > 0}
                    title={holderCount > 0 ? t("console.roleEditor.retireBlocked") : undefined}
                    onClick={() => setRetireTarget(selected)}
                  >
                    <Trash2 className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                    {t("console.roleEditor.retire")}
                  </Button>
                ) : null}
              </div>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="role-description">{t("roles.fields.description")}</Label>
              <Textarea
                id="role-description"
                rows={2}
                value={description}
                disabled={!canEdit}
                onChange={(e) => setDescription(e.target.value)}
              />
            </div>

            {selected.is_system ? (
              <p className="rounded-md border border-surface-border bg-surface-1 p-3 text-xs text-text-secondary">
                {t("console.roleEditor.systemReadOnly")}
              </p>
            ) : null}

            {validationError != null ? (
              <ErrorState
                error={validationError}
                title={t("console.roleEditor.validationError")}
                onRetry={() => setValidationRun((n) => n + 1)}
                className="py-4"
              />
            ) : null}

            {hardMissing.length > 0 ? (
              <div
                className="rounded-md border border-tone-danger-border bg-tone-danger-bg p-3 text-sm text-tone-danger-fg"
                role="alert"
              >
                <p className="flex items-center gap-1.5 font-medium">
                  <AlertTriangle className="h-4 w-4" aria-hidden />
                  {t("roles.hardDepErrors")}
                </p>
                <ul className="mt-1 list-inside list-disc text-xs">
                  {hardMissing.map((d) => (
                    <li key={`${d.permission_name}-${d.required_permission_name}`}>
                      {t("console.roleEditor.missingDependency", {
                        perm: d.permission_name,
                        required: d.required_permission_name,
                      })}
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}

            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="relative w-full max-w-xs">
                <Search
                  className="pointer-events-none absolute left-2.5 top-2.5 h-4 w-4 text-text-muted"
                  aria-hidden
                />
                <Input
                  value={permissionFilter}
                  onChange={(e) => setPermissionFilter(e.target.value)}
                  placeholder={t("console.roleEditor.filterPermissions")}
                  aria-label={t("console.roleEditor.filterPermissions")}
                  className="pl-8"
                />
              </div>
              {canEdit && dirty ? (
                <div className="flex gap-2">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setPendingAdd(new Set());
                      setPendingRemove(new Set());
                      setDescription(selected.description ?? "");
                    }}
                  >
                    {t("console.roleEditor.discard")}
                  </Button>
                  <Button
                    size="sm"
                    onClick={() => void save()}
                    disabled={saving || hardMissing.length > 0}
                  >
                    {t("console.roleEditor.save", {
                      added: pendingAdd.size,
                      removed: pendingRemove.size,
                    })}
                  </Button>
                </div>
              ) : null}
            </div>

            <PermissionMatrix
              catalog={catalog}
              active={active}
              readOnly={selected.is_system || !canEdit}
              onToggle={toggle}
              filter={permissionFilter}
            />
          </div>
        ) : null}
      </section>

      <CreateRoleDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        templates={templates}
        onCreated={(id) => {
          void load().then(() => setSelectedId(id));
        }}
        withStepUp={withStepUp}
      />
      <CloneRoleDialog
        source={cloneSource}
        onClose={() => setCloneSource(null)}
        onCloned={(id) => {
          void load().then(() => setSelectedId(id));
        }}
        withStepUp={withStepUp}
      />
      <CompareRolesDialog base={compareBase} roles={roles} onClose={() => setCompareBase(null)} />
      <ConfirmDialog
        open={retireTarget !== null}
        onOpenChange={(v) => !v && setRetireTarget(null)}
        title={t("console.roleEditor.retireTitle", { name: retireTarget?.name ?? "" })}
        description={t("console.roleEditor.retireBody")}
        confirmLabel={t("console.roleEditor.retire")}
        destructive
        onConfirm={async () => {
          if (!retireTarget) return;
          await withStepUp(() => deleteRole(retireTarget.id));
          setSelectedId(null);
          await load();
        }}
      />
      <ConfirmDialog
        open={pendingSwitchId !== null}
        onOpenChange={(v) => !v && setPendingSwitchId(null)}
        title={t("console.roleEditor.unsavedTitle")}
        description={t("console.roleEditor.unsavedBody", { name: selected?.name ?? "" })}
        confirmLabel={t("console.roleEditor.discardChanges")}
        destructive
        onConfirm={() => {
          if (pendingSwitchId !== null) setSelectedId(pendingSwitchId);
          setPendingSwitchId(null);
        }}
      />
      {StepUpDialogElement}
    </div>
  );
}
