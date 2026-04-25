/**
 * Create a live org node under the active structure model (types + relationship rules
 * are resolved from the published active model, not the draft).
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { Textarea } from "@/components/ui/textarea";
import { createOrgNode } from "@/services/org-node-service";
import { listOrgNodeTypes, listOrgRelationshipRules } from "@/services/org-service";
import { toErrorMessage } from "@/utils/errors";
import type { OrgDesignerNodeRow, OrgNodeType, OrgRelationshipRule } from "@shared/ipc-types";

type CreateMode = "root" | "child";

export interface OrgNodeCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: CreateMode;
  /** Required when mode is "child". */
  parentNode: OrgDesignerNodeRow | null;
  activeModelId: number;
  onCreated: (nodeId: number) => void;
}

export function OrgNodeCreateDialog({
  open,
  onOpenChange,
  mode,
  parentNode,
  activeModelId,
  onCreated,
}: OrgNodeCreateDialogProps) {
  const { t } = useTranslation("org");
  const [types, setTypes] = useState<OrgNodeType[]>([]);
  const [rules, setRules] = useState<OrgRelationshipRule[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [nodeTypeId, setNodeTypeId] = useState<string>("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [tList, rList] = await Promise.all([
        listOrgNodeTypes(activeModelId),
        listOrgRelationshipRules(activeModelId),
      ]);
      setTypes(tList.filter((x) => x.is_active));
      setRules(rList);
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [activeModelId]);

  useEffect(() => {
    if (open) {
      setCode("");
      setName("");
      setDescription("");
      setNodeTypeId("");
      void load();
    }
  }, [open, load]);

  const allowedChildTypes = useMemo(() => {
    if (mode === "root") {
      return types.filter((x) => x.is_root_type);
    }
    if (!parentNode) return [];
    const childIds = new Set(
      rules.filter((r) => r.parent_type_id === parentNode.node_type_id).map((r) => r.child_type_id),
    );
    return types.filter((x) => childIds.has(x.id) && !x.is_root_type);
  }, [mode, parentNode, types, rules]);

  useEffect(() => {
    if (allowedChildTypes.length === 0) {
      setNodeTypeId("");
      return;
    }
    const currentOk = nodeTypeId && allowedChildTypes.some((x) => String(x.id) === nodeTypeId);
    const first = allowedChildTypes[0];
    if (first == null) return;
    if (!nodeTypeId || !currentOk) {
      setNodeTypeId(String(first.id));
    }
  }, [allowedChildTypes, nodeTypeId]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (mode === "child" && !parentNode) {
      setError(t("createNode.missingParent"));
      return;
    }
    if (!nodeTypeId) {
      setError(t("createNode.noTypeForContext"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const nType = parseInt(nodeTypeId, 10);
      const result = await createOrgNode({
        code: code.trim(),
        name: name.trim(),
        node_type_id: nType,
        parent_id: mode === "root" || !parentNode ? null : parentNode.node_id,
        description: description.trim() || null,
      });
      onCreated(result.id);
      onOpenChange(false);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const canSubmit = code.trim() && name.trim() && nodeTypeId && !loading && !saving;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <form onSubmit={(e) => void handleSubmit(e)}>
          <DialogHeader>
            <DialogTitle>
              {mode === "root" ? t("createNode.titleRoot") : t("createNode.titleChild")}
            </DialogTitle>
            <DialogDescription>
              {mode === "child" && parentNode
                ? t("createNode.parentHint", { name: parentNode.name, code: parentNode.code })
                : t("createNode.hintRoot")}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-3 py-2">
            {allowedChildTypes.length === 0 && !loading && (
              <p className="text-xs text-status-warning">{t("createNode.noAllowedTypes")}</p>
            )}

            <div className="space-y-1.5">
              <Label htmlFor="org-node-code" className="text-xs">
                {t("createNode.codeLabel")}
              </Label>
              <Input
                id="org-node-code"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                className="font-mono h-8 text-sm"
                autoComplete="off"
                disabled={saving}
                placeholder={t("createNode.codePlaceholder")}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="org-node-name" className="text-xs">
                {t("createNode.nameLabel")}
              </Label>
              <Input
                id="org-node-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="h-8 text-sm"
                disabled={saving}
                placeholder={t("createNode.namePlaceholder")}
              />
            </div>
            <div className="space-y-1.5">
              <span className="text-xs text-text-muted">{t("createNode.typeLabel")}</span>
              <Select value={nodeTypeId} onValueChange={setNodeTypeId} disabled={saving || loading}>
                <SelectTrigger className="h-8 text-xs">
                  <SelectValue placeholder={t("createNode.typePlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {allowedChildTypes.map((nt) => (
                    <SelectItem key={nt.id} value={String(nt.id)}>
                      {nt.label} ({nt.code})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="org-node-desc" className="text-xs text-text-muted">
                {t("createNode.descriptionLabel")}
              </Label>
              <Textarea
                id="org-node-desc"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                className="min-h-[64px] text-sm"
                disabled={saving}
              />
            </div>

            {error && (
              <p className="text-xs text-status-danger" role="alert">
                {error}
              </p>
            )}
          </div>

          <DialogFooter className="gap-2 sm:gap-0">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={saving}
            >
              {t("createNode.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit || allowedChildTypes.length === 0}>
              {saving ? t("createNode.saving") : t("createNode.create")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
