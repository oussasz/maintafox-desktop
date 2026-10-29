/**
 * The "Téléphones" panel: the plant's phones, the revoke confirmation (with "lost" passed through)
 * and the QR enrolment dialog (code shown, status followed until the phone used it).
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MobilePhonesPanel } from "@/components/admin/MobilePhonesPanel";
import type {
  EnrolmentCandidates,
  IssuedMobileEnrolment,
  MobilePhonesView,
} from "@/services/mobile-enrolment-service";

// Keys with their parameters, so the tests read what is said and for whom.
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key} ${JSON.stringify(opts)}` : key,
  }),
}));

vi.mock("@/hooks/use-formatters", () => ({
  useFormatters: () => ({ formatDateTime: (v: string) => `dt:${v}` }),
}));

vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({
    withStepUp: <T,>(action: () => Promise<T>) => action(),
    StepUpDialogElement: null,
  }),
  isStepUpCancelled: () => false,
}));

vi.mock("@/store/app-toast-store", () => ({ pushAppToast: vi.fn() }));

const svc = vi.hoisted(() => ({
  listMobilePhones: vi.fn(),
  listMobileEnrolmentCandidates: vi.fn(),
  issueMobileEnrolment: vi.fn(),
  getMobileEnrolmentStatus: vi.fn(),
  cancelMobileEnrolment: vi.fn(),
  revokeMobilePhone: vi.fn(),
}));
vi.mock("@/services/mobile-enrolment-service", () => svc);

const view: MobilePhonesView = {
  max_mobile_devices: 5,
  active_mobile_devices: 2,
  open_enrolments: 0,
  devices: [
    {
      device_id: "act_1",
      person_sync_id: "p1",
      person_label: "Karim B.",
      site_label: "Usine Nord",
      enrolled_by_label: "Admin",
      enrolled_at: "2026-10-01T08:00:00Z",
      last_sync_at: null,
      app_version: "0.9.3",
      status: "active",
      revoked_at: null,
      revoked_by_label: null,
    },
    {
      device_id: "act_2",
      person_sync_id: "p2",
      person_label: "Samir T.",
      site_label: "Usine Sud",
      enrolled_by_label: "Admin",
      enrolled_at: "2026-10-02T08:00:00Z",
      last_sync_at: "2026-10-03T09:00:00Z",
      app_version: "0.9.3",
      status: "wipe_pending",
      revoked_at: "2026-10-04T09:00:00Z",
      revoked_by_label: "Admin",
    },
  ],
  pending_revocations: ["Nadia L."],
};

const candidates: EnrolmentCandidates = {
  people: [{ user_id: 7, sync_id: "p7", label: "Leila M.", username: "leila" }],
  sites: [{ node_id: 3, sync_id: "s3", code: "NORD", label: "Usine Nord" }],
};

const issued: IssuedMobileEnrolment = {
  enrolment_id: "enr_abc",
  qr_payload: "maintafox-enrol:1?s=https%3A%2F%2Fexample.test&t=tok",
  code: "ABCD-2345",
  issued_at: "2026-10-29T10:00:00Z",
  expires_at: new Date(Date.now() + 15 * 60 * 1000).toISOString(),
  person_label: "Leila M.",
  site_label: "Usine Nord",
};

beforeEach(() => {
  Object.values(svc).forEach((fn) => fn.mockReset());
  svc.listMobilePhones.mockResolvedValue(view);
});

afterEach(() => {
  vi.useRealTimers();
});

async function pick(comboboxIndex: number, optionName: RegExp) {
  fireEvent.keyDown(screen.getAllByRole("combobox")[comboboxIndex] as HTMLElement, {
    key: "Enter",
  });
  fireEvent.click(await screen.findByRole("option", { name: optionName }));
}

describe("MobilePhonesPanel list", () => {
  it("shows the slots, the pending revocations and one row per phone", async () => {
    render(<MobilePhonesPanel />);

    expect(await screen.findByText("Karim B.")).toBeInTheDocument();
    expect(screen.getByText(/phones\.slots/)).toHaveTextContent('"active":2,"max":5');
    expect(screen.getByText(/phones\.pendingRevocations/)).toHaveTextContent("Nadia L.");
    expect(screen.getByText("phones.never")).toBeInTheDocument();
    expect(screen.getByText("phones.status.active")).toBeInTheDocument();
    expect(screen.getByText("phones.status.wipePending")).toBeInTheDocument();
    // Only the active phone can be revoked.
    expect(screen.getAllByRole("button", { name: "phones.revoke.action" })).toHaveLength(1);
  });

  it("says the licence has no phones when the maximum is 0", async () => {
    svc.listMobilePhones.mockResolvedValue({
      ...view,
      max_mobile_devices: 0,
      active_mobile_devices: 0,
      devices: [],
      pending_revocations: [],
    });
    render(<MobilePhonesPanel />);

    expect(await screen.findByText("phones.noLicence")).toBeInTheDocument();
    expect(screen.getByText("phones.empty")).toBeInTheDocument();
    expect(screen.queryByText(/phones\.pendingRevocations/)).not.toBeInTheDocument();
  });

  it("shows a retryable error when the list cannot be read", async () => {
    svc.listMobilePhones.mockRejectedValueOnce(new Error("offline"));
    render(<MobilePhonesPanel />);

    expect(await screen.findByRole("alert")).toHaveTextContent("phones.loadError");
    svc.listMobilePhones.mockResolvedValueOnce(view);
    fireEvent.click(screen.getByRole("button", { name: /retry|réessayer|ds\.state/i }));
    expect(await screen.findByText("Karim B.")).toBeInTheDocument();
  });
});

describe("MobilePhonesPanel revoke", () => {
  it("revokes after confirmation and passes the lost-phone choice through", async () => {
    svc.revokeMobilePhone.mockResolvedValue(undefined);
    render(<MobilePhonesPanel />);
    await screen.findByText("Karim B.");

    fireEvent.click(screen.getByRole("button", { name: "phones.revoke.action" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/phones\.revoke\.confirm/)).toHaveTextContent("Karim B.");
    fireEvent.click(within(dialog).getByLabelText("phones.revoke.lost"));
    fireEvent.click(within(dialog).getByRole("button", { name: "phones.revoke.action" }));

    await waitFor(() =>
      expect(svc.revokeMobilePhone).toHaveBeenCalledWith({ device_id: "act_1", lost: true }),
    );
    // The list is read again once the phone is revoked.
    await waitFor(() => expect(svc.listMobilePhones).toHaveBeenCalledTimes(2));
  });

  it("revokes without the lost flag by default", async () => {
    svc.revokeMobilePhone.mockResolvedValue(undefined);
    render(<MobilePhonesPanel />);
    await screen.findByText("Karim B.");

    fireEvent.click(screen.getByRole("button", { name: "phones.revoke.action" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "phones.revoke.action" }));

    await waitFor(() =>
      expect(svc.revokeMobilePhone).toHaveBeenCalledWith({ device_id: "act_1", lost: false }),
    );
  });
});

describe("MobilePhonesPanel enrolment", () => {
  it("issues a QR, shows the code and follows the enrolment until the phone used it", async () => {
    svc.listMobileEnrolmentCandidates.mockResolvedValue(candidates);
    svc.issueMobileEnrolment.mockResolvedValue(issued);
    svc.getMobileEnrolmentStatus.mockResolvedValue({
      enrolment_id: "enr_abc",
      status: "pending",
      person_label: "Leila M.",
      site_label: "Usine Nord",
      expires_at: issued.expires_at,
      redeemed_at: null,
    });

    render(<MobilePhonesPanel />);
    await screen.findByText("Karim B.");
    vi.useFakeTimers({ shouldAdvanceTime: true });

    fireEvent.click(screen.getByRole("button", { name: "phones.enrol.action" }));
    await waitFor(() => expect(svc.listMobileEnrolmentCandidates).toHaveBeenCalled());
    await pick(0, /Leila M\./);
    await pick(1, /Usine Nord/);
    fireEvent.click(screen.getByRole("button", { name: "phones.enrol.create" }));

    await waitFor(() =>
      expect(svc.issueMobileEnrolment).toHaveBeenCalledWith({ user_id: 7, site_node_id: 3 }),
    );
    expect(await screen.findByText(/phones\.enrol\.code/)).toHaveTextContent("ABCD-2345");
    expect(screen.getByText(/phones\.enrol\.validFor/)).toHaveTextContent('"time":"1');
    expect(await screen.findByRole("img", { name: "phones.enrol.qrAlt" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "phones.enrol.cancelQr" })).toBeInTheDocument();

    // Still pending after the first poll…
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(svc.getMobileEnrolmentStatus).toHaveBeenCalledWith("enr_abc");
    expect(screen.queryByText(/phones\.enrol\.enrolled/)).not.toBeInTheDocument();

    // …then the phone uses it: the list is read again and the dialog says so.
    svc.getMobileEnrolmentStatus.mockResolvedValue({
      enrolment_id: "enr_abc",
      status: "redeemed",
      person_label: "Leila M.",
      site_label: "Usine Nord",
      expires_at: issued.expires_at,
      redeemed_at: "2026-10-29T10:02:00Z",
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(await screen.findByText(/phones\.enrol\.enrolled/)).toHaveTextContent("Leila M.");
    expect(svc.listMobilePhones).toHaveBeenCalledTimes(2);
    expect(screen.queryByText(/phones\.enrol\.code/)).not.toBeInTheDocument();
  });

  it("cancels the QR and offers a new one", async () => {
    svc.listMobileEnrolmentCandidates.mockResolvedValue(candidates);
    svc.issueMobileEnrolment.mockResolvedValue(issued);
    svc.cancelMobileEnrolment.mockResolvedValue({
      enrolment_id: "enr_abc",
      status: "cancelled",
      person_label: "Leila M.",
      site_label: "Usine Nord",
      expires_at: issued.expires_at,
      redeemed_at: null,
    });

    render(<MobilePhonesPanel />);
    await screen.findByText("Karim B.");
    fireEvent.click(screen.getByRole("button", { name: "phones.enrol.action" }));
    await waitFor(() => expect(svc.listMobileEnrolmentCandidates).toHaveBeenCalled());
    await pick(0, /Leila M\./);
    await pick(1, /Usine Nord/);
    fireEvent.click(screen.getByRole("button", { name: "phones.enrol.create" }));
    fireEvent.click(await screen.findByRole("button", { name: "phones.enrol.cancelQr" }));

    expect(await screen.findByText("phones.enrol.cancelled")).toBeInTheDocument();
    expect(svc.cancelMobileEnrolment).toHaveBeenCalledWith("enr_abc");
    fireEvent.click(screen.getByRole("button", { name: "phones.enrol.newQr" }));
    expect(await screen.findByRole("button", { name: "phones.enrol.create" })).toBeInTheDocument();
  });
});
