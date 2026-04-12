import { type ColumnDef } from "@tanstack/react-table";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useToast } from "@/hooks/use-toast";
import {
  createDelegationPolicy,
  deleteDelegationPolicy,
  listDelegationPolicies,
  listPermissions,
  listRoles,
  updateDelegationPolicy,
} from "@/services/rbac-service";
import type { DelegationPolicyView, RoleWithPermissions } from "@shared/ipc-types";

const SCOPE_TYPES = ["tenant", "site", "department"] as const;

export function DelegationManagerPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { toast } = useToast();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [policies, setPolicies] = useState<DelegationPolicyView[]>([]);
  const [roles, setRoles] = useState<RoleWithPermissions[]>([]);
  const [knownDomains, setKnownDomains] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  // Create dialog state
  const [showCreate, setShowCreate] = useState(false);
  const [createRoleId, setCreateRoleId] = useState("");
  const [createScopeType, setCreateScopeType] = useState("tenant");
  const [createScopeRef, setCreateScopeRef] = useState("");
  const [createDomains, setCreateDomains] = useState<Set<string>>(new Set());
  const [createStepUp, setCreateStepUp] = useState(true);

  // Edit dialog state
  const [editTarget, setEditTarget] = useState<DelegationPolicyView | null>(null);
  const [editDomains, setEditDomains] = useState<Set<string>>(new Set());
  const [editStepUp, setEditStepUp] = useState(true);

  // Delete dialog state
  const [deleteTarget, setDeleteTarget] = useState<DelegationPolicyView | null>(null);

  const fetchData = useCallback(async () => {
    try {
      const [policiesData, rolesData, permsData] = await Promise.all([
        listDelegationPolicies(),
        listRoles(),
        listPermissions({}),
      ]);
      setPolicies(policiesData);
      setRoles(rolesData);

      const domains = new Set<string>();
      for (const p of permsData) {
        if (p.category) domains.add(p.category);
      }
      setKnownDomains([...domains].sort());
    } catch {
      toast({
        title: t("delegation.errors.loadFailed", "Failed to load policies"),
        variant: "destructive",
      });
    } finally {
      setLoading(false);
    }
  }, [t, toast]);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  const nonSystemRoles = useMemo(() => roles.filter((r) => !r.is_system), [roles]);

  // ── Create ──────────────────────────────────────────────────────────────

  const resetCreateForm = useCallback(() => {
    setCreateRoleId("");
    setCreateScopeType("tenant");
    setCreateScopeRef("");
    setCreateDomains(new Set());
    setCreateStepUp(true);
  }, []);

  const handleCreate = useCallback(async () => {
    try {
      await withStepUp(() =>
        createDelegationPolicy({
          admin_role_id: Number(createRoleId),
          managed_scope_type: createScopeType,
          managed_scope_reference:
            createScopeType !== "tenant" && createScopeRef ? createScopeRef : null,
          allowed_domains: [...createDomains],
          requires_step_up_for_publish: createStepUp,
        }),
      );
      toast({ title: t("delegation.created", "Policy created"), variant: "success" });
      setShowCreate(false);
      resetCreateForm();
      void fetchData();
    } catch (err) {
      toast({
        title: err instanceof Error ? err.message : t("delegation.errors.createFailed"),
        variant: "destructive",
      });
    }
  }, [
    createRoleId,
    createScopeType,
    createScopeRef,
    createDomains,
    createStepUp,
    fetchData,
    resetCreateForm,
    t,
    toast,
    withStepUp,
  ]);

  const canCreate = createRoleId && createDomains.size > 0;

  // ── Edit ────────────────────────────────────────────────────────────────

  const openEdit = useCallback((policy: DelegationPolicyView) => {
    setEditTarget(policy);
    setEditDomains(new Set(policy.allowed_domains));
    setEditStepUp(policy.requires_step_up_for_publish);
  }, []);

  const handleUpdate = useCallback(async () => {
    if (!editTarget) return;
    try {
      await withStepUp(() =>
        updateDelegationPolicy({
          policy_id: editTarget.id,
          allowed_domains: [...editDomains],
          requires_step_up_for_publish: editStepUp,
        }),
      );
      toast({ title: t("delegation.updated", "Policy updated"), variant: "success" });
      setEditTarget(null);
      void fetchData();
    } catch (err) {
      toast({
        title: err instanceof Error ? err.message : t("delegation.errors.updateFailed"),
        variant: "destructive",
      });
    }
  }, [editTarget, editDomains, editStepUp, fetchData, t, toast, withStepUp]);

  // ── Delete ──────────────────────────────────────────────────────────────

  const handleDelete = useCallback(async () => {
    if (!deleteTarget) return;
    try {
      await withStepUp(() => deleteDelegationPolicy(deleteTarget.id));
      toast({ title: t("delegation.deleted", "Policy deleted"), variant: "success" });
      setDeleteTarget(null);
      void fetchData();
    } catch {
      toast({
        title: t("delegation.errors.deleteFailed", "Failed to delete policy"),
        variant: "destructive",
      });
    }
  }, [deleteTarget, fetchData, t, toast, withStepUp]);

  // ── Domain toggle helper ────────────────────────────────────────────────

  const toggleDomain = useCallback(
    (domain: string, set: Set<string>, setter: (s: Set<string>) => void) => {
      const next = new Set(set);
      if (next.has(domain)) {
        next.delete(domain);
      } else {
        next.add(domain);
      }
      setter(next);
    },
    [],
  );

  // ── Table columns ───────────────────────────────────────────────────────

  const columns: ColumnDef<DelegationPolicyView>[] = useMemo(
    () => [
      {
        accessorKey: "admin_role_name",
        header: t("delegation.columns.adminRole", "Admin Role"),
        cell: ({ row }) => <span className="font-medium">{row.original.admin_role_name}</span>,
      },
      {
        id: "scope",
        header: t("delegation.columns.scope", "Scope"),
        cell: ({ row }) => (
          <span>
            {t(
              `delegation.scopeTypes.${row.original.managed_scope_type}`,
              row.original.managed_scope_type,
            )}
            {row.original.managed_scope_reference && (
              <span className="ml-1 text-text-muted">({row.original.managed_scope_reference})</span>
            )}
          </span>
        ),
      },
      {
        id: "domains",
        header: t("delegation.columns.domains", "Allowed Domains"),
        cell: ({ row }) => (
          <div className="flex flex-wrap gap-1">
            {row.original.allowed_domains.map((d) => (
              <Badge key={d} variant="secondary" className="text-[10px]">
                {d}
              </Badge>
            ))}
          </div>
        ),
      },
      {
        id: "stepUp",
        header: t("delegation.columns.stepUp", "Step-up"),
        cell: ({ row }) =>
          row.original.requires_step_up_for_publish ? (
            <Badge variant="outline" className="border-orange-300 text-orange-600 text-[10px]">
              Step-up
            </Badge>
          ) : (
            <span className="text-text-muted text-xs">—</span>
          ),
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) => {
          if (!can("adm.roles")) return null;
          return (
            <div className="flex items-center gap-1">
              <Button variant="ghost" size="sm" onClick={() => openEdit(row.original)}>
                <Pencil className="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="text-destructive hover:text-destructive"
                onClick={() => setDeleteTarget(row.original)}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          );
        },
      },
    ],
    [t, can, openEdit],
  );

  // ── Domain picker component ─────────────────────────────────────────────

  const DomainPicker = ({
    selected,
    onToggle,
  }: {
    selected: Set<string>;
    onToggle: (domain: string) => void;
  }) => (
    <div className="flex flex-wrap gap-2">
      {knownDomains.map((d) => (
        <button
          key={d}
          type="button"
          onClick={() => onToggle(d)}
          className={`rounded-md border px-2.5 py-1 text-xs font-medium transition-colors ${
            selected.has(d)
              ? "border-primary bg-primary/10 text-primary"
              : "border-surface-border text-text-secondary hover:border-primary/50"
          }`}
        >
          {d}
        </button>
      ))}
    </div>
  );

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-text-primary">
          {t("delegation.title", "Delegation Policies")}
        </h2>
        {can("adm.roles") && (
          <Button
            size="sm"
            onClick={() => {
              resetCreateForm();
              setShowCreate(true);
            }}
          >
            <Plus className="mr-1.5 h-4 w-4" />
            {t("delegation.create", "New Policy")}
          </Button>
        )}
      </div>

      {/* Table */}
      <DataTable<DelegationPolicyView, unknown>
        columns={columns}
        data={policies}
        isLoading={loading}
        skeletonRows={4}
        pageSize={20}
      />

      {/* ── Create Dialog ────────────────────────────────────────────────── */}
      <Dialog open={showCreate} onOpenChange={(v) => !v && setShowCreate(false)}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("delegation.createTitle", "Create Delegation Policy")}</DialogTitle>
            <DialogDescription>{t("delegation.createDesc")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            {/* Role */}
            <div className="space-y-1.5">
              <Label>{t("delegation.fields.adminRole", "Admin Role")}</Label>
              <Select value={createRoleId} onValueChange={setCreateRoleId}>
                <SelectTrigger>
                  <SelectValue placeholder={t("delegation.fields.selectRole", "Choose a role")} />
                </SelectTrigger>
                <SelectContent>
                  {nonSystemRoles.map((r) => (
                    <SelectItem key={r.id} value={String(r.id)}>
                      {r.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Scope Type */}
            <div className="space-y-1.5">
              <Label>{t("delegation.fields.scopeType", "Scope Type")}</Label>
              <Select value={createScopeType} onValueChange={setCreateScopeType}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SCOPE_TYPES.map((s) => (
                    <SelectItem key={s} value={s}>
                      {t(`delegation.scopeTypes.${s}`, s)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Scope Reference (only if not tenant) */}
            {createScopeType !== "tenant" && (
              <div className="space-y-1.5">
                <Label>{t("delegation.fields.scopeReference", "Scope Reference")}</Label>
                <Input
                  value={createScopeRef}
                  onChange={(e) => setCreateScopeRef(e.target.value)}
                  placeholder="org_unit ID"
                />
              </div>
            )}

            {/* Allowed Domains */}
            <div className="space-y-1.5">
              <Label>{t("delegation.fields.allowedDomains", "Allowed Domains")}</Label>
              <DomainPicker
                selected={createDomains}
                onToggle={(d) => toggleDomain(d, createDomains, setCreateDomains)}
              />
            </div>

            {/* Step-up */}
            <div className="flex items-center gap-3">
              <Switch checked={createStepUp} onCheckedChange={setCreateStepUp} />
              <Label>
                {t("delegation.fields.requiresStepUp", "Require step-up for publishing")}
              </Label>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowCreate(false)}>
              {t("common.cancel", "Cancel")}
            </Button>
            <Button onClick={() => void handleCreate()} disabled={!canCreate}>
              {t("delegation.create", "Create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Edit Dialog ──────────────────────────────────────────────────── */}
      <Dialog open={!!editTarget} onOpenChange={(v) => !v && setEditTarget(null)}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("delegation.editTitle", "Edit Delegation Policy")}</DialogTitle>
            <DialogDescription>
              {editTarget?.admin_role_name} —{" "}
              {t(`delegation.scopeTypes.${editTarget?.managed_scope_type ?? "tenant"}`)}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label>{t("delegation.fields.allowedDomains", "Allowed Domains")}</Label>
              <DomainPicker
                selected={editDomains}
                onToggle={(d) => toggleDomain(d, editDomains, setEditDomains)}
              />
            </div>
            <div className="flex items-center gap-3">
              <Switch checked={editStepUp} onCheckedChange={setEditStepUp} />
              <Label>
                {t("delegation.fields.requiresStepUp", "Require step-up for publishing")}
              </Label>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditTarget(null)}>
              {t("common.cancel", "Cancel")}
            </Button>
            <Button onClick={() => void handleUpdate()} disabled={editDomains.size === 0}>
              {t("delegation.edit", "Save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Delete Dialog ────────────────────────────────────────────────── */}
      <Dialog open={!!deleteTarget} onOpenChange={(v) => !v && setDeleteTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("delegation.deleteConfirm", "Delete this policy?")}</DialogTitle>
            <DialogDescription>{t("delegation.deleteConfirmDesc")}</DialogDescription>
          </DialogHeader>
          <p className="text-sm text-text-secondary">
            {deleteTarget?.admin_role_name} — {deleteTarget?.allowed_domains.join(", ")}
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              {t("common.cancel", "Cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void handleDelete()}>
              {t("delegation.delete", "Delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {StepUpDialogElement}
    </div>
  );
}
