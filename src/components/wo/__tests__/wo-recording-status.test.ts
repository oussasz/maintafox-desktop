import { describe, expect, it } from "vitest";

import { isRecordingStatus } from "../wo-recording-status";

describe("isRecordingStatus", () => {
  it("allows task results, tool use and labour in progress and on hold", () => {
    expect(isRecordingStatus("in_progress")).toBe(true);
    expect(isRecordingStatus("on_hold")).toBe(true);
  });

  it("refuses every other status, as the backend does", () => {
    for (const status of [
      "draft",
      "planning",
      "ready",
      "completed",
      "closed",
      "cancelled",
      "",
      null,
      undefined,
    ]) {
      expect(isRecordingStatus(status)).toBe(false);
    }
  });
});
