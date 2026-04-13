import { describe, it, expect, beforeEach } from "vitest";

import { mockInvoke, fixtures } from "@/test/mocks/tauri";

import {
  healthCheck,
  getAppInfo,
  getTaskStatus,
  HealthCheckResponseSchema,
  AppInfoResponseSchema,
  TaskStatusEntrySchema,
} from "../app.service";

// ─────────────────────────────────────────────────────────────────────────────
// healthCheck
// ─────────────────────────────────────────────────────────────────────────────
describe("healthCheck", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("calls the health_check command", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.healthCheck);
    await healthCheck();
    expect(mockInvoke).toHaveBeenCalledWith("health_check");
  });

  it("returns a validated HealthCheckResponse when Rust returns correct shape", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.healthCheck);
    const result = await healthCheck();
    expect(result.status).toBe("ok");
    expect(result.version).toBe("0.1.0");
    expect(result.db_connected).toBe(true);
    expect(result.locale).toBe("fr");
  });

  it("accepts degraded status value", async () => {
    mockInvoke.mockResolvedValueOnce({ ...fixtures.healthCheck, status: "degraded" });
    const result = await healthCheck();
    expect(result.status).toBe("degraded");
  });

  it("throws a ZodError when Rust returns a malformed response", async () => {
    mockInvoke.mockResolvedValueOnce({ status: 123, version: "" });
    await expect(healthCheck()).rejects.toThrow();
  });

  it("propagates invoke rejection (Rust command error)", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("Tauri IPC error"));
    await expect(healthCheck()).rejects.toThrow("Tauri IPC error");
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// getAppInfo
// ─────────────────────────────────────────────────────────────────────────────
describe("getAppInfo", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("calls the get_app_info command", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.appInfo);
    await getAppInfo();
    expect(mockInvoke).toHaveBeenCalledWith("get_app_info");
  });

  it("returns a validated AppInfoResponse", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.appInfo);
    const result = await getAppInfo();
    expect(result.app_name).toBe("Maintafox");
    expect(result.default_locale).toBe("fr");
    expect(result.build_mode).toBe("debug");
  });

  it("rejects an invalid build_mode value", async () => {
    mockInvoke.mockResolvedValueOnce({ ...fixtures.appInfo, build_mode: "staging" });
    await expect(getAppInfo()).rejects.toThrow();
  });

  it("validates default_locale is at least 2 characters", async () => {
    mockInvoke.mockResolvedValueOnce({ ...fixtures.appInfo, default_locale: "x" });
    await expect(getAppInfo()).rejects.toThrow();
  });

  it("propagates invoke rejection", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("IPC failure"));
    await expect(getAppInfo()).rejects.toThrow("IPC failure");
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// getTaskStatus
// ─────────────────────────────────────────────────────────────────────────────
describe("getTaskStatus", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("calls the get_task_status command", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.taskStatus);
    await getTaskStatus();
    expect(mockInvoke).toHaveBeenCalledWith("get_task_status");
  });

  it("returns an empty array when no tasks are registered", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    const result = await getTaskStatus();
    expect(result).toEqual([]);
  });

  it("returns validated entries with correct status values", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.taskStatusWithEntries);
    const result = await getTaskStatus();
    expect(result).toHaveLength(2);
    expect(result.at(0)?.status).toBe("running");
    expect(result.at(1)?.status).toBe("finished");
  });

  it("throws when an entry has an invalid status", async () => {
    mockInvoke.mockResolvedValueOnce([{ id: "t-1", status: "unknown" }]);
    await expect(getTaskStatus()).rejects.toThrow();
  });

  it("propagates invoke rejection", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("IPC error"));
    await expect(getTaskStatus()).rejects.toThrow("IPC error");
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Zod schema standalone tests
// ─────────────────────────────────────────────────────────────────────────────
describe("HealthCheckResponseSchema", () => {
  it("rejects unknown status values", () => {
    const result = HealthCheckResponseSchema.safeParse({
      status: "error",
      version: "1.0.0",
      db_connected: true,
      locale: "fr",
    });
    expect(result.success).toBe(false);
  });

  it("rejects missing db_connected field", () => {
    const result = HealthCheckResponseSchema.safeParse({
      status: "ok",
      version: "1.0.0",
      locale: "fr",
    });
    expect(result.success).toBe(false);
  });
});

describe("AppInfoResponseSchema", () => {
  it("rejects empty app_name", () => {
    const result = AppInfoResponseSchema.safeParse({
      ...fixtures.appInfo,
      app_name: "",
    });
    expect(result.success).toBe(false);
  });

  it("rejects invalid build_mode", () => {
    const result = AppInfoResponseSchema.safeParse({
      ...fixtures.appInfo,
      build_mode: "staging",
    });
    expect(result.success).toBe(false);
  });
});

describe("TaskStatusEntrySchema", () => {
  it("rejects non-string id", () => {
    const result = TaskStatusEntrySchema.safeParse({
      id: 123,
      status: "running",
    });
    expect(result.success).toBe(false);
  });

  it("rejects unknown status kind", () => {
    const result = TaskStatusEntrySchema.safeParse({
      id: "t-1",
      status: "paused",
    });
    expect(result.success).toBe(false);
  });
});
