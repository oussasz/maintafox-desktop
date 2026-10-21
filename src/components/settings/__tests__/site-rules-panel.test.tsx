/**
 * Rules per site: a site's four-eyes opt-out changes only with a reason, under step-up; a site
 * without a rule shows the default; failures to load are shown with a retry.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SELF_APPROVAL_SITE_RULE, SiteRulesPanel } from "@/components/settings/SiteRulesPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const svc = vi.hoisted(() => ({
  listSiteSettings: vi.fn(),
  listSiteSettingEvents: vi.fn(),
  setSiteSetting: vi.fn(),
}));
vi.mock("@/services/settings-service", () => svc);

const stepUp = vi.hoisted(() => ({ calls: 0 }));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({
    withStepUp: <T,>(action: () => Promise<T>) => {
      stepUp.calls += 1;
      return action();
    },
    StepUpDialogElement: null,
  }),
}));

// Radix Select is replaced by a native one so the test picks a site directly.
vi.mock("@/components/ui/select", () => ({
  Select: ({
    value,
    onValueChange,
    children,
  }: {
    value: string;
    onValueChange: (v: string) => void;
    children: React.ReactNode;
  }) => (
    <select
      aria-label="siteRules.site"
      value={value}
      onChange={(e) => onValueChange(e.target.value)}
    >
      <option value="" />
      {children}
    </select>
  ),
  SelectTrigger: () => null,
  SelectValue: () => null,
  SelectContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  SelectItem: ({ value, children }: { value: string; children: React.ReactNode }) => (
    <option value={value}>{children}</option>
  ),
}));

const SITES = [
  {
    site_node_id: 7,
    site_code: "ORAN",
    site_name: "Usine d'Oran",
    value: null,
    value_unreadable: false,
    default_value: false,
    updated_by_name: null,
    updated_at: null,
    row_version: null,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  stepUp.calls = 0;
  svc.listSiteSettings.mockResolvedValue(SITES);
  svc.listSiteSettingEvents.mockResolvedValue([]);
  svc.setSiteSetting.mockResolvedValue(undefined);
});

describe("SiteRulesPanel", () => {
  it("lifts four-eyes for one site only with a reason, under step-up", async () => {
    const onToast = vi.fn();
    render(<SiteRulesPanel onToast={onToast} />);
    fireEvent.change(await screen.findByLabelText("siteRules.site"), { target: { value: "7" } });
    expect(await screen.findByText("siteRules.notSet")).toBeInTheDocument();
    expect(svc.listSiteSettingEvents).toHaveBeenCalledWith(7, SELF_APPROVAL_SITE_RULE);

    const toggle = screen.getByRole("switch");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    const ask = await screen.findByRole("dialog", { name: "siteRules.confirmTitle" });
    expect(within(ask).getByText("siteRules.confirmOnBody")).toBeInTheDocument();
    const confirm = within(ask).getByRole("button", { name: "siteRules.confirm" });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(ask).getByRole("textbox"), { target: { value: "too short" } });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(ask).getByRole("textbox"), {
      target: { value: "  One reliability engineer on this site  " },
    });
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(svc.setSiteSetting).toHaveBeenCalledWith({
        site_node_id: 7,
        key: SELF_APPROVAL_SITE_RULE,
        value: true,
        reason: "One reliability engineer on this site",
        expected_row_version: null,
      }),
    );
    expect(stepUp.calls).toBe(1);
    await waitFor(() =>
      expect(onToast).toHaveBeenCalledWith({ title: "siteRules.saved", variant: "success" }),
    );
  });

  it("keeps the dialog open when the change is refused", async () => {
    svc.setSiteSetting.mockRejectedValueOnce({ code: "VALIDATION_FAILED", message: "stale" });
    render(<SiteRulesPanel onToast={() => undefined} />);
    fireEvent.change(await screen.findByLabelText("siteRules.site"), { target: { value: "7" } });
    fireEvent.click(await screen.findByRole("switch"));
    const ask = await screen.findByRole("dialog", { name: "siteRules.confirmTitle" });
    fireEvent.change(within(ask).getByRole("textbox"), {
      target: { value: "One reliability engineer on this site" },
    });
    fireEvent.click(within(ask).getByRole("button", { name: "siteRules.confirm" }));
    await waitFor(() => expect(svc.setSiteSetting).toHaveBeenCalled());
    expect(screen.getByRole("dialog", { name: "siteRules.confirmTitle" })).toBeInTheDocument();
  });

  it("after a refusal, reads the site again so a retry carries the current version", async () => {
    const changedElsewhere = {
      ...SITES[0],
      value: false,
      updated_by_name: "Nadia",
      updated_at: "2026-10-18T09:00:00Z",
      row_version: 4,
    };
    svc.listSiteSettings.mockResolvedValueOnce(SITES).mockResolvedValue([changedElsewhere]);
    svc.setSiteSetting.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: "SITE_SETTING_STALE",
    });
    render(<SiteRulesPanel onToast={() => undefined} />);
    fireEvent.change(await screen.findByLabelText("siteRules.site"), { target: { value: "7" } });
    fireEvent.click(await screen.findByRole("switch"));
    const ask = await screen.findByRole("dialog", { name: "siteRules.confirmTitle" });
    fireEvent.change(within(ask).getByRole("textbox"), {
      target: { value: "One reliability engineer on this site" },
    });
    fireEvent.click(within(ask).getByRole("button", { name: "siteRules.confirm" }));

    await waitFor(() => expect(svc.listSiteSettings).toHaveBeenCalledTimes(2));
    expect(svc.listSiteSettingEvents).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("dialog", { name: "siteRules.confirmTitle" })).toBeInTheDocument();

    const retry = within(screen.getByRole("dialog", { name: "siteRules.confirmTitle" }));
    await waitFor(() =>
      expect(retry.getByRole("button", { name: "siteRules.confirm" })).toBeEnabled(),
    );
    fireEvent.click(retry.getByRole("button", { name: "siteRules.confirm" }));
    await waitFor(() => expect(svc.setSiteSetting).toHaveBeenCalledTimes(2));
    expect(svc.setSiteSetting).toHaveBeenLastCalledWith(
      expect.objectContaining({ site_node_id: 7, value: true, expected_row_version: 4 }),
    );
  });

  it("says when the stored rule cannot be read (the default applies)", async () => {
    svc.listSiteSettings.mockResolvedValueOnce([{ ...SITES[0], value_unreadable: true }]);
    render(<SiteRulesPanel onToast={() => undefined} />);
    fireEvent.change(await screen.findByLabelText("siteRules.site"), { target: { value: "7" } });
    expect(await screen.findByText("siteRules.unreadable")).toBeInTheDocument();
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  });

  it("shows a failed load with a retry", async () => {
    svc.listSiteSettings.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "busy" });
    render(<SiteRulesPanel onToast={() => undefined} />);
    expect(await screen.findByText("siteRules.loadError")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });
});
