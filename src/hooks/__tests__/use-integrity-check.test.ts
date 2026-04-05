import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";

import { useIntegrityCheck } from "@/hooks/use-integrity-check";
import { mockInvoke, fixtures } from "@/test/mocks/tauri";

describe("useIntegrityCheck", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("starts in idle state with no report", () => {
    const { result } = renderHook(() => useIntegrityCheck());
    expect(result.current.status).toBe("idle");
    expect(result.current.report).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it("check() fetches a healthy report", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.integrityReportHealthy);

    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => {
      await result.current.check();
    });

    expect(result.current.status).toBe("done");
    expect(result.current.report?.is_healthy).toBe(true);
    expect(result.current.report?.domain_count).toBe(18);
    expect(mockInvoke).toHaveBeenCalledWith("run_integrity_check");
  });

  it("check() sets error status on failure", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("IPC failed"));

    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => {
      await result.current.check();
    });

    expect(result.current.status).toBe("error");
    expect(result.current.error).toBe("IPC failed");
    expect(result.current.report).toBeNull();
  });

  it("repair() calls repair_seed_data and returns updated report", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.integrityReportHealthy);

    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => {
      await result.current.repair();
    });

    expect(result.current.status).toBe("done");
    expect(result.current.report?.is_healthy).toBe(true);
    expect(mockInvoke).toHaveBeenCalledWith("repair_seed_data");
  });

  it("repair() sets error status on failure", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("Repair failed"));

    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => {
      await result.current.repair();
    });

    expect(result.current.status).toBe("error");
    expect(result.current.error).toBe("Repair failed");
  });

  it("check() with unhealthy report exposes issues", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.integrityReportUnhealthy);

    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => {
      await result.current.check();
    });

    expect(result.current.status).toBe("done");
    expect(result.current.report?.is_healthy).toBe(false);
    expect(result.current.report?.is_recoverable).toBe(true);
    expect(result.current.report?.issues).toHaveLength(1);
    expect(result.current.report?.issues[0]?.code).toBe("MISSING_DOMAIN");
  });
});
