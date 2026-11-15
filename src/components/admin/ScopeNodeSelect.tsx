import { useTranslation } from "react-i18next";

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AssignableScopeNode } from "@shared/ipc-types";

/** Select value for a plant-wide (tenant) scope. */
export const PLANT_SCOPE = "tenant";

/** `tenant`, or the id of an org node (the assignment covers the node and below). */
export type ScopeChoice = string;

export function scopeChoiceToInput(choice: ScopeChoice): {
  scope_type: string;
  scope_reference: string | null;
} {
  if (choice === PLANT_SCOPE) return { scope_type: "tenant", scope_reference: null };
  return { scope_type: "org_node", scope_reference: choice };
}

/** Readable scope of an assignment (whole plant, or the node / team name). */
export function useScopeLabel() {
  const { t } = useTranslation("admin");
  return (scopeType: string, scopeLabel: string | null, scopeReference: string | null) => {
    if (scopeType === "tenant") return t("console.scope.plant");
    if (scopeLabel) return scopeLabel;
    return scopeReference ? `#${scopeReference}` : t("console.scope.unknown");
  };
}

/**
 * The node that stands for the whole plant: the single top node of the organisation (made at
 * activation, its code is internal). `null` when the organisation has no single top node.
 */
export function wholePlantNodeId(nodes: AssignableScopeNode[]): number | null {
  const tops = nodes.filter((n) => n.parent_id === null);
  return tops.length === 1 && tops[0] ? tops[0].id : null;
}

interface ScopeNodeSelectProps {
  nodes: AssignableScopeNode[];
  value: ScopeChoice;
  onChange: (value: ScopeChoice) => void;
  /** Offer "whole plant" (role assignments); filters offer "all" instead. */
  includePlant?: boolean;
  disabled?: boolean;
  id?: string;
  ariaLabel?: string;
}

/** Plant-wide or one org node of the active structure, indented by depth. */
export function ScopeNodeSelect({
  nodes,
  value,
  onChange,
  includePlant = true,
  disabled,
  id,
  ariaLabel,
}: ScopeNodeSelectProps) {
  const { t } = useTranslation("admin");
  // The top node is the whole plant: with "Toute l'usine" offered it is not repeated, otherwise it
  // shows by its name alone (its code is an internal one, never a name for people).
  const topId = wholePlantNodeId(nodes);
  const shown = includePlant ? nodes.filter((n) => n.id !== topId) : nodes;
  const minDepth = shown.reduce((m, n) => Math.min(m, n.depth), Number.POSITIVE_INFINITY);
  return (
    <Select
      value={value}
      onValueChange={onChange}
      {...(disabled !== undefined ? { disabled } : {})}
    >
      <SelectTrigger {...(id ? { id } : {})} aria-label={ariaLabel ?? t("console.scope.label")}>
        <SelectValue placeholder={t("console.scope.choose")} />
      </SelectTrigger>
      <SelectContent>
        {includePlant ? (
          <SelectItem value={PLANT_SCOPE}>{t("console.scope.plant")}</SelectItem>
        ) : null}
        {shown.map((n) => {
          const indent = Number.isFinite(minDepth) ? Math.max(0, n.depth - minDepth) : 0;
          return (
            <SelectItem key={n.id} value={String(n.id)}>
              <span style={{ paddingLeft: `${indent * 12}px` }}>
                {n.name}
                {n.id === topId ? null : (
                  <span className="ml-1.5 text-xs text-text-muted">
                    {n.type_label ? `${n.type_label} · ` : ""}
                    {n.code}
                  </span>
                )}
              </span>
            </SelectItem>
          );
        })}
      </SelectContent>
    </Select>
  );
}
