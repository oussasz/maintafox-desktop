import { Check, ChevronDown, Copy } from "lucide-react";
import { useId, useState } from "react";

import { cn } from "@/lib/utils";

type ActivationTechnicalDetailsProps = {
  summaryLabel: string;
  rows: Array<{ label: string; value: string }>;
  defaultOpen?: boolean;
  /** When both labels are given, a button copies the rows as text (useful for support tickets). */
  copyLabel?: string;
  copiedLabel?: string;
};

/** Collapsible diagnostics. Closed by default so the main task stays uncluttered. */
export function ActivationTechnicalDetails({
  summaryLabel,
  rows,
  defaultOpen = false,
  copyLabel,
  copiedLabel,
}: ActivationTechnicalDetailsProps) {
  const panelId = useId();
  const [open, setOpen] = useState(defaultOpen);
  const [copied, setCopied] = useState(false);

  const copyRows = async () => {
    try {
      await navigator.clipboard.writeText(rows.map((r) => `${r.label}: ${r.value}`).join("\n"));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard unavailable (permissions, insecure context): the rows stay selectable on screen.
    }
  };

  return (
    <div className="rounded-lg border border-surface-border bg-surface-2/60">
      <button
        type="button"
        className="flex min-h-10 w-full items-center justify-between rounded-lg px-3 py-2 text-left text-[13px] font-medium text-text-secondary transition-colors hover:text-text-primary"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((v) => !v)}
      >
        <span>{summaryLabel}</span>
        <ChevronDown
          className={cn("h-4 w-4 transition-transform", open && "rotate-180")}
          aria-hidden
        />
      </button>
      {open ? (
        <div id={panelId} className="border-t border-surface-border px-3 py-3">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
            {rows.map((row) => (
              <div key={row.label} className="contents">
                <dt className="text-text-muted">{row.label}</dt>
                <dd className="min-w-0 break-words font-mono text-text-primary">{row.value}</dd>
              </div>
            ))}
          </dl>
          {copyLabel && copiedLabel ? (
            <button
              type="button"
              onClick={() => void copyRows()}
              className="mt-3 inline-flex items-center gap-1.5 rounded-md text-xs font-medium text-[var(--mfa-blue)] hover:underline"
            >
              {copied ? (
                <Check className="h-3.5 w-3.5" aria-hidden />
              ) : (
                <Copy className="h-3.5 w-3.5" aria-hidden />
              )}
              <span aria-live="polite">{copied ? copiedLabel : copyLabel}</span>
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
