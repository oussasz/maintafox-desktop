import type { EntityActivityItem } from "@/components/detail/entity-activity-types";
import { formatSiteLabel } from "@/lib/display";
import type {
  InventoryArticle,
  InventoryStateEvent,
  InventoryTransaction,
  StockReservation,
} from "@shared/ipc-types";

function hasCreatedEvent(events: InventoryStateEvent[]): boolean {
  return events.some(
    (e) =>
      e.entity_type === "ARTICLE" && (e.to_status === "CREATED" || e.reason === "article.created"),
  );
}

/** Compose unified Activity items from real IPC sources only. */
export function composeArticleActivity(params: {
  article: InventoryArticle;
  stateEvents: InventoryStateEvent[];
  transactions: InventoryTransaction[];
  reservations: StockReservation[];
}): EntityActivityItem[] {
  const { article, stateEvents, transactions, reservations } = params;
  const items: EntityActivityItem[] = [];

  if (!hasCreatedEvent(stateEvents)) {
    items.push({
      id: `article-created-${article.id}`,
      at: article.created_at,
      kind: "created",
      typeKey: "article.created",
      title: article.article_code,
      description: null,
    });
  }

  for (const ev of stateEvents) {
    if (ev.entity_type !== "ARTICLE") continue;
    const isCreated = ev.to_status === "CREATED" || ev.reason === "article.created";
    items.push({
      id: `state-${ev.id}`,
      at: ev.changed_at,
      kind: isCreated ? "created" : "edited",
      typeKey: ev.reason ?? (isCreated ? "article.created" : "article.updated"),
      title: isCreated ? article.article_code : (ev.note ?? ev.to_status),
      description: isCreated ? (ev.note ?? null) : ev.note,
    });
  }

  for (const tx of transactions) {
    const site = formatSiteLabel(tx.warehouse_code, tx.warehouse_name);
    const loc = formatSiteLabel(tx.location_code, tx.location_name);
    const motifLabel = tx.reason_code;
    const detailParts = [site, loc, tx.source_ref, tx.notes].filter(Boolean);
    items.push({
      id: `tx-${tx.id}`,
      at: tx.performed_at,
      kind: "stock_movement",
      typeKey: `movement.${tx.movement_type}`,
      title: motifLabel
        ? `${tx.movement_type} · ${tx.quantity} · ${motifLabel}`
        : `${tx.movement_type} · ${tx.quantity}`,
      description: detailParts.join(" · ") || null,
    });
  }

  for (const res of reservations) {
    const site = formatSiteLabel(res.warehouse_code, res.warehouse_name);
    const loc = formatSiteLabel(res.location_code, res.location_name);
    items.push({
      id: `res-created-${res.id}`,
      at: res.created_at,
      kind: "reservation_created",
      typeKey: "reservation.created",
      title: `${res.quantity_reserved}`,
      description: [site, loc, res.source_ref, res.status].filter(Boolean).join(" · ") || null,
      statusLabel: res.status,
    });
    if (res.released_at) {
      items.push({
        id: `res-released-${res.id}`,
        at: res.released_at,
        kind: "reservation_released",
        typeKey: "reservation.released",
        title: `${res.quantity_reserved}`,
        description: [site, loc, res.source_ref].filter(Boolean).join(" · ") || null,
        statusLabel: res.status,
      });
    }
  }

  items.sort((a, b) => {
    const ta = new Date(a.at).getTime();
    const tb = new Date(b.at).getTime();
    if (Number.isNaN(ta) && Number.isNaN(tb)) return 0;
    if (Number.isNaN(ta)) return 1;
    if (Number.isNaN(tb)) return -1;
    return tb - ta;
  });

  return items;
}

export function computeArticleKpis(
  balances: {
    on_hand_qty: number;
    reserved_qty: number;
    available_qty: number;
    location_id: number;
  }[],
  transactions: { performed_at: string }[],
): {
  onHand: number;
  reserved: number;
  available: number;
  locations: number;
  movements30d: number;
} {
  const real = balances.filter(
    (b) => b.on_hand_qty !== 0 || b.reserved_qty !== 0 || b.available_qty !== 0,
  );
  const source = real.length > 0 ? real : balances;
  const onHand = source.reduce((s, b) => s + b.on_hand_qty, 0);
  const reserved = source.reduce((s, b) => s + b.reserved_qty, 0);
  const available = source.reduce((s, b) => s + b.available_qty, 0);
  const locations = new Set(source.map((b) => b.location_id)).size;
  const cutoff = Date.now() - 30 * 86400000;
  const movements30d = transactions.filter((t) => {
    const ts = new Date(t.performed_at).getTime();
    return !Number.isNaN(ts) && ts >= cutoff;
  }).length;
  return { onHand, reserved, available, locations, movements30d };
}
