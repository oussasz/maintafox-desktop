import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { FmecaRatingScalesHost } from "@/components/lookups/adapters/FmecaRatingScalesHost";
import type { RatingScales } from "@/services/reliability-model-service";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

const perms = vi.hoisted(() => ({ allowed: true }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: () => perms.allowed,
    canAny: () => perms.allowed,
    isLoading: false,
  }),
}));

const svc = vi.hoisted(() => ({
  getReliabilityRatingScales: vi.fn(),
  saveReliabilityRatingScales: vi.fn(),
}));
vi.mock("@/services/reliability-model-service", () => svc);

const grid = (prefix: string) =>
  Array.from({ length: 10 }, (_, i) => ({
    score: i + 1,
    label_fr: `${prefix} ${i + 1}`,
    label_en: `${prefix} EN ${i + 1}`,
  }));
const SCALES: RatingScales = {
  version: 1,
  severity: grid("Gravité"),
  occurrence: grid("Occurrence"),
  detection: grid("Détection"),
  matrix: { classes: 5, zones: [{ min_product: 0, zone: "acceptable" }] },
};

beforeEach(() => {
  vi.clearAllMocks();
  perms.allowed = true;
  svc.getReliabilityRatingScales.mockResolvedValue(SCALES);
});

describe("FmecaRatingScalesHost", () => {
  it("lists the plant's grids and opens the editor for the reliability manage right", async () => {
    render(<FmecaRatingScalesHost />);
    expect(await screen.findByText("Gravité 10")).toBeInTheDocument();
    expect(screen.getByText("rams.scales.whoEdits")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "rams.scales.edit" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });

  it("is read only without the right", async () => {
    perms.allowed = false;
    render(<FmecaRatingScalesHost />);
    expect(await screen.findByText("rams.scales.readOnly")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "rams.scales.view" })).toBeInTheDocument();
  });

  it("shows a failed load with a retry instead of product grids", async () => {
    svc.getReliabilityRatingScales.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: "RATING_SCALE_MATRIX",
    });
    render(<FmecaRatingScalesHost />);
    expect(await screen.findByText("rams.scales.loadError")).toBeInTheDocument();
    expect(screen.queryByText("Gravité 10")).not.toBeInTheDocument();
  });
});
