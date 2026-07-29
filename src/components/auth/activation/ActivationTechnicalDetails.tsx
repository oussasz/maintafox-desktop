import { useId, useState } from "react";

import { Button } from "@/components/ui/button";

type ActivationTechnicalDetailsProps = {
  summaryLabel: string;
  rows: Array<{ label: string; value: string }>;
  defaultOpen?: boolean;
};

export function ActivationTechnicalDetails({
  summaryLabel,
  rows,
  defaultOpen = false,
}: ActivationTechnicalDetailsProps) {
  const panelId = useId();
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className="rounded-md border border-surface-border">
      <Button
        type="button"
        variant="ghost"
        className="h-auto w-full justify-between rounded-md px-3 py-2 text-left text-xs font-medium text-text-secondary"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((v) => !v)}
      >
        <span>{summaryLabel}</span>
        <span aria-hidden>{open ? "−" : "+"}</span>
      </Button>
      {open ? (
        <div id={panelId} className="space-y-1 border-t border-surface-border px-3 py-2 text-xs text-text-secondary">
          {rows.map((row) => (
            <p key={row.label}>
              {row.label}: <span className="font-mono text-text-primary">{row.value}</span>
            </p>
          ))}
        </div>
      ) : null}
    </div>
  );
}
