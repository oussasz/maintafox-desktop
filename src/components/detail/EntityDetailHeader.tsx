import type { ReactNode } from "react";

import { Card, CardContent } from "@/components/ui/card";

export interface EntityDetailHeaderProps {
  code: string;
  designation: string;
  statusSlot?: ReactNode;
  qrSlot?: ReactNode;
  primaryActionsSlot?: ReactNode;
  overflowSlot?: ReactNode;
  /** Optional strip below identity (e.g. KPI strip). Prefer EntitySummaryKpiStrip outside when full-width. */
  footerSlot?: ReactNode;
}

const HEADER_ACTION_ROW = "flex flex-wrap items-center gap-2 pt-1";

/**
 * Identity header — CODE, designation, status, QR, primary actions, overflow.
 * Do not place technical specifications here.
 */
export function EntityDetailHeader({
  code,
  designation,
  statusSlot,
  qrSlot,
  primaryActionsSlot,
  overflowSlot,
  footerSlot,
}: EntityDetailHeaderProps) {
  return (
    <Card className="border-surface-border shadow-sm">
      <CardContent className="space-y-3 pt-4 pb-4">
        <div className="grid grid-cols-1 items-start gap-4 sm:grid-cols-[minmax(0,1.85fr)_minmax(0,1fr)]">
          <div className="min-w-0 space-y-1.5">
            <p className="font-mono text-xs text-text-muted">{code}</p>
            <h2 className="text-lg font-semibold leading-tight tracking-tight">{designation}</h2>
            {statusSlot}
            {(primaryActionsSlot || overflowSlot) && (
              <div className={HEADER_ACTION_ROW}>
                {primaryActionsSlot}
                {overflowSlot}
              </div>
            )}
          </div>
          {qrSlot ? (
            <div className="flex justify-start sm:justify-end sm:self-start">{qrSlot}</div>
          ) : null}
        </div>
        {footerSlot}
      </CardContent>
    </Card>
  );
}

export const ENTITY_DETAIL_HEADER_ACTION_BTN = "h-7 px-2.5 text-xs gap-1.5";
