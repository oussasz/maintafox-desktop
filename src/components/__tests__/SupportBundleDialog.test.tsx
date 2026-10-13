import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SupportBundleDialog } from "@/components/SupportBundleDialog";
import { uploadSupportBundle } from "@/services/diagnostics-service";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string>) =>
      vars ? `${key}:${JSON.stringify(vars)}` : key,
  }),
}));

vi.mock("@/services/diagnostics-service", () => ({
  getDiagnosticsInfo: vi.fn(),
  generateSupportBundle: vi.fn(),
  uploadSupportBundle: vi.fn(),
}));

const upload = vi.mocked(uploadSupportBundle);

describe("SupportBundleDialog — send to support", () => {
  beforeEach(() => upload.mockReset());

  it("sends the bundle with the trimmed note and shows the opened ticket", async () => {
    upload.mockResolvedValueOnce({ bundle_id: "bundle_1", status: "uploaded", ticket_id: "sup_9" });
    render(<SupportBundleDialog open onClose={() => undefined} />);
    fireEvent.change(screen.getByLabelText("notePlaceholder"), {
      target: { value: "  Sync fails  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        'sentWithTicket:{"bundle":"bundle_1","ticket":"sup_9"}',
      ),
    );
    expect(upload).toHaveBeenCalledWith("Sync fails");
  });

  it("sends without a note as null and reports a failure", async () => {
    upload.mockRejectedValueOnce(new Error("Support bundle upload failed: offline"));
    render(<SupportBundleDialog open onClose={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("offline"));
    expect(upload).toHaveBeenCalledWith(null);
  });

  it("shows the bundle id when no ticket was requested", async () => {
    upload.mockResolvedValueOnce({ bundle_id: "bundle_2", status: "uploaded", ticket_id: null });
    render(<SupportBundleDialog open onClose={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent('sent:{"bundle":"bundle_2"}'),
    );
  });
});
