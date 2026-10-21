import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { getUpdateJournal, reinstallPreviousVersion } from "@/services/updater-service";
import { useLocaleStore } from "@/stores/locale-store";
import { useUpdaterStore } from "@/stores/updater-store";

import { UpdateRecoveryScreen } from "../UpdateRecoveryScreen";

vi.mock("@/services/updater-service", () => ({
  checkForUpdate: vi.fn(),
  installPendingUpdate: vi.fn(),
  reinstallPreviousVersion: vi.fn(),
  getUpdateJournal: vi.fn(),
  acknowledgeUpdateNotice: vi.fn(),
}));

vi.mock("@/services/product-license-service", () => ({
  getProductLicenseOnboardingState: vi.fn(),
}));

const mockReinstall = vi.mocked(reinstallPreviousVersion);
const initialState = useUpdaterStore.getState();

function renderScreen() {
  return render(
    <UpdateRecoveryScreen
      fromVersion="1.1.0"
      toVersion="1.2.0"
      reason="Migration failed: duplicate column"
    />,
  );
}

beforeAll(async () => {
  initI18n();
  await i18n.changeLanguage("en");
});

beforeEach(() => {
  vi.clearAllMocks();
  useUpdaterStore.setState(initialState, true);
  useLocaleStore.setState({ activeLocale: "en" });
});

describe("UpdateRecoveryScreen", () => {
  it("explains the situation and keeps the technical reason collapsed", () => {
    renderScreen();

    expect(screen.getByText("The update could not be completed")).toBeInTheDocument();
    expect(screen.getByText(/Your data is safe/)).toBeInTheDocument();
    const reason = screen.getByText("Migration failed: duplicate column");
    expect(reason.closest("details")).not.toHaveAttribute("open");
  });

  it("reinstalls the previous version and shows progress", async () => {
    mockReinstall.mockImplementationOnce((_locale, onProgress) => {
      onProgress({ event: "downloading", downloaded_bytes: 1024 * 1024, total_bytes: null });
      return new Promise(() => undefined);
    });

    renderScreen();
    fireEvent.click(screen.getByRole("button", { name: "Reinstall version 1.1.0" }));

    expect(mockReinstall).toHaveBeenCalledWith("en", expect.any(Function));
    expect(await screen.findByText("1.0 MB downloaded")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Reinstall version 1.1.0" }),
    ).not.toBeInTheDocument();
  });

  it("says plainly when the previous version is not offered", async () => {
    mockReinstall.mockResolvedValueOnce({
      outcome: "refused",
      refusal: { code: "update_withdrawn" },
    });

    renderScreen();
    fireEvent.click(screen.getByRole("button", { name: "Reinstall version 1.1.0" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The previous version cannot be downloaded right now.",
    );
    // The user can try again.
    expect(screen.getByRole("button", { name: "Reinstall version 1.1.0" })).toBeEnabled();
  });

  it("tells the user to keep the PC as it is and contact the vendor when the rollback is refused", async () => {
    mockReinstall.mockResolvedValueOnce({
      outcome: "refused",
      refusal: { code: "rollback_refused" },
    });
    vi.mocked(getUpdateJournal).mockResolvedValueOnce({
      state: "rollback_required",
      kind: "upgrade",
      from_version: "1.1.0",
      to_version: "1.2.0",
      failure: "Migration failed",
      rollback_refusal: "the server refused the rollback request (rollback_target_not_allowed)",
      rollback_allowed_version: "1.0.9",
      backup_refusal: null,
      notice_acknowledged: false,
    });

    renderScreen();
    fireEvent.click(screen.getByRole("button", { name: "Reinstall version 1.1.0" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The update server refused to reinstall version 1.1.0.");
    expect(alert).toHaveTextContent("Keep this computer as it is");
    expect(alert).toHaveTextContent("Contact Maintafox support.");
    // The version the server would allow is information only: shown, never installed.
    expect(await screen.findByText(/names version 1.0.9 as allowed instead/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /1\.0\.9/ })).not.toBeInTheDocument();
    // Exactly one request: no other version is tried.
    expect(mockReinstall).toHaveBeenCalledTimes(1);
  });

  it("shows a failure with its technical detail", async () => {
    mockReinstall.mockRejectedValueOnce(new Error("download or signature check failed"));

    renderScreen();
    fireEvent.click(screen.getByRole("button", { name: "Reinstall version 1.1.0" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Version 1.1.0 could not be reinstalled.",
    );
    expect(
      screen.getByText("Technical detail: download or signature check failed"),
    ).toBeInTheDocument();
  });

  it("offers no reinstall when the backup taken before the update could not be verified", () => {
    render(
      <UpdateRecoveryScreen
        fromVersion="1.1.0"
        toVersion="1.2.0"
        reason="Migration failed: duplicate column"
        backupRefusal="the backup changed since it was taken (checksum mismatch)"
      />,
    );

    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByText(/Your data is safe/)).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Keep this computer as it is: do not reinstall, reset or replace Maintafox, and do not reinstall version 1.1.0",
    );
    expect(screen.getByText(/could not be verified, so it was not put back/)).toBeInTheDocument();
    expect(
      screen.getByText("the backup changed since it was taken (checksum mismatch)"),
    ).toBeInTheDocument();
    expect(mockReinstall).not.toHaveBeenCalled();
  });
});
