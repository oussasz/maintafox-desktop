/**
 * Draft-scoped parent→child relationship rules. Required for publish validation:
 * every live parent/child edge must appear as a rule, and every type must be
 * reachable from the root type through the rule graph.
 *
 * Dialog overlay matching NodeTypeManagerPanel (config task, not page chrome).
 */

import { Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  createOrgRelationshipRule,
  deleteOrgRelationshipRule,
  listOrgNodeTypes,
  listOrgRelationshipRules,
} from "@/services/org-service";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";
import { formatOrgIpcError } from "@/utils/errors";
import type { OrgNodeType, OrgRelationshipRule } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface OrgRelationshipRulesPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  structureModelId: number | null;
  onChanged: () => void;
}

export function OrgRelationshipRulesPanel({
  open,
  onOpenChange,
  structureModelId,
  onChanged,
}: OrgRelationshipRulesPanelProps) {
  const { t } = useTranslation("org");
  const loadPublishValidation = useOrgGovernanceStore((s) => s.loadPublishValidation);

  const [types, setTypes] = useState<OrgNodeType[]>([]);
  const [rules, setRules] = useState<OrgRelationshipRule[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [parentId, setParentId] = useState<string>("");
  const [childId, setChildId] = useState<string>("");

  const load = useCallback(async () => {
    if (structureModelId == null) {
      setTypes([]);
      setRules([]);
      setError(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const [tList, rList] = await Promise.all([
        listOrgNodeTypes(structureModelId),
        listOrgRelationshipRules(structureModelId),
      ]);
      setTypes(tList.filter((x) => x.is_active));
      setRules(rList);
    } catch (e) {
      setError(formatOrgIpcError(e));
    } finally {
      setLoading(false);
    }
  }, [structureModelId]);

  useEffect(() => {
    if (open) {
      setParentId("");
      setChildId("");
      setError(null);
      void load();
    }
  }, [open, load]);

  const addRule = async () => {
    if (structureModelId == null) return;
    const p = parseInt(parentId, 10);
    const c = parseInt(childId, 10);
    if (Number.isNaN(p) || Number.isNaN(c) || p === c) return;
    const exists = rules.some((r) => r.parent_type_id === p && r.child_type_id === c);
    if (exists) {
      setError(t("relationshipRules.duplicatePair"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await createOrgRelationshipRule({
        structure_model_id: structureModelId,
        parent_type_id: p,
        child_type_id: c,
      });
      setParentId("");
      setChildId("");
      await load();
      onChanged();
      void loadPublishValidation(structureModelId);
    } catch (e) {
      setError(formatOrgIpcError(e));
    } finally {
      setSaving(false);
    }
  };

  const removeRule = async (ruleId: number) => {
    if (structureModelId == null) return;
    setSaving(true);
    setError(null);
    try {
      await deleteOrgRelationshipRule(ruleId);
      await load();
      onChanged();
      void loadPublishValidation(structureModelId);
    } catch (e) {
      setError(formatOrgIpcError(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onOpenChange(false)}>
      <DialogContent
        className="max-w-2xl max-h-[85vh] flex flex-col p-0 gap-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader className="px-6 py-4 border-b border-surface-border">
          <DialogTitle>{t("relationshipRules.title")}</DialogTitle>
          <DialogDescription>{t("relationshipRules.hint")}</DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4 text-sm">
          {structureModelId == null ? (
            <p className="text-sm text-text-muted">
              {t("designer.manageHierarchyRulesNoDraftHint")}
            </p>
          ) : loading ? (
            <p className="text-sm text-text-muted">{t("relationshipRules.loading")}</p>
          ) : (
            <>
              {rules.length === 0 ? (
                <p className="text-sm text-text-muted">{t("relationshipRules.empty")}</p>
              ) : (
                <ul className="space-y-1.5 border border-surface-border rounded-md p-2 text-xs max-h-56 overflow-y-auto">
                  {rules.map((r) => (
                    <li
                      key={r.id}
                      className="flex items-center justify-between gap-2 py-1.5 border-b border-surface-border/50 last:border-0"
                    >
                      <span>
                        <span className="font-mono text-text-muted">
                          {r.parent_type_label ?? "?"}
                        </span>
                        <span className="mx-1.5 text-text-muted">→</span>
                        <span className="font-mono text-text-primary">
                          {r.child_type_label ?? "?"}
                        </span>
                      </span>
                      <PermissionGate permission={P.ORG_ADMIN}>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          className="h-7 w-7 p-0 text-status-danger"
                          onClick={() => void removeRule(r.id)}
                          disabled={saving}
                          aria-label={t("relationshipRules.remove")}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      </PermissionGate>
                    </li>
                  ))}
                </ul>
              )}

              <PermissionGate permission={P.ORG_ADMIN}>
                <div className="flex flex-wrap items-end gap-2 pt-1">
                  <div className="space-y-1 min-w-[140px]">
                    <span className="text-[10px] text-text-muted uppercase">
                      {t("relationshipRules.parentType")}
                    </span>
                    <Select value={parentId} onValueChange={setParentId}>
                      <SelectTrigger className="h-8 text-xs">
                        <SelectValue placeholder={t("relationshipRules.pick")} />
                      </SelectTrigger>
                      <SelectContent>
                        {types.map((nt) => (
                          <SelectItem key={nt.id} value={String(nt.id)}>
                            {nt.label} ({nt.code})
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-1 min-w-[140px]">
                    <span className="text-[10px] text-text-muted uppercase">
                      {t("relationshipRules.childType")}
                    </span>
                    <Select value={childId} onValueChange={setChildId}>
                      <SelectTrigger className="h-8 text-xs">
                        <SelectValue placeholder={t("relationshipRules.pick")} />
                      </SelectTrigger>
                      <SelectContent>
                        {types.map((nt) => (
                          <SelectItem key={nt.id} value={String(nt.id)}>
                            {nt.label} ({nt.code})
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    className="h-8"
                    onClick={() => void addRule()}
                    disabled={saving || !parentId || !childId || parentId === childId}
                  >
                    {t("relationshipRules.add")}
                  </Button>
                </div>
              </PermissionGate>

              {error && (
                <p className="text-xs text-status-danger" role="alert">
                  {error}
                </p>
              )}
            </>
          )}
        </div>

        <DialogFooter className="px-6 py-3 border-t border-surface-border">
          <Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("lifecycle.cancel")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
