import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { invoke } from "@/lib/ipc-invoke";
import {
  registerStepUpRequester,
  STEP_UP_CANCELLED_MESSAGE,
  trackLocalStepUp,
} from "@/lib/step-up-bridge";
import { mockInvoke } from "@/test/mocks/tauri";

const REFUSED = { code: "STEP_UP_REQUIRED", message: "Step-up required" };

describe("ipc-invoke step-up fallback", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });
  afterEach(() => registerStepUpRequester(null));

  it("asks for the password once, then retries the command", async () => {
    let asked = 0;
    registerStepUpRequester(async () => {
      asked += 1;
      return true;
    });
    mockInvoke.mockRejectedValueOnce(REFUSED).mockResolvedValueOnce("done");

    await expect(invoke("cancel_work_order")).resolves.toBe("done");
    expect(asked).toBe(1);
    expect(mockInvoke).toHaveBeenCalledTimes(2);
  });

  it("rejects with the cancelled marker when the dialog is closed", async () => {
    registerStepUpRequester(async () => false);
    mockInvoke.mockRejectedValueOnce(REFUSED);

    await expect(invoke("cancel_work_order")).rejects.toThrow(STEP_UP_CANCELLED_MESSAGE);
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("leaves the refusal to a caller that wraps the command in withStepUp", async () => {
    let asked = 0;
    registerStepUpRequester(async () => {
      asked += 1;
      return true;
    });
    mockInvoke.mockRejectedValueOnce(REFUSED);

    await expect(trackLocalStepUp(() => invoke("cancel_work_order"))).rejects.toEqual(REFUSED);
    expect(asked).toBe(0);
  });

  it("passes the refusal through when no dialog host is mounted", async () => {
    mockInvoke.mockRejectedValueOnce(REFUSED);
    await expect(invoke("cancel_work_order")).rejects.toEqual(REFUSED);
  });
});
