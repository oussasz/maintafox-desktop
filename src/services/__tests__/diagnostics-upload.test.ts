import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { uploadSupportBundle } from "../diagnostics-service";

describe("uploadSupportBundle", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes upload_support_bundle with the note and validates the answer", async () => {
    mockInvoke.mockResolvedValueOnce({
      bundle_id: "bundle_1",
      status: "uploaded",
      ticket_id: "sup_1",
    });
    await expect(uploadSupportBundle("Sync fails")).resolves.toEqual({
      bundle_id: "bundle_1",
      status: "uploaded",
      ticket_id: "sup_1",
    });
    expect(mockInvoke).toHaveBeenCalledWith("upload_support_bundle", { note: "Sync fails" });
  });

  it("rejects an answer that does not follow the contract", async () => {
    mockInvoke.mockResolvedValueOnce({
      bundle_id: "bundle_1",
      status: "pending_upload",
      ticket_id: null,
    });
    await expect(uploadSupportBundle(null)).rejects.toThrow();
  });
});
