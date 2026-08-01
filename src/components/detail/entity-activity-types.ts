/** Normalized activity item for EntityActivityTimeline (data-agnostic). */
export type EntityActivityKind =
  | "created"
  | "edited"
  | "stock_movement"
  | "reservation_created"
  | "reservation_released"
  | "status_change"
  | "other";

export interface EntityActivityItem {
  id: string;
  at: string;
  kind: EntityActivityKind;
  /** Stable key for i18n / badge, e.g. ADJUST, CREATED, field.safety_stock */
  typeKey: string;
  title: string;
  description?: string | null;
  statusLabel?: string | null;
}
