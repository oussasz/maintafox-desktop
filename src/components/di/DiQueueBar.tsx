/**
 * DiQueueBar.tsx
 *
 * Work queues of the requests screen (benchmark: Maximo service request queries, SAP PM
 * notification worklists): each queue is one step of the request lifecycle, with its live size.
 * A user sees the queues their role works on; everyone sees "All" and "My requests".
 * Orange (attention) only marks a queue that needs action now: SLA at risk, or my requests
 * waiting for my answer.
 */

import { useTranslation } from "react-i18next";

import { mfTone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import type { DiQueue, DiQueueCounts } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

/** `all` = no queue (every request, newest first). */
export type DiQueueSelection = DiQueue | "all";

interface QueueDef {
  id: DiQueueSelection;
  /** Count shown in the badge. */
  count: (c: DiQueueCounts) => number;
  /** Count that turns the badge orange (needs action now). */
  attention?: (c: DiQueueCounts) => number;
  visible: (can: Can) => boolean;
}

type Can = (permission: PermissionName) => boolean;

const canScreen = (can: Can) => can(P.DI_SCREEN) || can(P.DI_REVIEW);

const QUEUES: QueueDef[] = [
  { id: "all", count: () => -1, visible: () => true },
  {
    id: "mine",
    count: (c) => c.mine,
    attention: (c) => c.mine_awaiting_info,
    visible: (can) => can(P.DI_CREATE_OWN) || can(P.DI_CREATE),
  },
  { id: "to_screen", count: (c) => c.to_screen, visible: canScreen },
  { id: "to_review", count: (c) => c.to_review, visible: canScreen },
  { id: "awaiting_info", count: (c) => c.awaiting_info, visible: canScreen },
  { id: "to_approve", count: (c) => c.to_approve, visible: (can) => can(P.DI_APPROVE) },
  {
    id: "approved_not_converted",
    count: (c) => c.approved_not_converted,
    visible: (can) => can(P.DI_CONVERT) || can(P.DI_APPROVE),
  },
  { id: "deferred_due", count: (c) => c.deferred_due, visible: (can) => can(P.DI_APPROVE) },
  {
    id: "sla_at_risk",
    count: (c) => c.sla_at_risk,
    attention: (c) => c.sla_at_risk,
    visible: (can) => canScreen(can) || can(P.DI_APPROVE),
  },
];

/** Queues the signed-in user works on (always starts with "all"). */
export function useVisibleDiQueues(): DiQueueSelection[] {
  const { can } = usePermissions();
  return QUEUES.filter((q) => q.visible(can)).map((q) => q.id);
}

interface DiQueueBarProps {
  value: DiQueueSelection;
  counts: DiQueueCounts | null;
  onChange: (queue: DiQueueSelection) => void;
}

export function DiQueueBar({ value, counts, onChange }: DiQueueBarProps) {
  const { t } = useTranslation("di");
  const { can } = usePermissions();
  const visible = QUEUES.filter((q) => q.visible(can));

  return (
    <nav
      aria-label={t("queues.label")}
      className="flex items-center gap-1 overflow-x-auto border-b border-surface-border bg-surface-1 px-6 py-2"
    >
      {visible.map((q) => {
        const selected = q.id === value;
        const count = counts ? q.count(counts) : null;
        const urgent = counts && q.attention ? q.attention(counts) > 0 : false;
        return (
          <button
            key={q.id}
            type="button"
            aria-pressed={selected}
            onClick={() => onChange(q.id)}
            title={t(`queues.hint.${q.id}`)}
            className={cn(
              "inline-flex h-9 shrink-0 items-center gap-2 rounded-md px-3 text-sm font-medium transition-colors",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              selected
                ? "bg-primary/10 text-primary"
                : "text-text-secondary hover:bg-surface-2 hover:text-text-primary",
            )}
          >
            <span>{t(`queues.${q.id}`)}</span>
            {count != null && count >= 0 ? (
              <span
                className={cn(
                  "min-w-[1.5rem] rounded-full border px-1.5 py-0.5 text-center font-mono text-[11px] tabular-nums",
                  urgent
                    ? mfTone.attention.chip
                    : selected
                      ? "border-primary/30 bg-surface-1 text-primary"
                      : "border-surface-border bg-surface-2 text-text-muted",
                )}
              >
                {count}
              </span>
            ) : null}
          </button>
        );
      })}
    </nav>
  );
}
