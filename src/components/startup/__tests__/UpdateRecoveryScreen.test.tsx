import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { reinstallPreviousVersion } from "@/services/updater-service";
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
});
