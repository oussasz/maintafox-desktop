/**
 * Shared completion / closeout readiness checklist.
 */

import { Check, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { WoCompletionGate } from "@shared/ipc-types";

export function WoCompletionGatesChecklist({
  gates,
  title,
  showBlockingCount = true,
}: {
  gates: WoCompletionGate[];
  title?: string;
  showBlockingCount?: boolean;
}) {
  const { t } = useTranslation("ot");
  const visible = gates.filter((g) => g.required);
  const blocking = visible.filter((g) => !g.passed).length;
  const done = visible.filter((g) => g.passed).length;

  if (visible.length === 0) return null;

  return (
    <div className="space-y-2 rounded-md border bg-muted/20 p-3">
      {title ? <p className="text-sm font-semibold">{title}</p> : null}
      <ul className="space-y-1.5">
        {visible.map((gate) => (
          <li key={gate.code} className="flex items-start gap-2 text-sm">
            {gate.passed ? (
              <Check className="mt-0.5 h-4 w-4 shrink-0 text-green-700" aria-hidden />
            ) : (
              <X className="mt-0.5 h-4 w-4 shrink-0 text-destructive" aria-hidden />
            )}
            <span className={gate.passed ? "text-foreground" : "text-destructive"}>
              {t(`completion.gates.${gate.code}`, { defaultValue: gate.code })}
            </span>
          </li>
        ))}
      </ul>
      {showBlockingCount && blocking > 0 ? (
        <p className="border-t pt-2 text-xs text-muted-foreground">
          {t("completion.gates.blockingCount", { count: blocking, done, total: visible.length })}
        </p>
      ) : null}
    </div>
  );
}

export function completionGatesProgress(gates: WoCompletionGate[]): {
  done: number;
  total: number;
} {
  const visible = gates.filter((g) => g.required);
  return {
    done: visible.filter((g) => g.passed).length,
    total: visible.length,
  };
}
