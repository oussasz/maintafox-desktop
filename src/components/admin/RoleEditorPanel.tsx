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

import {
  type DependencyLink,
  hardRequirements,
  planToggle,
} from "@/components/admin/role-permission-rules";
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
import { UnsavedChangesGuard } from "@/components/ui/unsaved-changes-guard";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { permissionDomain, usePermissionText } from "@/lib/permission-labels";
import {
  formerRoleMatches,
  isShippedRole,
  useRoleText,
  withoutFormerRoles,
} from "@/lib/role-labels";
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

/** Module of a permission code (`ot.view` → `ot`; goods-receipt validation → stores). */
const moduleOf = permissionDomain;

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
  const roleText = useRoleText();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [templateId, setTemplateId] = useState("__none__");
  /** Role models by their shown name; the rights a model gives come from the registry. */
  const sortedTemplates = useMemo(
    () =>
      [...templates].sort((a, b) =>
        roleText.templateLabel(a.name).localeCompare(roleText.templateLabel(b.name)),
      ),
    [templates, roleText],
  );
  const chosen = templates.find((tmpl) => String(tmpl.id) === templateId) ?? null;
  const chosenRights = chosen ? (ROLE_TEMPLATES[chosen.name] ?? templatePermissions(chosen)) : null;
  const chooseTemplate = (id: string) => {
    const previous = templates.find((tmpl) => String(tmpl.id) === templateId) ?? null;
    const next = templates.find((tmpl) => String(tmpl.id) === id) ?? null;
    setTemplateId(id);
    // The model's name is a starting point for the role's name, never over a name typed in.
    if (
      next &&
      (name.trim() === "" || (previous && name === roleText.templateLabel(previous.name)))
    ) {
      setName(roleText.templateLabel(next.name));
    }
  };
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
    const template = chosen;
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
          {templates.length > 0 ? (
            <div className="space-y-1.5">
              <Label htmlFor="new-role-template">{t("roles.create.template")}</Label>
              <Select value={templateId} onValueChange={chooseTemplate}>
                <SelectTrigger id="new-role-template">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("roles.create.noTemplate")}</SelectItem>
                  {sortedTemplates.map((tmpl) => (
                    <SelectItem key={tmpl.id} value={String(tmpl.id)}>
                      {roleText.templateLabel(tmpl.name)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {chosen ? (
                <div className="space-y-1 rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-xs text-text-secondary">
                  <p>{roleText.templateDescription(chosen.name, chosen.description)}</p>
                  {chosenRights ? (
                    <p>{t("roles.create.templateRights", { count: chosenRights.length })}</p>
                  ) : null}
                </div>
              ) : null}
            </div>
          ) : null}
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
  matches,
}: {
  source: RoleWithPermissions | null;
  onClose: () => void;
  onCloned: (id: number) => void;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
  /** How each former role's rights compare with the role it stands for. */
  matches: ReadonlyMap<string, "same" | "different">;
}) {
  const { t } = useTranslation("admin");
  const roleText = useRoleText();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!source) return;
    // A copy of a shipped role starts from its shown name and description, never the internal key.
    const match = matches.get(source.name);
    setName(t("console.roleEditor.copyName", { name: roleText.label(source.name, match) }));
    setDescription(roleText.description(source.name, source.description, match) ?? "");
  }, [source, t, roleText, matches]);

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
            {t("console.roleEditor.cloneTitle", {
              name: source ? roleText.label(source.name, matches.get(source.name)) : "",
            })}
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
  catalog,
  onClose,
}: {
  base: RoleWithPermissions | null;
  roles: RoleWithPermissions[];
  /** Only the permissions the role editor shows are compared. */
  catalog: RoleEditorCatalog | null;
  onClose: () => void;
}) {
  const { t } = useTranslation("admin");
  const moduleLabel = useModuleLabel();
  const text = usePermissionText();
  const roleText = useRoleText();
  const matches = useMemo(() => formerRoleMatches(roles), [roles]);
  const roleName = (name: string) => roleText.label(name, matches.get(name));
  const shown = useMemo(
    () => new Map((catalog?.permissions ?? []).map((p) => [p.name, p.description] as const)),
    [catalog],
  );
  const labelOf = (p: string) => text.label(p, shown.get(p));
  const [otherId, setOtherId] = useState("");

  useEffect(() => {
    setOtherId("");
  }, [base]);

  const other = roles.find((r) => String(r.id) === otherId) ?? null;
  const rows = useMemo(() => {
    if (!base || !other) return [];
    const a = new Set(base.permissions.filter((p) => shown.has(p)));
    const b = new Set(other.permissions.filter((p) => shown.has(p)));
    const modules = new Set([...a, ...b].map(moduleOf));
    return [...modules].sort().map((m) => {
      const onlyA = [...a].filter((p) => moduleOf(p) === m && !b.has(p)).sort();
      const onlyB = [...b].filter((p) => moduleOf(p) === m && !a.has(p)).sort();
      const both = [...a].filter((p) => moduleOf(p) === m && b.has(p)).length;
      return { module: m, onlyA, onlyB, both };
    });
  }, [base, other, shown]);

  return (
    <Dialog open={base !== null} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>
            {t("console.roleEditor.compareTitle", { name: base ? roleName(base.name) : "" })}
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
                    {roleName(r.name)}
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
                  {t("console.roleEditor.onlyIn", { name: roleName(base.name) })}
                </th>
                <th className="py-1.5 pr-2 font-medium">{t("console.roleEditor.common")}</th>
                <th className="py-1.5 font-medium">
                  {t("console.roleEditor.onlyIn", { name: roleName(other.name) })}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-surface-border">
              {rows.map((r) => (
                <tr key={r.module} className="align-top">
                  <td className="py-1.5 pr-2 font-medium text-text-primary">
                    {moduleLabel(r.module)}
                  </td>
                  <td className="py-1.5 pr-2">{r.onlyA.map(labelOf).join(", ") || "—"}</td>
                  <td className="py-1.5 pr-2">{r.both}</td>
                  <td className="py-1.5">{r.onlyB.map(labelOf).join(", ") || "—"}</td>
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

/** A right the registry flags as dangerous or as needing a step-up: never ticked by one click. */
function isSensitive(p: Pick<PermissionWithSystem, "is_dangerous" | "requires_step_up">) {
  return p.is_dangerous || p.requires_step_up;
}

function PermissionMatrix({
  catalog,
  active,
  readOnly,
  onToggle,
  filter,
  autoAdded,
  onSensitiveGroup,
}: {
  catalog: RoleEditorCatalog;
  active: Set<string>;
  readOnly: boolean;
  /** View right ticked automatically → the rights that needed it (L5-06). */
  autoAdded: ReadonlyMap<string, readonly string[]>;
  /** Ticks or unticks the given permissions (one, or a whole group). */
  onToggle: (permissions: string[], enabled: boolean) => void;
  /**
   * The whole-group switch left these sensitive rights unticked (flagged dangerous or needing a
   * step-up in the registry): the panel asks before ticking them (SF-2).
   */
  onSensitiveGroup: (group: string, permissions: string[]) => void;
  filter: string;
}) {
  const { t } = useTranslation("admin");
  const moduleLabel = useModuleLabel();
  const text = usePermissionText();
  const descriptions = useMemo(
    () => new Map(catalog.permissions.map((p) => [p.name, p.description] as const)),
    [catalog.permissions],
  );
  const labelOf = useCallback(
    (name: string) => text.label(name, descriptions.get(name)),
    [text, descriptions],
  );

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
        !labelOf(p.name).toLowerCase().includes(q) &&
        !text.description(p.name, p.description).toLowerCase().includes(q)
      ) {
        continue;
      }
      const m = moduleOf(p.name);
      map.set(m, [...(map.get(m) ?? []), p]);
    }
    return [...map.entries()].sort(([a], [b]) => moduleLabel(a).localeCompare(moduleLabel(b)));
  }, [catalog.permissions, filter, moduleLabel, labelOf, text]);

  if (groups.length === 0) {
    return <p className="text-sm text-text-secondary">{t("console.roleEditor.noMatch")}</p>;
  }

  return (
    <div className="space-y-3">
      {groups.map(([module, perms]) => {
        const granted = perms.filter((p) => active.has(p.name)).length;
        const allGranted = granted === perms.length;
        const groupLabel = moduleLabel(module);
        return (
          <details
            key={module}
            className="rounded-md border border-surface-border bg-surface-0"
            open={filter.trim() !== "" || granted > 0}
          >
            <summary className="flex cursor-pointer items-center justify-between gap-2 px-3 py-2 text-sm font-medium text-text-primary">
              <span>{groupLabel}</span>
              <span className="text-xs font-normal text-text-secondary">
                {t("console.roleEditor.grantedOf", { granted, total: perms.length })}
              </span>
            </summary>
            <ul className="divide-y divide-surface-border border-t border-surface-border">
              {readOnly ? null : (
                // The whole group at once (only the rights the filter shows); each right still goes
                // through the same rules as a single tick.
                <li className="flex items-center justify-between gap-3 bg-surface-1 px-3 py-2">
                  <span className="text-xs font-medium text-text-secondary">
                    {t("console.roleEditor.wholeGroup")}
                  </span>
                  <Switch
                    checked={allGranted}
                    onCheckedChange={(checked) => {
                      if (!checked) {
                        onToggle(
                          perms.map((p) => p.name),
                          false,
                        );
                        return;
                      }
                      // One click never ticks a right the registry flags as sensitive: the others
                      // are ticked, the sensitive ones are listed and asked for.
                      const sensitive = perms
                        .filter((p) => isSensitive(p) && !active.has(p.name))
                        .map((p) => p.name);
                      onToggle(
                        perms.filter((p) => !sensitive.includes(p.name)).map((p) => p.name),
                        true,
                      );
                      if (sensitive.length > 0) onSensitiveGroup(groupLabel, sensitive);
                    }}
                    aria-label={t("console.roleEditor.wholeGroupToggle", { group: groupLabel })}
                  />
                </li>
              )}
              {perms.map((p) => {
                const deps = requires.get(p.name) ?? [];
                const enabled = active.has(p.name);
                const missing = enabled
                  ? deps.filter((d) => !active.has(d.required_permission_name))
                  : [];
                const label = labelOf(p.name);
                const description = text.description(p.name, p.description);
                return (
                  <li key={p.name} className="flex items-start justify-between gap-3 px-3 py-2">
                    <div className="min-w-0 space-y-0.5">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="text-sm text-text-primary" title={p.name}>
                          {label}
                        </span>
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
                      {description && description !== label ? (
                        <p className="text-xs text-text-secondary">{description}</p>
                      ) : null}
                      {enabled && autoAdded.has(p.name) ? (
                        <p className="text-[11px] font-medium text-tone-info-fg">
                          {t("console.roleEditor.autoAdded", {
                            perms: (autoAdded.get(p.name) ?? []).map(labelOf).join(", "),
                          })}
                        </p>
                      ) : null}
                      {deps.length > 0 ? (
                        <p className="flex flex-wrap items-center gap-1 text-[11px] text-text-secondary">
                          <Link2 className="h-3 w-3" aria-hidden />
                          {deps.map((d) => (
                            <span
                              key={d.id}
                              title={d.required_permission_name}
                              className={cn(
                                missing.includes(d) && "font-semibold text-tone-attention-fg",
                              )}
                            >
                              {d.dependency_type === "hard"
                                ? t("console.roleEditor.requires", {
                                    perm: labelOf(d.required_permission_name),
                                  })
                                : t("console.roleEditor.suggests", {
                                    perm: labelOf(d.required_permission_name),
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
                        onCheckedChange={(checked) => onToggle([p.name], checked)}
                        aria-label={t("roles.togglePermission", { perm: label })}
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

  /**
   * What the user asked for while the open role has unsaved changes: another role, a new role or
   * a copy (each opens another role, which drops the ticks). Asked before anything is dropped.
   */
  type PendingLeave =
    | { kind: "select"; id: number }
    | { kind: "create" }
    | { kind: "clone"; source: RoleWithPermissions };
  const [pendingLeave, setPendingLeave] = useState<PendingLeave | null>(null);
  const runLeave = (action: PendingLeave) => {
    if (action.kind === "select") setSelectedId(action.id);
    else if (action.kind === "create") setShowCreate(true);
    else setCloneSource(action.source);
  };
  const requestLeave = (action: PendingLeave) => {
    if (action.kind === "select" && action.id === selectedId) return;
    if (dirty) setPendingLeave(action);
    else runLeave(action);
  };
  const requestSelect = (id: number) => requestLeave({ kind: "select", id });

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

  const hardRequires = useMemo(() => hardRequirements(catalog?.dependencies ?? []), [catalog]);
  const offered = useMemo(
    () => new Set((catalog?.permissions ?? []).map((p) => p.name)),
    [catalog],
  );
  /** View rights ticked as a prerequisite → the rights that needed them (L5-06). */
  const [autoAdded, setAutoAdded] = useState<Map<string, string[]>>(new Map());
  /** What the last tick left for the user to decide: never ticked or unticked for them. */
  const [ruleNotice, setRuleNotice] = useState<{
    notAdded: DependencyLink[];
    dependents: DependencyLink[];
  } | null>(null);

  useEffect(() => {
    // Nothing pending (another role, discarded or saved): the notes have nothing left to explain.
    if (permsChanged) return;
    setAutoAdded(new Map());
    setRuleNotice(null);
  }, [permsChanged, selected]);

  /** Sensitive rights of a group the whole-group switch left for the user to confirm (SF-2). */
  const [sensitiveAsk, setSensitiveAsk] = useState<{
    group: string;
    permissions: string[];
  } | null>(null);

  const toggle = (perms: string[], enabled: boolean) => {
    const plan = planToggle(perms, enabled, active, hardRequires, offered);
    const original = new Set(selected?.permissions ?? []);
    const add = new Set(pendingAdd);
    const remove = new Set(pendingRemove);
    for (const perm of plan.tick) {
      if (original.has(perm)) remove.delete(perm);
      else add.add(perm);
    }
    for (const perm of plan.untick) {
      if (original.has(perm)) remove.add(perm);
      else add.delete(perm);
    }
    setPendingAdd(add);
    setPendingRemove(remove);
    setAutoAdded((prev) => {
      const next = new Map(prev);
      for (const perm of plan.untick) next.delete(perm);
      for (const [perm, neededBy] of plan.autoAdded) {
        next.set(perm, [...new Set([...(next.get(perm) ?? []), ...neededBy])]);
      }
      return next;
    });
    setRuleNotice(
      plan.notAdded.length > 0 || plan.dependents.length > 0
        ? { notAdded: plan.notAdded, dependents: plan.dependents }
        : null,
    );
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
  const text = usePermissionText();
  const catalogDescriptions = useMemo(
    () => new Map((catalog?.permissions ?? []).map((p) => [p.name, p.description] as const)),
    [catalog],
  );
  /** Permissions the role holds that this version does not offer: kept on save, never shown. */
  const hiddenHeld = selected
    ? selected.permissions.filter((p) => !catalogDescriptions.has(p)).length
    : 0;
  const holderCount = selected ? (holders.get(selected.id) ?? 0) : 0;
  const roleText = useRoleText();
  /** How each former shipped role's rights compare with the role it stands for (L5-03). */
  const matches = useMemo(() => formerRoleMatches(roles), [roles]);
  const roleName = (name: string) => roleText.label(name, matches.get(name));
  /** Former shipped roles are listed apart, in "Anciens rôles", to open and check them. */
  const listedRoles = useMemo(() => withoutFormerRoles(roles), [roles]);
  const formerRoles = useMemo(
    () => roles.filter((r) => !listedRoles.includes(r)),
    [roles, listedRoles],
  );
  const selectedIsFormer = selected != null && formerRoles.includes(selected);
  const selectedLabel = selected ? roleName(selected.name) : "";
  /** A shipped role is described by the application (translated), not by its stored text. */
  const shipped = selected ? isShippedRole(selected.name) : false;

  const renderRole = (role: RoleWithPermissions) => (
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
        <span className="min-w-0 flex-1 truncate" title={roleName(role.name)}>
          {roleName(role.name)}
        </span>
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
  );

  return (
    <div className="flex flex-col gap-4 lg:flex-row">
      <aside className="w-full shrink-0 space-y-3 lg:w-72" aria-label={t("roles.list")}>
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-text-primary">{t("roles.list")}</h3>
          {canEdit ? (
            <Button variant="outline" size="sm" onClick={() => requestLeave({ kind: "create" })}>
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
        <ul className="space-y-1">{listedRoles.map(renderRole)}</ul>
        {formerRoles.length > 0 ? (
          <details
            className="rounded-md border border-surface-border"
            open={selectedIsFormer || undefined}
          >
            <summary className="cursor-pointer px-3 py-2 text-xs font-medium text-text-secondary">
              {t("console.roleEditor.formerRoles", { count: formerRoles.length })}
            </summary>
            <p className="px-3 pb-2 text-[11px] text-text-secondary">
              {t("console.roleEditor.formerRolesHint")}
            </p>
            <ul className="space-y-1 px-1 pb-1">{formerRoles.map(renderRole)}</ul>
          </details>
        ) : null}
      </aside>

      <section className="min-w-0 flex-1 rounded-lg border border-surface-border bg-surface-0 p-4">
        {!selected ? (
          <p className="text-sm text-text-secondary">{t("roles.selectRole")}</p>
        ) : catalog ? (
          <div className="space-y-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 space-y-1">
                <h3 className="text-lg font-semibold text-text-primary">{selectedLabel}</h3>
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
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => requestLeave({ kind: "clone", source: selected })}
                  >
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

            {shipped ? (
              <p className="text-sm text-text-secondary">
                {roleText.description(
                  selected.name,
                  selected.description,
                  matches.get(selected.name),
                )}
              </p>
            ) : (
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
            )}

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

            {ruleNotice ? (
              <div
                className="rounded-md border border-tone-attention-border bg-tone-attention-bg p-3 text-sm text-tone-attention-fg"
                role="status"
              >
                {ruleNotice.notAdded.length > 0 ? (
                  <>
                    <p className="flex items-center gap-1.5 font-medium">
                      <AlertTriangle className="h-4 w-4" aria-hidden />
                      {t("console.roleEditor.notAddedTitle")}
                    </p>
                    <ul className="mt-1 list-inside list-disc text-xs">
                      {ruleNotice.notAdded.map((l) => (
                        <li key={`${l.perm}-${l.required}`}>
                          {t("console.roleEditor.needs", {
                            perm: text.label(l.perm, catalogDescriptions.get(l.perm)),
                            required: text.label(l.required, catalogDescriptions.get(l.required)),
                          })}
                        </li>
                      ))}
                    </ul>
                    <p className="mt-1 text-xs">{t("console.roleEditor.notAddedHint")}</p>
                  </>
                ) : null}
                {ruleNotice.dependents.length > 0 ? (
                  <>
                    <p className="flex items-center gap-1.5 font-medium">
                      <AlertTriangle className="h-4 w-4" aria-hidden />
                      {t("console.roleEditor.dependentsTitle")}
                    </p>
                    <ul className="mt-1 list-inside list-disc text-xs">
                      {ruleNotice.dependents.map((l) => (
                        <li key={`${l.perm}-${l.required}`}>
                          {t("console.roleEditor.needs", {
                            perm: text.label(l.perm, catalogDescriptions.get(l.perm)),
                            required: text.label(l.required, catalogDescriptions.get(l.required)),
                          })}
                        </li>
                      ))}
                    </ul>
                    <p className="mt-1 text-xs">{t("console.roleEditor.dependentsHint")}</p>
                  </>
                ) : null}
                <Button
                  variant="ghost"
                  size="sm"
                  className="mt-1 h-7 px-2 text-xs"
                  onClick={() => setRuleNotice(null)}
                >
                  {t("console.roleEditor.noticeDismiss")}
                </Button>
              </div>
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
                        perm: text.label(
                          d.permission_name,
                          catalogDescriptions.get(d.permission_name),
                        ),
                        required: text.label(
                          d.required_permission_name,
                          catalogDescriptions.get(d.required_permission_name),
                        ),
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
            </div>

            {hiddenHeld > 0 ? (
              <p className="text-xs text-text-secondary">
                {t("console.roleEditor.hiddenRetained", { count: hiddenHeld })}
              </p>
            ) : null}

            <PermissionMatrix
              catalog={catalog}
              active={active}
              readOnly={selected.is_system || !canEdit}
              onToggle={toggle}
              filter={permissionFilter}
              autoAdded={autoAdded}
              onSensitiveGroup={(group, permissions) => setSensitiveAsk({ group, permissions })}
            />

            {canEdit && dirty ? (
              // Stays in view at the bottom while the long list of groups scrolls.
              <div
                role="region"
                aria-label={t("console.roleEditor.saveBar")}
                className="sticky bottom-0 z-10 -mx-4 -mb-4 flex flex-wrap items-center justify-between gap-2 rounded-b-lg border-t border-surface-border bg-surface-0 px-4 py-3 shadow-[0_-4px_12px_rgba(0,0,0,0.06)]"
              >
                <span className="text-sm text-text-secondary">
                  {t("console.roleEditor.unsavedSummary", {
                    added: pendingAdd.size,
                    removed: pendingRemove.size,
                  })}
                </span>
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
              </div>
            ) : null}
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
        matches={matches}
      />
      <CompareRolesDialog
        base={compareBase}
        roles={roles}
        catalog={catalog}
        onClose={() => setCompareBase(null)}
      />
      <ConfirmDialog
        open={retireTarget !== null}
        onOpenChange={(v) => !v && setRetireTarget(null)}
        title={t("console.roleEditor.retireTitle", {
          name: retireTarget ? roleName(retireTarget.name) : "",
        })}
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
      {/* Leaving with unsaved role changes — another Administration tab, another page, or closing
          the app — asks first (the shared guard; switching role has its own dialog above). */}
      <UnsavedChangesGuard
        when={dirty}
        title={t("console.roleEditor.unsavedTitle")}
        description={t("console.roleEditor.unsavedLeaveBody", { name: selectedLabel })}
        confirmLabel={t("console.roleEditor.discardChanges")}
      />
      <ConfirmDialog
        open={sensitiveAsk !== null}
        onOpenChange={(v) => !v && setSensitiveAsk(null)}
        title={t("console.roleEditor.sensitiveGroupTitle", { group: sensitiveAsk?.group ?? "" })}
        description={t("console.roleEditor.sensitiveGroupBody", {
          count: sensitiveAsk?.permissions.length ?? 0,
        })}
        confirmLabel={t("console.roleEditor.sensitiveGroupConfirm")}
        dismissLabel={t("console.roleEditor.sensitiveGroupKeepOut")}
        onConfirm={() => {
          if (sensitiveAsk) toggle(sensitiveAsk.permissions, true);
          setSensitiveAsk(null);
        }}
      >
        <ul className="list-inside list-disc text-sm text-text-primary">
          {(sensitiveAsk?.permissions ?? []).map((perm) => (
            <li key={perm} title={perm}>
              {text.label(perm, catalogDescriptions.get(perm))}
            </li>
          ))}
        </ul>
      </ConfirmDialog>
      <ConfirmDialog
        open={pendingLeave !== null}
        onOpenChange={(v) => !v && setPendingLeave(null)}
        title={t("console.roleEditor.unsavedTitle")}
        description={t(
          pendingLeave?.kind === "select"
            ? "console.roleEditor.unsavedBody"
            : "console.roleEditor.unsavedNewBody",
          { name: selectedLabel },
        )}
        confirmLabel={t("console.roleEditor.discardChanges")}
        destructive
        onConfirm={() => {
          if (pendingLeave) {
            // Drop the changes first, so the guard does not ask a second time.
            setPendingAdd(new Set());
            setPendingRemove(new Set());
            setDescription(selected?.description ?? "");
            runLeave(pendingLeave);
          }
          setPendingLeave(null);
        }}
      />
      {StepUpDialogElement}
    </div>
  );
}
