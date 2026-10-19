/** Badge / dot colours for `personnel.availability_status` (PRD §6.6), on the design-system tones. */

const BADGE: Record<string, string> = {
  available: "border border-tone-success-border bg-tone-success-bg text-tone-success-fg",
  assigned: "border border-tone-info-border bg-tone-info-bg text-tone-info-fg",
  in_training: "border border-tone-warning-border bg-tone-warning-bg text-tone-warning-fg",
  on_leave: "border border-tone-neutral-border bg-tone-neutral-bg text-tone-neutral-fg",
  blocked: "border border-tone-danger-border bg-tone-danger-bg text-tone-danger-fg",
  inactive: "border border-tone-neutral-border bg-tone-neutral-bg text-tone-neutral-fg",
};

const DOT: Record<string, string> = {
  available: "bg-tone-success-fg",
  assigned: "bg-tone-info-fg",
  in_training: "bg-tone-warning-fg",
  on_leave: "bg-tone-neutral-fg",
  blocked: "bg-tone-danger-fg",
  inactive: "bg-tone-neutral-fg",
};

export function personnelAvailabilityBadgeClass(status: string): string {
  return BADGE[status] ?? "border border-border bg-muted text-muted-foreground";
}

export function personnelAvailabilityDotClass(status: string): string {
  return DOT[status] ?? "bg-muted-foreground";
}
