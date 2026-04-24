/**
 * Draft-scoped parent→child relationship rules. Required for publish validation:
 * every live parent/child edge must appear as a rule, and every type must be
 * reachable from the root type through the rule graph.
 */

import { ChevronDown, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardTitle } from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import {
  createOrgRelationshipRule,
  deleteOrgRelationshipRule,
  listOrgNodeTypes,
  listOrgRelationshipRules,
} from "@/services/org-service";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";
import { toErrorMessage } from "@/utils/errors";
import type { OrgNodeType, OrgRelationshipRule } from "@shared/ipc-types";

interface OrgRelationshipRulesPanelProps {
  structureModelId: number;
  onChanged: () => void;
}

export function OrgRelationshipRulesPanel({
  structureModelId,
  onChanged,
}: OrgRelationshipRulesPanelProps) {
  const { t } = useTranslation("org");
  const loadPublishValidation = useOrgGovernanceStore((s) => s.loadPublishValidation);

  const [types, setTypes] = useState<OrgNodeType[]>([]);
  const [rules, setRules] = useState<OrgRelationshipRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [parentId, setParentId] = useState<string>("");
  const [childId, setChildId] = useState<string>("");
  const [sectionOpen, setSectionOpen] = useState(false);

  const load = useCallback(async () => {
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
      setError(toErrorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [structureModelId]);

  useEffect(() => {
    void load();
  }, [load]);

  const addRule = async () => {
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
      setError(toErrorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const removeRule = async (ruleId: number) => {
    setSaving(true);
    setError(null);
    try {
      await deleteOrgRelationshipRule(ruleId);
      await load();
      onChanged();
      void loadPublishValidation(structureModelId);
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="mx-6 mt-2 shrink-0">
      <Card className="overflow-hidden">
        <button
          type="button"
          className="flex w-full items-start gap-2 p-3 text-left hover:bg-surface-2/50 transition-colors"
          onClick={() => setSectionOpen((o) => !o)}
          aria-expanded={sectionOpen}
        >
          <ChevronDown
            className={cn(
              "h-4 w-4 mt-0.5 shrink-0 text-text-muted transition-transform",
              sectionOpen && "rotate-180",
            )}
          />
          <div className="min-w-0 flex-1">
            <CardTitle className="text-sm text-left">{t("relationshipRules.title")}</CardTitle>
            <CardDescription className="text-xs text-left line-clamp-1">
              {t("relationshipRules.hint")}
            </CardDescription>
            {!sectionOpen && rules.length > 0 && (
              <p className="text-[10px] text-text-muted mt-1">
                {t("relationshipRules.countHint", { count: rules.length })}
              </p>
            )}
          </div>
        </button>
        {sectionOpen && (
          <CardContent className="max-h-[min(40vh,22rem)] overflow-y-auto space-y-3 text-sm border-t border-surface-border/80 pt-3">
            {loading ? (
              <p className="text-xs text-text-muted">{t("relationshipRules.loading")}</p>
            ) : (
              <>
                {rules.length === 0 ? (
                  <p className="text-xs text-text-muted">{t("relationshipRules.empty")}</p>
                ) : (
                  <ul className="space-y-1.5 border border-surface-border rounded-md p-2 text-xs max-h-40 overflow-y-auto">
                    {rules.map((r) => (
                      <li
                        key={r.id}
                        className="flex items-center justify-between gap-2 py-0.5 border-b border-surface-border/50 last:border-0"
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
                        <PermissionGate permission="org.admin">
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

                <PermissionGate permission="org.admin">
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
          </CardContent>
        )}
      </Card>
    </div>
  );
}
