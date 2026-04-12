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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { usePermissions } from "@/hooks/use-permissions";
import { useToast } from "@/hooks/use-toast";
import { cn } from "@/lib/utils";
import {
  listPermissions,
  getPermissionDependencies,
  createCustomPermission,
} from "@/services/rbac-service";
import type { PermissionWithSystem, PermissionDependencyRow } from "@shared/ipc-types";

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

/** Human-readable domain labels (French). */
const DOMAIN_LABELS: Record<string, string> = {
  eq: "Équipements",
  di: "Demandes d'intervention",
  ot: "Ordres de travail",
  org: "Organisation",
  per: "Personnel",
  ref: "Données de référence",
  inv: "Inventaire",
  pm: "Maintenance préventive",
  ram: "Fiabilité / RAMS",
  rep: "Rapports",
  arc: "Archives",
  doc: "Documentation",
  plan: "Planification",
  log: "Journal d'activité",
  trn: "Formation",
  iot: "IoT",
  erp: "Connecteur ERP",
  ptw: "Permis de travail",
  fin: "Budget / Finance",
  ins: "Rondes d'inspection",
  cfg: "Configuration",
  adm: "Administration",
  cst: "Personnalisé",
};

/** Domain colour classes matching RoleEditorPanel. */
const DOMAIN_COLOURS: Record<string, string> = {
  eq: "bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-200",
  di: "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200",
  ot: "bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-200",
  org: "bg-pink-100 text-pink-800 dark:bg-pink-900 dark:text-pink-200",
  per: "bg-cyan-100 text-cyan-800 dark:bg-cyan-900 dark:text-cyan-200",
  ref: "bg-slate-100 text-slate-800 dark:bg-slate-900 dark:text-slate-200",
  inv: "bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200",
  pm: "bg-violet-100 text-violet-800 dark:bg-violet-900 dark:text-violet-200",
  ram: "bg-teal-100 text-teal-800 dark:bg-teal-900 dark:text-teal-200",
  rep: "bg-indigo-100 text-indigo-800 dark:bg-indigo-900 dark:text-indigo-200",
  arc: "bg-stone-100 text-stone-800 dark:bg-stone-900 dark:text-stone-200",
  doc: "bg-lime-100 text-lime-800 dark:bg-lime-900 dark:text-lime-200",
  plan: "bg-sky-100 text-sky-800 dark:bg-sky-900 dark:text-sky-200",
  log: "bg-neutral-100 text-neutral-800 dark:bg-neutral-900 dark:text-neutral-200",
  trn: "bg-fuchsia-100 text-fuchsia-800 dark:bg-fuchsia-900 dark:text-fuchsia-200",
  iot: "bg-rose-100 text-rose-800 dark:bg-rose-900 dark:text-rose-200",
  erp: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200",
  ptw: "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200",
  fin: "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200",
  ins: "bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200",
  cfg: "bg-zinc-100 text-zinc-800 dark:bg-zinc-900 dark:text-zinc-200",
  adm: "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200",
  cst: "bg-gray-100 text-gray-800 dark:bg-gray-800 dark:text-gray-200",
};

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
  const { toast } = useToast();
  const [name, setName] = useState("cst.");
  const [description, setDescription] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const nameError = useMemo(() => {
    const trimmed = name.trim().toLowerCase();
    if (!trimmed.startsWith("cst.")) {
      return t("permissions.errors.mustStartWithCst", "Le nom doit commencer par 'cst.'");
    }
    if (trimmed.length < 5) {
      return t("permissions.errors.nameTooShort", "Ajoutez au moins un caractère après 'cst.'");
    }
    for (const prefix of SYSTEM_PREFIXES) {
      if (trimmed.startsWith(prefix)) {
        return t("permissions.errors.systemPrefix", "Préfixe système réservé");
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
        title: t("permissions.created", "Permission créée"),
        variant: "success",
      });
      onCreated();
      onClose();
      setName("cst.");
      setDescription("");
    } catch {
      toast({
        title: t("permissions.errors.createFailed", "Erreur lors de la création"),
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
          <DialogTitle>
            {t("permissions.create.title", "Créer une permission personnalisée")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "permissions.create.description",
              "Les permissions personnalisées utilisent le préfixe 'cst.' et ne peuvent pas être dangereuses.",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("permissions.fields.name", "Nom")}</label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="cst.my_permission"
            />
            {nameError && <p className="text-xs text-red-600">{nameError}</p>}
          </div>
          <div className="space-y-1.5">
            <label className="text-sm font-medium">
              {t("permissions.fields.description", "Description")}
            </label>
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t(
                "permissions.create.descPlaceholder",
                "Brève description de cette permission",
              )}
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel", "Annuler")}
          </Button>
          <Button onClick={handleCreate} disabled={!!nameError || submitting}>
            {t("permissions.create.confirm", "Créer")}
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

  useEffect(() => {
    setLoading(true);
    getPermissionDependencies(permissionName)
      .then(setDeps)
      .catch(() => setDeps([]))
      .finally(() => setLoading(false));
  }, [permissionName]);

  const hardDeps = deps.filter((d) => d.dependency_type === "hard");
  const warnDeps = deps.filter((d) => d.dependency_type !== "hard");

  return (
    <div className="w-72 shrink-0 rounded-lg border border-surface-border bg-surface-1 p-4">
      <div className="flex items-center justify-between">
        <h4 className="text-sm font-semibold text-text-primary">
          {t("permissions.dependencies.title", "Dépendances")}
        </h4>
        <Button variant="ghost" size="sm" onClick={onClose}>
          <X className="h-4 w-4" />
        </Button>
      </div>
      <p className="mt-1 font-mono text-xs text-text-secondary">{permissionName}</p>

      {loading && (
        <p className="mt-3 text-xs text-text-secondary">{t("common.loading", "Chargement…")}</p>
      )}

      {!loading && deps.length === 0 && (
        <p className="mt-3 text-xs text-text-secondary">
          {t("permissions.dependencies.none", "Aucune dépendance")}
        </p>
      )}

      {!loading && hardDeps.length > 0 && (
        <div className="mt-3 space-y-1">
          <h5 className="flex items-center gap-1 text-xs font-medium text-red-600">
            <AlertTriangle className="h-3.5 w-3.5" />
            {t("permissions.dependencies.hard", "Obligatoire (hard)")}
          </h5>
          {hardDeps.map((d) => (
            <div
              key={d.id}
              className="rounded-md border border-red-200 bg-red-50 px-2 py-1 text-xs dark:border-red-900 dark:bg-red-950"
            >
              {d.permission_name === permissionName ? (
                <span>
                  {t("permissions.dependencies.requires", "Requiert")}{" "}
                  <strong>{d.required_permission_name}</strong>
                </span>
              ) : (
                <span>
                  {t("permissions.dependencies.requiredBy", "Requis par")}{" "}
                  <strong>{d.permission_name}</strong>
                </span>
              )}
            </div>
          ))}
        </div>
      )}

      {!loading && warnDeps.length > 0 && (
        <div className="mt-3 space-y-1">
          <h5 className="flex items-center gap-1 text-xs font-medium text-orange-600">
            <Info className="h-3.5 w-3.5" />
            {t("permissions.dependencies.warn", "Recommandé (warn)")}
          </h5>
          {warnDeps.map((d) => (
            <div
              key={d.id}
              className="rounded-md border border-orange-200 bg-orange-50 px-2 py-1 text-xs dark:border-orange-900 dark:bg-orange-950"
            >
              {d.permission_name === permissionName ? (
                <span>
                  {t("permissions.dependencies.suggests", "Suggère")}{" "}
                  <strong>{d.required_permission_name}</strong>
                </span>
              ) : (
                <span>
                  {t("permissions.dependencies.suggestedBy", "Suggéré par")}{" "}
                  <strong>{d.permission_name}</strong>
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
  const { toast } = useToast();

  const [allPermissions, setAllPermissions] = useState<PermissionWithSystem[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedDomain, setSelectedDomain] = useState<string>("eq");
  const [searchQuery, setSearchQuery] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [depViewerPerm, setDepViewerPerm] = useState<string | null>(null);

  const fetchPermissions = useCallback(async () => {
    setLoading(true);
    try {
      const perms = await listPermissions({});
      setAllPermissions(perms);
    } catch {
      toast({
        title: t("permissions.errors.loadFailed", "Erreur de chargement des permissions"),
        variant: "destructive",
      });
    } finally {
      setLoading(false);
    }
  }, [toast, t]);

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
          <h3 className="text-sm font-semibold text-text-primary">
            {t("permissions.domains", "Domaines")}
          </h3>
          {can("adm.permissions") && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => exportToCSV(allPermissions)}
              title={t("permissions.export", "Exporter CSV")}
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
                    DOMAIN_COLOURS[domain] ?? "bg-gray-100 text-gray-800",
                  )}
                >
                  {domain}
                </span>
                <span className="truncate text-xs">{DOMAIN_LABELS[domain] ?? domain}</span>
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
                  DOMAIN_COLOURS[selectedDomain] ?? "bg-gray-100 text-gray-800",
                )}
              >
                {selectedDomain}
              </span>
              {DOMAIN_LABELS[selectedDomain] ?? selectedDomain}
            </h3>
          </div>

          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-text-secondary" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={t("permissions.search", "Rechercher…")}
                className="w-48 pl-8"
              />
            </div>
            {can("adm.permissions") && (
              <Button size="sm" onClick={() => setShowCreate(true)}>
                <Plus className="mr-1.5 h-4 w-4" />
                {t("permissions.addCustom", "Ajouter")}
              </Button>
            )}
          </div>
        </div>

        {/* Loading */}
        {loading && (
          <p className="text-sm text-text-secondary">{t("common.loading", "Chargement…")}</p>
        )}

        {/* Empty state */}
        {!loading && filteredPermissions.length === 0 && (
          <p className="text-sm text-text-secondary">
            {t("permissions.empty", "Aucune permission dans ce domaine.")}
          </p>
        )}

        {/* Permission table */}
        {!loading && filteredPermissions.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-surface-border text-left text-xs font-medium uppercase tracking-wide text-text-secondary">
                  <th className="px-3 py-2">{t("permissions.table.name", "Nom")}</th>
                  <th className="px-3 py-2">{t("permissions.table.description", "Description")}</th>
                  <th className="px-3 py-2 text-center">
                    {t("permissions.table.dangerous", "Dangereux")}
                  </th>
                  <th className="px-3 py-2 text-center">
                    {t("permissions.table.stepUp", "Step-up")}
                  </th>
                  <th className="px-3 py-2 text-center">{t("permissions.table.type", "Type")}</th>
                </tr>
              </thead>
              <tbody>
                {filteredPermissions.map((perm) => (
                  <tr
                    key={perm.id}
                    onClick={() => setDepViewerPerm(perm.name)}
                    className="cursor-pointer border-b border-surface-border transition-colors hover:bg-surface-2"
                  >
                    <td className="px-3 py-2">
                      <span className="font-mono text-xs">{perm.name}</span>
                    </td>
                    <td className="px-3 py-2 text-text-secondary">{perm.description ?? "—"}</td>
                    <td className="px-3 py-2 text-center">
                      {perm.is_dangerous && (
                        <Badge variant="destructive" className="text-[10px]">
                          <ShieldAlert className="mr-1 h-3 w-3" />
                          {t("permissions.badges.dangerous", "Dangereux")}
                        </Badge>
                      )}
                    </td>
                    <td className="px-3 py-2 text-center">
                      {perm.requires_step_up && (
                        <Badge
                          variant="outline"
                          className="border-orange-300 text-[10px] text-orange-600"
                        >
                          <Lock className="mr-1 h-3 w-3" />
                          {t("permissions.badges.stepUp", "Step-up")}
                        </Badge>
                      )}
                    </td>
                    <td className="px-3 py-2 text-center">
                      <Badge
                        variant={perm.is_system ? "outline" : "secondary"}
                        className="text-[10px]"
                      >
                        {perm.is_system
                          ? t("permissions.badges.system", "Système")
                          : t("permissions.badges.custom", "Personnalisé")}
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
