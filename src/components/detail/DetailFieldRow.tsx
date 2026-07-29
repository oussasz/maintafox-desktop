import type { ReactNode } from "react";

export interface DetailFieldRowProps {
  label: string;
  value?: string | null;
  mono?: boolean;
  children?: ReactNode;
}

/** Label + value row used across Entity Detail Workspace section cards. */
export function DetailFieldRow({ label, value, mono, children }: DetailFieldRowProps) {
  return (
    <div className="flex items-center justify-between gap-3 py-0.5">
      <span className="text-text-muted">{label}</span>
      {children ?? (
        <span className={mono ? "font-mono text-xs" : "text-right"}>{value ?? "—"}</span>
      )}
    </div>
  );
}
