import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { installPendingUpdate } from "@/services/updater-service";
import { useLocaleStore } from "@/stores/locale-store";
import { useUpdaterStore } from "@/stores/updater-store";
import type { UpdateCheckResult } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { UpdateBanner } from "../UpdateBanner";

vi.mock("@/services/updater-service", () => ({
  checkForUpdate: vi.fn(),
  installPendingUpdate: vi.fn(),
  reinstallPreviousVersion: vi.fn(),
  getUpdateJournal: vi.fn(),
  acknowledgeUpdateNotice: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/services/product-license-service", () => ({
  getProductLicenseOnboardingState: vi.fn(),
}));

const mockInstall = vi.mocked(installPendingUpdate);

function makeCheck(overrides: Partial<UpdateCheckResult> = {}): UpdateCheckResult {
  return {
    status: "available",
    available: true,
    current_version: "1.1.0",
    version: "1.2.0",
    notes: "Generic notes",
    pub_date: null,
    kind: "upgrade",
    mandatory: false,
    mandatory_reason: null,
    mandatory_message: null,
    notes_en: "Faster work order search.",
    notes_fr: "Recherche des OT plus rapide.",
    message: null,
    ...overrides,
  };
}

const initialState = useUpdaterStore.getState();

beforeAll(async () => {
  initI18n();
  await i18n.changeLanguage("en");
});

beforeEach(() => {
  vi.clearAllMocks();
  useUpdaterStore.setState(initialState, true);
  useLocaleStore.setState({ activeLocale: "en" });
});

describe("UpdateBanner", () => {
  it("renders nothing without an offer, outcome or notice", () => {
    const { container } = render(<UpdateBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows an optional update with notes in the UI language and a dismiss button", () => {
    useUpdaterStore.setState({ lastCheckResult: makeCheck() });

    render(<UpdateBanner />);

    expect(screen.getByText("Version 1.2.0 is available")).toBeInTheDocument();
    expect(screen.getByText("Faster work order search.")).toBeInTheDocument();
    expect(screen.queryByText("Recherche des OT plus rapide.")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(screen.queryByText("Version 1.2.0 is available")).not.toBeInTheDocument();
  });

  it("uses the rollback wording for a withdrawn version", () => {
    useUpdaterStore.setState({
      lastCheckResult: makeCheck({ kind: "rollback", version: "1.0.4" }),
    });

    render(<UpdateBanner />);

    expect(
      screen.getByText(
        "This version was withdrawn by Maintafox. Version 1.0.4 (previous) will be reinstalled.",
      ),
    ).toBeInTheDocument();
  });

  it("shows a mandatory update with its reason and no dismiss button", () => {
    useUpdaterStore.setState({
      lastCheckResult: makeCheck({
        mandatory: true,
        mandatory_reason: "forced_update_floor",
        mandatory_message: "<b>Security fix</b>",
      }),
    });

    render(<UpdateBanner />);

    expect(screen.getByText("Mandatory")).toBeInTheDocument();
    expect(
      screen.getByText("Your organisation requires this version or a newer one."),
    ).toBeInTheDocument();
    // The operator's text is plain text, never HTML; the raw code is never shown.
    expect(screen.getByText("<b>Security fix</b>")).toBeInTheDocument();
    expect(screen.queryByText(/forced_update_floor/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Later" })).not.toBeInTheDocument();
  });

  it("asks for confirmation, then installs with the UI locale", async () => {
    mockInstall.mockReturnValueOnce(new Promise(() => undefined));
    useUpdaterStore.setState({ lastCheckResult: makeCheck() });

    render(<UpdateBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Install now" }));

    expect(await screen.findByText("A backup of your data is made first.")).toBeInTheDocument();
    expect(
      screen.getByText("Unsaved work is lost: save or finish what you are doing first."),
    ).toBeInTheDocument();
    expect(mockInstall).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Install and restart" }));

    await waitFor(() => expect(mockInstall).toHaveBeenCalledWith("en", expect.any(Function)));
    expect(await screen.findByText("Preparing the update…")).toBeInTheDocument();
  });

  it("shows real download progress", () => {
    useUpdaterStore.setState({
      lastCheckResult: makeCheck(),
      phase: "downloading",
      downloadedBytes: 5 * 1024 * 1024,
      totalBytes: 20 * 1024 * 1024,
    });

    render(<UpdateBanner />);

    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "25");
    expect(screen.getByText("25% · 5.0 MB of 20.0 MB")).toBeInTheDocument();
  });

  it("shows megabytes when the total size is unknown", () => {
    useUpdaterStore.setState({
      phase: "downloading",
      downloadedBytes: 3 * 1024 * 1024,
      totalBytes: null,
    });

    render(<UpdateBanner />);

    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.getByText("3.0 MB downloaded")).toBeInTheDocument();
  });

  it("explains refusals in plain language", () => {
    useUpdaterStore.setState({ refusal: { code: "rollback_unsynced_changes", count: 3 } });
    const { unmount } = render(<UpdateBanner />);
    expect(
      screen.getByText(
        "3 changes are not synchronised yet. Synchronise first, then try again; otherwise they would be lost.",
      ),
    ).toBeInTheDocument();
    unmount();

    useUpdaterStore.setState({
      refusal: { code: "rollback_backup_missing" },
      installTargetVersion: "1.0.4",
    });
    render(<UpdateBanner />);
    expect(
      screen.getByText(
        "The backup needed to go back to version 1.0.4 is not on this computer. Contact Maintafox support.",
      ),
    ).toBeInTheDocument();
  });

  it("shows an install error with the technical detail on a secondary line", () => {
    useUpdaterStore.setState({
      lastCheckResult: makeCheck(),
      error: "download or signature check failed: bad signature",
    });

    render(<UpdateBanner />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "The update could not be installed. Nothing was changed",
    );
    expect(
      screen.getByText("Technical detail: download or signature check failed: bad signature"),
    ).toBeInTheDocument();
  });

  it("shows the rolled-back notice once and acknowledges it", async () => {
    useUpdaterStore.setState({
      journal: {
        state: "rolled_back",
        kind: "rollback",
        from_version: "1.1.0",
        to_version: "1.2.0",
        failure: "Migration failed",
        rollback_refusal: null,
        rollback_allowed_version: null,
        notice_acknowledged: false,
      },
    });

    render(<UpdateBanner />);

    expect(
      screen.getByText(/The update to version 1.2.0 could not update your data/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Understood" }));
    await waitFor(() =>
      expect(screen.queryByText(/could not update your data/)).not.toBeInTheDocument(),
    );
  });

  it("translates every mandatory reason code, with a generic text for an unknown one", () => {
    for (const [code, text] of [
      ["emergency", "Maintafox requires this update urgently."],
      [
        "rollback_requested",
        "This computer goes back to the version it ran before the failed update.",
      ],
      ["recalled_version", "The version installed here was withdrawn by Maintafox."],
      ["a_future_code", "This update is required to keep using Maintafox."],
    ] as const) {
      useUpdaterStore.setState({
        lastCheckResult: makeCheck({ mandatory: true, mandatory_reason: code }),
      });
      const { unmount } = render(<UpdateBanner />);
      expect(screen.getByText(text)).toBeInTheDocument();
      expect(screen.queryByText(new RegExp(code))).not.toBeInTheDocument();
      unmount();
    }
  });

  it("explains that only an administrator installs, or that one must sign in", () => {
    useUpdaterStore.setState({
      lastCheckResult: makeCheck(),
      error: `Permission requise : ${P.ADM_SETTINGS}`,
      errorCode: "PERMISSION_DENIED",
    });
    const { unmount } = render(<UpdateBanner />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Only an administrator of this computer can install updates.",
    );
    unmount();

    useUpdaterStore.setState({ error: "Session expirée", errorCode: "AUTH_ERROR" });
    render(<UpdateBanner />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Sign in as an administrator of this computer to install the update.",
    );
  });

  it("says a required update is not published yet when the licence forces one and none is offered", () => {
    useUpdaterStore.setState({
      lastCheckResult: makeCheck({ status: "up_to_date", available: false, version: null }),
      forceRequired: true,
    });
    render(<UpdateBanner />);
    expect(screen.getByText(/none is published for this computer yet/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install now" })).not.toBeInTheDocument();
  });
});
