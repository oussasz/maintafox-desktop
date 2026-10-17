/**
 * MoveNodeDialog.tsx
 *
 * First step of "Move unit": pick the new parent among the units the hierarchy rules allow,
 * then open the impact preview (which re-validates and applies the move on confirm).
 */

import { ArrowRightLeft, Check } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { moveParentCandidates, orgNodePathLabel } from "@/components/org/org-display";
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
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { listOrgRelationshipRules } from "@/services/org-service";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgDesignerNodeRow, OrgRelationshipRule } from "@shared/ipc-types";

interface MoveNodeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  node: OrgDesignerNodeRow;
}

export function MoveNodeDialog({ open, onOpenChange, node }: MoveNodeDialogProps) {
  const { t } = useTranslation("org");
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const openPreview = useOrgDesignerStore((s) => s.openPreview);

  const [rules, setRules] = useState<OrgRelationshipRule[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [query, setQuery] = useState("");
  const [targetId, setTargetId] = useState<number | null>(null);

  const modelId = node.structure_model_id ?? snapshot?.display_model_id ?? null;

  const loadRules = useCallback(async () => {
    if (modelId == null) return;
    setLoading(true);
    setError(null);
    try {
      setRules(await listOrgRelationshipRules(modelId));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [modelId]);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setTargetId(null);
    void loadRules();
  }, [open, loadRules]);

  const candidates = useMemo(
    () => moveParentCandidates(snapshot, node, rules),
    [snapshot, node, rules],
  );

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return candidates;
    return candidates.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        c.code.toLowerCase().includes(q) ||
        c.node_type_label.toLowerCase().includes(q),
    );
  }, [candidates, query]);

  const target = candidates.find((c) => c.node_id === targetId) ?? null;
  const currentParentLabel =
    node.parent_id != null ? orgNodePathLabel(snapshot, node.parent_id) : t("move.topLevel");

  const review = () => {
    if (!target) return;
    onOpenChange(false);
    void openPreview(
      { action: "MoveNode", node_id: node.node_id, new_parent_id: target.node_id },
      { targetLabel: target.name },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("move.title", { unit: node.name })}</DialogTitle>
          <DialogDescription>
            {t("move.currentParent", { parent: currentParentLabel })}
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <LoadingState label={t("move.loadingRules")} />
        ) : error != null ? (
          <ErrorState error={error} onRetry={() => void loadRules()} />
        ) : candidates.length === 0 ? (
          <EmptyState
            icon={<ArrowRightLeft className="h-6 w-6" aria-hidden />}
            title={t("move.noCandidates")}
            description={t("move.noCandidatesHint", { level: node.node_type_label })}
          />
        ) : (
          <div className="space-y-2">
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("move.searchPlaceholder")}
              aria-label={t("move.searchPlaceholder")}
              className="h-10"
            />
            <div
              role="radiogroup"
              aria-label={t("move.targetLabel")}
              className="max-h-72 space-y-1 overflow-y-auto rounded-md border border-surface-border p-1"
            >
              {visible.length === 0 ? (
                <p className="px-3 py-4 text-sm text-text-muted">{t("move.noMatch")}</p>
              ) : (
                visible.map((c) => {
                  const selected = c.node_id === targetId;
                  return (
                    <button
                      key={c.node_id}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      onClick={() => setTargetId(c.node_id)}
                      className={cn(
                        "flex min-h-11 w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm",
                        "hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                        selected && "bg-primary/10",
                      )}
                    >
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-medium text-text-primary">
                          {c.name}
                          <span className="ml-2 font-mono text-xs text-text-muted">{c.code}</span>
                        </span>
                        <span className="block truncate text-xs text-text-muted">
                          {c.node_type_label} · {orgNodePathLabel(snapshot, c.node_id)}
                        </span>
                      </span>
                      {selected && <Check className="h-4 w-4 shrink-0 text-primary" aria-hidden />}
                    </button>
                  );
                })
              )}
            </div>
          </div>
        )}

        <DialogFooter className="gap-2">
          <Button variant="outline" className="h-11" onClick={() => onOpenChange(false)}>
            {t("move.cancel")}
          </Button>
          <Button className="h-11" onClick={review} disabled={!target}>
            {t("move.review")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
