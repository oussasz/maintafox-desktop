import { render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { NegativeStockSyncNotice } from "@/components/sync/NegativeStockSyncNotice";
import enShell from "@/i18n/en/shell.json";
import frShell from "@/i18n/fr/shell.json";

const mocks = vi.hoisted(() => ({ getNegativeStockSummary: vi.fn() }));
vi.mock("@/services/negative-stock-service", () => mocks);

function renderNotice() {
  return render(
    <MemoryRouter>
      <NegativeStockSyncNotice />
    </MemoryRouter>,
  );
}

describe("NegativeStockSyncNotice", () => {
  beforeAll(async () => {
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["shell"],
        defaultNS: "shell",
        resources: { en: { shell: enShell } },
        interpolation: { escapeValue: false },
      });
    } else {
      i18next.addResourceBundle("en", "shell", enShell, true, true);
    }
  });

  beforeEach(() => mocks.getNegativeStockSummary.mockReset());

  it("points to the stock to reconcile when locations are below zero", async () => {
    mocks.getNegativeStockSummary.mockResolvedValue({
      negative_count: 2,
      double_count_count: 1,
      over_reserved_count: 1,
    });
    renderNotice();
    expect(
      await screen.findByText(/2 stock location\(s\) below zero after sync/),
    ).toBeInTheDocument();
    expect(screen.getByText(/1 stock location\(s\) with more reserved/)).toBeInTheDocument();
    expect(screen.getByText(/1 stock location\(s\) counted on two PCs/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open the stock to reconcile" })).toHaveAttribute(
      "href",
      "/inventory?stock=to_reconcile",
    );
  });

  it("stays hidden when there is nothing to reconcile", async () => {
    mocks.getNegativeStockSummary.mockResolvedValue({
      negative_count: 0,
      double_count_count: 0,
      over_reserved_count: 0,
    });
    const { container } = renderNotice();
    await waitFor(() => expect(mocks.getNegativeStockSummary).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("has its texts in French", () => {
    for (const key of [
      "negativeStock",
      "doubleCount",
      "overReserved",
      "openNegativeStock",
    ] as const) {
      expect(frShell.sync[key].trim()).not.toBe("");
    }
  });
});
