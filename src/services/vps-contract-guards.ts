import { z } from "zod";

import type { VpsContractError, VpsRequestContext, VpsRouteContract } from "@shared/ipc-types";

const VpsRouteContractSchema = z.object({
  family: z.enum(["license", "sync", "updates", "admin", "relay"]),
  owner: z.string().min(1),
  route_prefix: z.string().min(1),
  version: z.string().min(1),
  required_boundary: z.enum(["tenant_runtime", "vendor_admin"]),
  tenant_scope: z.enum(["required", "not_allowed"]),
  required_permissions: z.array(z.string()),
  idempotency_required: z.boolean(),
  replay_guard_required: z.boolean(),
});

const VpsRequestContextSchema = z.object({
  correlation_id: z.string().min(1),
  api_version: z.string().min(1),
  auth_boundary: z.enum(["tenant_runtime", "vendor_admin"]),
  actor_id: z.string().min(1),
  tenant_id: z.string().nullable(),
  token_tenant_id: z.string().nullable(),
  permissions: z.array(z.string()),
  idempotency_key: z.string().nullable(),
  request_nonce: z.string().nullable(),
  checkpoint_token: z.string().nullable(),
});

function contractError(
  route: VpsRouteContract,
  code: string,
  message: string,
  httpStatus: number,
  retryable = false,
): Error {
  const detail: VpsContractError = {
    family: route.family,
    code,
    message,
    http_status: httpStatus,
    retryable,
  };
  const err = new Error(`${code}: ${message}`);
  Object.assign(err, { contractError: detail });
  return err;
}

export function assertVpsRequestReadiness(routeInput: VpsRouteContract, ctxInput: VpsRequestContext): void {
  const route = VpsRouteContractSchema.parse(routeInput) as VpsRouteContract;
  const ctx = VpsRequestContextSchema.parse(ctxInput) as VpsRequestContext;

  if (ctx.api_version !== route.version) {
    throw contractError(route, "unsupported_api_version", "API version is not compatible.", 426);
  }
  if (ctx.auth_boundary !== route.required_boundary) {
    throw contractError(route, "auth_boundary_violation", "Auth boundary mismatch for route family.", 403);
  }
  if (route.tenant_scope === "required") {
    if (!ctx.tenant_id || !ctx.token_tenant_id) {
      throw contractError(route, "tenant_context_required", "Tenant context is required.", 403);
    }
    if (ctx.tenant_id !== ctx.token_tenant_id) {
      throw contractError(route, "tenant_isolation_violation", "Token tenant mismatch.", 403);
    }
  } else if (ctx.tenant_id || ctx.token_tenant_id) {
    throw contractError(route, "tenant_context_forbidden", "Vendor admin boundary cannot carry tenant scope.", 403);
  }

  for (const requiredPermission of route.required_permissions) {
    if (!ctx.permissions.includes(requiredPermission)) {
      throw contractError(route, "permission_scope_missing", "Required permission scope missing.", 403);
    }
  }
  if (route.idempotency_required && !ctx.idempotency_key) {
    throw contractError(route, "idempotency_key_required", "Mutation endpoint requires idempotency key.", 400);
  }
  if (route.replay_guard_required && !ctx.request_nonce && !ctx.checkpoint_token) {
    throw contractError(route, "replay_guard_required", "Replay guard metadata required.", 400);
  }
}
