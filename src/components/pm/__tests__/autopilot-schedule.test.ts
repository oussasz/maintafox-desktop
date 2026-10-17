import { describe, expect, it } from "vitest";

import { cronOfDailyTime, dailyTimeOfCron } from "../PmAutopilotStrip";

describe("autopilot schedule ↔ cron", () => {
  it("reads a daily six-field cron as a time of day", () => {
    expect(dailyTimeOfCron("0 0 0 * * *")).toBe("00:00");
    expect(dailyTimeOfCron("0 30 6 * * *")).toBe("06:30");
  });

  it("keeps any other schedule as custom", () => {
    expect(dailyTimeOfCron("0 0 6 * * 1-5")).toBeNull();
    expect(dailyTimeOfCron("0 */15 * * * *")).toBeNull();
    expect(dailyTimeOfCron("0 0 6 * *")).toBeNull();
  });

  it("writes a daily cron from a time of day", () => {
    expect(cronOfDailyTime("06:30")).toBe("0 30 6 * * *");
    expect(dailyTimeOfCron(cronOfDailyTime("23:05"))).toBe("23:05");
  });
});
