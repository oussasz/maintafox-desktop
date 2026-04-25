import { describe, expect, it } from "vitest";

import type { VpsRequestContext, VpsRouteContract } from "@shared/ipc-types";
import { assertVpsRequestReadiness } from "@/services/vps-contract-guards";

const syncRoute: VpsRouteContract = {
  family: "sync",
  owner: "control-plane-sync",
  route_prefix: "/api/v1/sync",
  version: "v1",
  required_boundary: "tenant_runtime",
  tenant_scope: "required",
  required_permissions: ["sync.runtime"],
  idempotency_required: true,
  replay_guard_required: true,
};

const adminRoute: VpsRouteContract = {
  family: "admin",
  owner: "vendor-admin-console",
  route_prefix: "/admin/v1",
  version: "v1",
  required_boundary: "vendor_admin",
  tenant_scope: "not_allowed",
  required_permissions: ["platform.health"],
  idempotency_required: true,
  replay_guard_required: false,
};

function baseRuntimeCtx(): VpsRequestContext {
  return {
    correlation_id: "corr-001",
    api_version: "v1",
    auth_boundary: "tenant_runtime",
    actor_id: "machine-1",
    tenant_id: "tenant-a",
    token_tenant_id: "tenant-a",
    permissions: ["sync.runtime"],
    idempotency_key: "idk-001",
    request_nonce: "nonce-001",
    checkpoint_token: null,
  };
}

describe("vps contract guards", () => {
  it("enforces tenant isolation deny-by-default", () => {
    const ctx = baseRuntimeCtx();
    ctx.tenant_id = null;
    expect(() => assertVpsRequestReadiness(syncRoute, ctx)).toThrow(/tenant_context_required/);
  });

  it("blocks cross-tenant runtime mismatch", () => {
    const ctx = baseRuntimeCtx();
    ctx.token_tenant_id = "tenant-b";
    expect(() => assertVpsRequestReadiness(syncRoute, ctx)).toThrow(/tenant_isolation_violation/);
  });

  it("enforces split auth boundary for admin routes", () => {
    const ctx = baseRuntimeCtx();
    ctx.auth_boundary = "vendor_admin";
    ctx.permissions = ["platform.health"];
    expect(() => assertVpsRequestReadiness(syncRoute, ctx)).toThrow(/auth_boundary_violation/);
  });

  it("blocks tenant context on vendor admin boundary", () => {
    const ctx = baseRuntimeCtx();
    ctx.auth_boundary = "vendor_admin";
    ctx.permissions = ["platform.health"];
    expect(() => assertVpsRequestReadiness(adminRoute, ctx)).toThrow(/tenant_context_forbidden/);
  });

  it("rejects incompatible route versions", () => {
    const ctx = baseRuntimeCtx();
    ctx.api_version = "v2";
    expect(() => assertVpsRequestReadiness(syncRoute, ctx)).toThrow(/unsupported_api_version/);
  });

  it("accepts valid runtime context with replay-safe fields", () => {
    const ctx = baseRuntimeCtx();
    expect(() => assertVpsRequestReadiness(syncRoute, ctx)).not.toThrow();
  });

  it("does not bypass tenant isolation when degraded path uses checkpoint token", () => {
    const ctx = baseRuntimeCtx();
    ctx.tenant_id = "tenant-a";
    ctx.token_tenant_id = "tenant-b";
    ctx.request_nonce = null;
    ctx.checkpoint_token = "ckpt-degraded-offline-replay";
    expect(() => assertVpsRequestReadiness(syncRoute, ctx)).toThrow(/tenant_isolation_violation/);
  });
});
