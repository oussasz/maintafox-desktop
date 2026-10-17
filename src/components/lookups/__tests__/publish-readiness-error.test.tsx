/**
 * The publish panel's error box shows only a failure of its own readiness check, never another
 * reference action's error (they used to share one store field).
 */
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PublishReadinessPanel } from "@/components/lookups/PublishReadinessPanel";
import { useReferenceGovernanceStore } from "@/stores/reference-governance-store";

const i18nMock = vi.hoisted(() => ({ t: (key: string) => key, i18n: { language: "en" } }));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const ref = vi.hoisted(() => ({
  computeRefPublishReadiness: vi.fn(),
  previewRefPublishImpact: vi.fn(),
  validateReferenceSet: vi.fn(),
  governedPublishReferenceSet: vi.fn(),
}));
vi.mock("@/services/reference-service", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...ref,
}));

function renderPanel() {
  return render(
    <PublishReadinessPanel setId={5} setStatus="draft" isProtected={false} onPublished={vi.fn()} />,
  );
}

describe("PublishReadinessPanel error box", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useReferenceGovernanceStore.setState({
      readiness: null,
      readinessError: null,
      error: null,
    });
  });

  it("does not show another action's error", async () => {
    // The readiness on screen belongs to another set (e.g. still the previous one).
    ref.computeRefPublishReadiness.mockResolvedValue({ set_id: 6, issues: [] });
    renderPanel();
    await waitFor(() => expect(ref.computeRefPublishReadiness).toHaveBeenCalledWith(5));
    // A value save failed elsewhere on the screen.
    useReferenceGovernanceStore.setState({ error: "Saving the value failed" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText("Saving the value failed")).not.toBeInTheDocument();
  });

  it("shows its own failure with a retry", async () => {
    ref.computeRefPublishReadiness.mockRejectedValue({ code: "DATABASE_ERROR", message: "busy" });
    renderPanel();
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });
});
