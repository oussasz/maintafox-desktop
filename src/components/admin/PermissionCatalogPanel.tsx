import {
  AlertTriangle,
  Download,
  Info,
  Lock,
  Plus,
  Search,
  Shield,
  ShieldAlert,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { mfChip, mfPermissionDomainChip } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  createCustomPermission,
  getPermissionDependencies,
  listPermissions,
} from "@/services/rbac-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { PermissionDependencyRow, PermissionWithSystem } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

// ── Constants ───────────────────────────────────────────────────────────────

/** All standard permission domain prefixes in display order. */
const DOMAIN_GROUPS = [
  "eq",
  "di",
  "ot",
  "org",
  "per",
  "ref",
  "inv",
  "pm",
  "ram",
  "rep",
  "arc",
  "doc",
  "plan",
  "log",
  "trn",
  "iot",
  "erp",
  "ptw",
  "fin",
  "ins",
  "cfg",
  "adm",
] as const;

/** Domains with a translated label under `admin:permissions.domainLabels.<domain>`. */
const LABELLED_DOMAINS = new Set<string>([...DOMAIN_GROUPS, "cst"]);

const SYSTEM_PREFIXES = [
  "eq.",
  "di.",
  "ot.",
  "org.",
  "per.",
  "ref.",
  "inv.",
  "pm.",
  "ram.",
  "rep.",
  "arc.",
  "doc.",
  "plan.",
  "log.",
  "trn.",
  "iot.",
  "erp.",
  "ptw.",
  "fin.",
  "ins.",
  "cfg.",
  "adm.",
  "sync.",
  "console.",
  "customer.",
  "entitlement.",
  "rollout.",
  "platform.",
  "audit.",
];

// ── Create Custom Permission Dialog ─────────────────────────────────────────

function CreatePermissionDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
}) {
  const { t } = useTranslation("admin");
  // The global toast host shows them (a local useToast list was never rendered here).
  const toast = pushAppToast;
  const [name, setName] = useState("cst.");
  const [description, setDescription] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const nameError = useMemo(() => {
    const trimmed = name.trim().toLowerCase();
    if (!trimmed.startsWith("cst.")) {
      return t("permissions.errors.mustStartWithCst");
    }
    if (trimmed.length < 5) {
      return t("permissions.errors.nameTooShort");
    }
    for (const prefix of SYSTEM_PREFIXES) {
      if (trimmed.startsWith(prefix)) {
        return t("permissions.errors.systemPrefix");
      }
    }
    return null;
  }, [name, t]);

  const handleCreate = async () => {
    if (nameError) return;
    setSubmitting(true);
    try {
      await createCustomPermission({
        name: name.trim().toLowerCase(),
        description: description.trim() || null,
        category: "custom",
      });
      toast({
        title: t("permissions.created"),
        variant: "success",
      });
      onCreated();
      onClose();
      setName("cst.");
      setDescription("");
    } catch (err) {
      // The real cause (FR/EN message for the error code), not only "creation failed".
      const described = describeAppError(err);
      toast({
        title: t("permissions.errors.createFailed"),
        description: described.description
          ? `${described.title} — ${described.description}`
          : described.title,
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
          <DialogTitle>{t("permissions.create.title")}</DialogTitle>
          <DialogDescription>{t("permissions.create.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("permissions.fields.name")}</label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="cst.my_permission"
            />
            {nameError && <p className="text-xs text-red-600">{nameError}</p>}
          </div>
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("permissions.fields.description")}</label>
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t("permissions.create.descPlaceholder")}
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button onClick={handleCreate} disabled={!!nameError || submitting}>
            {t("permissions.create.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Dependency Viewer Side Panel ────────────────────────────────────────────

function DependencyViewer({
  permissionName,
  onClose,
}: {
  permissionName: string;
  onClose: () => void;
}) {
  const { t } = useTranslation("admin");
  const [deps, setDeps] = useState<PermissionDependencyRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);

  const load = useCallback(() => {
    setLoading(true);
    setLoadError(null);
    getPermissionDependencies(permissionName)
      .then(setDeps)
      .catch((err: unknown) => {
        // Shown with a retry: never "no dependencies" when they could not be read.
        setDeps([]);
        setLoadError(err);
      })
      .finally(() => setLoading(false));
  }, [permissionName]);

  useEffect(() => {
    load();
  }, [load]);

  const hardDeps = deps.filter((d) => d.dependency_type === "hard");
  const warnDeps = deps.filter((d) => d.dependency_type !== "hard");

  return (
    <div className="w-72 shrink-0 rounded-lg border border-surface-border bg-surface-1 p-4">
      <div className="flex items-center justify-between">
        <h4 className="text-sm font-semibold text-text-primary">
          {t("permissions.dependencies.title")}
        </h4>
        <Button
          variant="ghost"
          className="h-10 w-10 p-0"
          onClick={onClose}
          aria-label={t("permissions.dependencies.close")}
          title={t("permissions.dependencies.close")}
        >
          <X className="h-4 w-4" aria-hidden />
        </Button>
      </div>
      <p className="mt-1 font-mono text-xs text-text-secondary">{permissionName}</p>

      {loading && <LoadingState className="mt-3 py-4" />}

      {!loading && loadError != null ? (
        <ErrorState
          error={loadError}
          title={t("permissions.dependencies.loadError")}
          onRetry={load}
          className="mt-3 px-3 py-4"
        />
      ) : null}

      {!loading && loadError == null && deps.length === 0 && (
        <p className="mt-3 text-xs text-text-secondary">{t("permissions.dependencies.none")}</p>
      )}

      {!loading && hardDeps.length > 0 && (
        <div className="mt-3 space-y-1">
          <h5 className="flex items-center gap-1 text-xs font-medium text-tone-danger-fg">
            <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
            {t("permissions.dependencies.hard")}
          </h5>
          {hardDeps.map((d) => (
            <div
              key={d.id}
              className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-2 py-1 text-xs"
            >
              {d.permission_name === permissionName ? (
                <span>
                  {t("permissions.dependencies.requires")}{" "}
                  <strong>{d.required_permission_name}</strong>
                </span>
              ) : (
                <span>
                  {t("permissions.dependencies.requiredBy")} <strong>{d.permission_name}</strong>
                </span>
              )}
            </div>
          ))}
        </div>
      )}

      {!loading && warnDeps.length > 0 && (
        <div className="mt-3 space-y-1">
          <h5 className="flex items-center gap-1 text-xs font-medium text-tone-warning-fg">
            <Info className="h-3.5 w-3.5" aria-hidden />
            {t("permissions.dependencies.warn")}
          </h5>
          {warnDeps.map((d) => (
            <div
              key={d.id}
              className="rounded-md border border-tone-warning-border bg-tone-warning-bg px-2 py-1 text-xs"
            >
              {d.permission_name === permissionName ? (
                <span>
                  {t("permissions.dependencies.suggests")}{" "}
                  <strong>{d.required_permission_name}</strong>
                </span>
              ) : (
                <span>
                  {t("permissions.dependencies.suggestedBy")} <strong>{d.permission_name}</strong>
                </span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Export helpers ───────────────────────────────────────────────────────────

function exportToCSV(permissions: PermissionWithSystem[]) {
  const header = "name,description,category,is_dangerous,requires_step_up,is_system";
  const rows = permissions.map((p) =>
    [
      p.name,
      `"${(p.description ?? "").replace(/"/g, '""')}"`,
      p.category,
      p.is_dangerous ? 1 : 0,
      p.requires_step_up ? 1 : 0,
      p.is_system ? 1 : 0,
    ].join(","),
  );
  const csv = [header, ...rows].join("\n");
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = "permission_catalog.csv";
  a.click();
  URL.revokeObjectURL(url);
}

// ── Main Panel ──────────────────────────────────────────────────────────────

export function PermissionCatalogPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();

  const [allPermissions, setAllPermissions] = useState<PermissionWithSystem[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [selectedDomain, setSelectedDomain] = useState<string>("eq");
  const [searchQuery, setSearchQuery] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [depViewerPerm, setDepViewerPerm] = useState<string | null>(null);

  const domainLabel = (domain: string) =>
    LABELLED_DOMAINS.has(domain) ? t(`permissions.domainLabels.${domain}`) : domain;

  const fetchPermissions = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const perms = await listPermissions({});
      setAllPermissions(perms);
    } catch (err) {
      // Shown in place of the list with a retry (never an empty catalog).
      setAllPermissions([]);
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchPermissions();
  }, [fetchPermissions]);

  // Group permissions by domain
  const permissionsByDomain = useMemo(() => {
    const groups: Record<string, PermissionWithSystem[]> = {};
    for (const p of allPermissions) {
      const domain = p.name.split(".")[0] ?? "_";
      (groups[domain] ??= []).push(p);
    }
    return groups;
  }, [allPermissions]);

  // Build sidebar domain list including custom domain if custom perms exist
  const domainList = useMemo(() => {
    const list = [...DOMAIN_GROUPS] as string[];
    if (permissionsByDomain["cst"] && permissionsByDomain["cst"].length > 0) {
      list.push("cst");
    }
    const extras = Object.keys(permissionsByDomain)
      .filter((d) => d !== "_" && !list.includes(d))
      .sort();
    list.push(...extras);
    return list;
  }, [permissionsByDomain]);

  // Filtered permissions for the selected domain
  const filteredPermissions = useMemo(() => {
    const domainPerms = permissionsByDomain[selectedDomain] ?? [];
    if (!searchQuery.trim()) return domainPerms;
    const q = searchQuery.toLowerCase();
    return domainPerms.filter(
      (p) => p.name.toLowerCase().includes(q) || (p.description ?? "").toLowerCase().includes(q),
    );
  }, [permissionsByDomain, selectedDomain, searchQuery]);

  return (
    <div className="flex gap-4">
      {/* Left sidebar: domain groups */}
      <div className="w-56 shrink-0 space-y-1">
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-sm font-semibold text-text-primary">{t("permissions.domains")}</h3>
          {can(P.ADM_PERMISSIONS) && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => exportToCSV(allPermissions)}
              disabled={loading || loadError != null}
              title={t("permissions.export")}
            >
              <Download className="h-4 w-4" />
            </Button>
          )}
        </div>

        {domainList.map((domain) => {
          const count = (permissionsByDomain[domain] ?? []).length;
          return (
            <button
              key={domain}
              type="button"
              onClick={() => {
                setSelectedDomain(domain);
                setSearchQuery("");
              }}
              className={cn(
                "flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors",
                selectedDomain === domain
                  ? "bg-primary/10 text-primary"
                  : "text-text-primary hover:bg-surface-2",
              )}
            >
              <div className="flex items-center gap-2">
                <span
                  className={cn(
                    "inline-flex h-5 items-center rounded px-1.5 text-[10px] font-semibold uppercase",
                    mfPermissionDomainChip[domain] ?? mfChip.neutral,
                  )}
                >
                  {domain}
                </span>
                <span className="truncate text-xs">{domainLabel(domain)}</span>
              </div>
              <Badge variant="secondary" className="text-[10px]">
                {count}
              </Badge>
            </button>
          );
        })}
      </div>

      {/* Right main area: permission table */}
      <div className="min-w-0 flex-1 rounded-lg border border-surface-border bg-surface-1 p-4">
        {/* Header */}
        <div className="mb-4 flex items-center justify-between">
          <div>
            <h3 className="flex items-center gap-2 text-lg font-semibold text-text-primary">
              <Shield className="h-5 w-5" />
              <span
                className={cn(
                  "inline-flex items-center rounded px-2 py-0.5 text-xs font-semibold uppercase",
                  mfPermissionDomainChip[selectedDomain] ?? mfChip.neutral,
                )}
              >
                {selectedDomain}
              </span>
              {domainLabel(selectedDomain)}
            </h3>
          </div>

          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-text-secondary" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={t("permissions.search")}
                className="w-48 pl-8"
              />
            </div>
            {can(P.ADM_PERMISSIONS) && (
              <Button size="sm" onClick={() => setShowCreate(true)}>
                <Plus className="mr-1.5 h-4 w-4" />
                {t("permissions.addCustom")}
              </Button>
            )}
          </div>
        </div>

        {/* Loading */}
        {loading && <p className="text-sm text-text-secondary">{t("common.loading")}</p>}

        {/* Load failure */}
        {!loading && loadError != null ? (
          <ErrorState
            error={loadError}
            title={t("permissions.errors.loadFailed")}
            onRetry={() => void fetchPermissions()}
          />
        ) : null}

        {/* Empty state */}
        {!loading && loadError == null && filteredPermissions.length === 0 && (
          <p className="text-sm text-text-secondary">{t("permissions.empty")}</p>
        )}

        {/* Permission table */}
        {!loading && filteredPermissions.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-surface-border text-left text-xs font-medium uppercase tracking-wide text-text-secondary">
                  <th className="px-3 py-2">{t("permissions.table.name")}</th>
                  <th className="px-3 py-2">{t("permissions.table.description")}</th>
                  <th className="px-3 py-2 text-center">{t("permissions.table.dangerous")}</th>
                  <th className="px-3 py-2 text-center">{t("permissions.table.stepUp")}</th>
                  <th className="px-3 py-2 text-center">{t("permissions.table.type")}</th>
                </tr>
              </thead>
              <tbody>
                {filteredPermissions.map((perm) => (
                  <tr
                    key={perm.id}
                    aria-selected={depViewerPerm === perm.name}
                    className={cn(
                      "border-b border-surface-border transition-colors hover:bg-surface-2",
                      depViewerPerm === perm.name && "bg-surface-2",
                    )}
                  >
                    <td className="px-3 py-1">
                      {/* The row action is a real button: reachable with Tab, opened with Enter. */}
                      <button
                        type="button"
                        className="min-h-9 rounded text-start font-mono text-xs text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        aria-label={t("permissions.dependencies.open", { name: perm.name })}
                        onClick={() => setDepViewerPerm(perm.name)}
                      >
                        {perm.name}
                      </button>
                    </td>
                    <td className="px-3 py-2 text-text-secondary">{perm.description ?? "—"}</td>
                    <td className="px-3 py-2 text-center">
                      {perm.is_dangerous && (
                        <StatusChip
                          tone="danger"
                          icon={ShieldAlert}
                          label={t("permissions.badges.dangerous")}
                        />
                      )}
                    </td>
                    <td className="px-3 py-2 text-center">
                      {perm.requires_step_up && (
                        <StatusChip
                          tone="attention"
                          icon={Lock}
                          label={t("permissions.badges.stepUp")}
                        />
                      )}
                    </td>
                    <td className="px-3 py-2 text-center">
                      <Badge
                        variant={perm.is_system ? "outline" : "secondary"}
                        className="text-[10px]"
                      >
                        {perm.is_system
                          ? t("permissions.badges.system")
                          : t("permissions.badges.custom")}
                      </Badge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Dependency viewer side panel */}
      {depViewerPerm && (
        <DependencyViewer permissionName={depViewerPerm} onClose={() => setDepViewerPerm(null)} />
      )}

      {/* Create dialog */}
      <CreatePermissionDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={() => void fetchPermissions()}
      />
    </div>
  );
}
