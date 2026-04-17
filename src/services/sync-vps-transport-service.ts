import { invoke } from "@tauri-apps/api/core";
import { z, ZodError } from "zod";

import { controlPlaneApiBase } from "@/services/product-license-service";
import { applySyncBatch, getSyncPushPayload } from "@/services/sync-service";
import type { ApplySyncBatchInput } from "@shared/ipc-types";

const op = z.enum(["create", "update", "delete", "upsert", "archive"]);

const SyncAckInputSchema = z.object({
  idempotency_key: z.string(),
  entity_sync_id: z.string(),
  operation: op,
});

const SyncRejectedItemInputSchema = z.object({
  idempotency_key: z.string(),
  entity_sync_id: z.string(),
  operation: op,
  rejection_code: z.string(),
  rejection_message: z.string(),
});

const SyncInboundItemInputSchema = z.object({
  entity_type: z.string(),
  entity_sync_id: z.string(),
  operation: op,
  row_version: z.number(),
  payload_json: z.string(),
});

/** Response body from `POST /api/v1/sync/exchange` — matches `ApplySyncBatchInput`. */
const VpsSyncExchangeResponseSchema = z.object({
  protocol_version: z.literal("v1"),
  server_batch_id: z.string(),
  checkpoint_token: z.string(),
  acknowledged_items: z.array(SyncAckInputSchema),
  rejected_items: z.array(SyncRejectedItemInputSchema),
  inbound_items: z.array(SyncInboundItemInputSchema),
  policy_metadata_json: z.string().nullable().optional(),
});

function decodeError(scope: string, err: unknown): Error {
  if (err instanceof ZodError) {
    return new Error(`${scope}: ${err.message}`);
  }
  return err instanceof Error ? err : new Error(String(err));
}

/**
 * One control-plane round-trip: push pending outbox via `/api/v1/sync/exchange`, then apply the batch locally.
 * Requires product license activation JWT and an authenticated desktop session (see Tauri command).
 * No-ops when there is no stored activation bearer token (offline onboarding or inactive license).
 */
export async function exchangeControlPlaneSyncRound(): Promise<void> {
  let bearer: string | null = null;
  try {
    bearer = await invoke<string | null>("get_control_plane_activation_bearer_token");
  } catch (e) {
    throw decodeError("get_control_plane_activation_bearer_token", e);
  }
  if (!bearer?.trim()) {
    return;
  }

  const push = await getSyncPushPayload(100);

  const idempotencyKey =
    typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;

  const body = {
    protocol_version: "v1" as const,
    checkpoint_token: push.checkpoint_token,
    idempotency_key: idempotencyKey,
    outbox_batch: push.outbox_batch.map((o) => ({
      idempotency_key: o.idempotency_key,
      entity_type: o.entity_type,
      entity_sync_id: o.entity_sync_id,
      operation: o.operation,
      row_version: o.row_version,
      payload_json: o.payload_json,
      payload_hash: o.payload_hash,
    })),
  };

  const res = await fetch(`${controlPlaneApiBase()}/api/v1/sync/exchange`, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      Authorization: `Bearer ${bearer}`,
      "Idempotency-Key": idempotencyKey,
    },
    body: JSON.stringify(body),
  });

  if (!res.ok) {
    let message = `Sync exchange failed (${res.status})`;
    try {
      const j = (await res.json()) as { error?: string; message?: string };
      message = j.message ?? j.error ?? message;
    } catch {
      // ignore
    }
    throw new Error(message);
  }

  const raw = await res.json();
  const parsed = VpsSyncExchangeResponseSchema.safeParse(raw);
  if (!parsed.success) {
    throw decodeError("sync_exchange_response", parsed.error);
  }

  const batch: ApplySyncBatchInput = {
    protocol_version: parsed.data.protocol_version,
    server_batch_id: parsed.data.server_batch_id,
    checkpoint_token: parsed.data.checkpoint_token,
    acknowledged_items: parsed.data.acknowledged_items,
    rejected_items: parsed.data.rejected_items,
    inbound_items: parsed.data.inbound_items,
    policy_metadata_json: parsed.data.policy_metadata_json ?? null,
  };

  await applySyncBatch(batch);
}
