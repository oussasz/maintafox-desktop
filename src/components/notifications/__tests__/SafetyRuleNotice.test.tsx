import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SafetyRuleNotice } from "@/components/notifications/SafetyRuleNotice";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, o?: Record<string, unknown>) => {
      const vals = Object.entries(o ?? {})
        .filter(([k]) => k !== "defaultValue")
        .map(([k, v]) => `${k}=${String(v)}`);
      return vals.length ? `${key}[${vals.join(",")}]` : key;
    },
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({
  getSafetyRuleNotices: vi.fn(),
  acknowledgeSafetyRuleNotice: vi.fn(),
}));
vi.mock("@/services/notification-service", () => svc);

const notice = {
  id: 42,
  created_at: "2026-10-20T09:00:00Z",
  title: "Four-eyes lifted",
  body: null,
  message_key: "site_rule_received_on",
  message_params: { site: "Plant A", changed_by: "Admin", changed_at: "2026-10-20T08:00:00Z" },
};

beforeEach(() => {
  vi.clearAllMocks();
  svc.getSafetyRuleNotices.mockResolvedValue([notice]);
  svc.acknowledgeSafetyRuleNotice.mockResolvedValue(undefined);
});

describe("SafetyRuleNotice", () => {
  it("shows the notice after sign-in until it is acknowledged", async () => {
    render(<SafetyRuleNotice sessionUserId={1} />);
    expect(await screen.findByTestId("safety-rule-notice")).toHaveTextContent(
      "messages.site_rule_received_on.title",
    );
    fireEvent.click(screen.getByRole("button", { name: "safetyNotice.acknowledge" }));
    await waitFor(() => expect(svc.acknowledgeSafetyRuleNotice).toHaveBeenCalledWith(42));
    await waitFor(() => expect(screen.queryByTestId("safety-rule-notice")).toBeNull());
  });

  it("shows nothing signed out", () => {
    render(<SafetyRuleNotice sessionUserId={null} />);
    expect(svc.getSafetyRuleNotices).not.toHaveBeenCalled();
    expect(screen.queryByTestId("safety-rule-notices")).toBeNull();
  });

  it("says when the notices cannot be read", async () => {
    svc.getSafetyRuleNotices.mockRejectedValue(new Error("offline"));
    render(<SafetyRuleNotice sessionUserId={1} />);
    expect(await screen.findByTestId("safety-rule-notices-error")).toHaveTextContent(
      "safetyNotice.loadError",
    );
  });

  it("reads the notices again when one is acknowledged from the inbox", async () => {
    render(<SafetyRuleNotice sessionUserId={1} />);
    await screen.findByTestId("safety-rule-notice");
    svc.getSafetyRuleNotices.mockResolvedValue([]);
    window.dispatchEvent(new Event("maintafox:safety-notice-acknowledged"));
    await waitFor(() => expect(screen.queryByTestId("safety-rule-notice")).toBeNull());
  });

  it("names the notice its button confirms, in one alert region", async () => {
    render(<SafetyRuleNotice sessionUserId={1} />);
    const button = await screen.findByRole("button", { name: "safetyNotice.acknowledge" });
    const described = button.getAttribute("aria-describedby");
    expect(described).toBeTruthy();
    expect(document.getElementById(described ?? "")).toHaveTextContent(
      "messages.site_rule_received_on.title",
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
  });

  it("keeps the notice and says why when the acknowledgement is refused", async () => {
    svc.acknowledgeSafetyRuleNotice.mockRejectedValue(new Error("refused"));
    render(<SafetyRuleNotice sessionUserId={1} />);
    fireEvent.click(await screen.findByRole("button", { name: "safetyNotice.acknowledge" }));
    await waitFor(() => expect(svc.acknowledgeSafetyRuleNotice).toHaveBeenCalled());
    expect(screen.getByTestId("safety-rule-notice")).toBeInTheDocument();
  });
});
