import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import enTraining from "@/i18n/locale-data/en/training.json";

import { HANDOVER_NOTE_MIN, PermitHandoverForm } from "../PermitHandoverForm";

// Keys resolve against the real English catalogue so a missing key fails the test.
function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

function fill(text: string, opts: Record<string, unknown> = {}): string {
  return text.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts[n] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = lookup(enPermits, key);
      if (value == null) throw new Error(`missing permits key: ${key}`);
      return fill(value, opts);
    },
    i18n: { language: "en" },
  }),
}));

// A coded refusal is shown with its translated text (training.json "errors").
vi.mock("@/lib/app-error", () => ({
  appErrorReason: (e: { code?: string }) =>
    ((e.code && lookup(enTraining, `errors.${e.code}`)) || "unknown").replace(/\{\{\w+\}\}/g, ""),
}));

const session = vi.hoisted(() => ({
  info: { user_id: 7, username: "amine", display_name: "Amine Tech" } as Record<
    string,
    unknown
  > | null,
}));
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: session.info }),
}));

const svc = vi.hoisted(() => ({
  listPermitHandoverRoles: vi.fn(),
  listPermitHandoverReceivers: vi.fn(),
  appendPermitHandoverLog: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

const ROLES = [
  { code: "MAINT", label: "Maintenance", review_required: false },
  { code: "Exploitation", label: "Exploitation", review_required: true },
];

async function choose(trigger: HTMLElement, option: string) {
  fireEvent.keyDown(trigger, { key: "Enter" });
  fireEvent.click(await screen.findByRole("option", { name: option }));
}

function renderForm(onRecorded = vi.fn()) {
  render(<PermitHandoverForm permitId={42} onRecorded={onRecorded} />);
  return onRecorded;
}

const noteBox = () => screen.getByRole("textbox", { name: new RegExp(enPermits.handover.note) });

describe("PermitHandoverForm", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.info = { user_id: 7, username: "amine", display_name: "Amine Tech" };
    svc.listPermitHandoverRoles.mockResolvedValue(ROLES);
    svc.listPermitHandoverReceivers.mockResolvedValue({
      items: [{ id: 9, display_name: "Ali Operations" }],
      total: 1,
    });
    svc.appendPermitHandoverLog.mockResolvedValue({});
  });

  it("shows the signed-in user as the signer, read-only", async () => {
    renderForm();
    expect(await screen.findByTestId("handover-signer")).toHaveTextContent(
      fill(enPermits.handover.signerYou, { name: "Amine Tech" }),
    );
    // Nothing to type: the signer is never sent by the screen.
    expect(screen.queryByRole("textbox", { name: /sign/i })).toBeNull();
  });

  it("asks for both roles and a note of the minimum length, with nothing preselected", async () => {
    renderForm();
    await waitFor(() => expect(svc.listPermitHandoverRoles).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: enPermits.handover.submit })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: enPermits.handover.submit }));
    expect(await screen.findAllByText(enPermits.errors.required)).toHaveLength(2);
    expect(
      screen.getByText(fill(enPermits.handover.noteMin, { min: HANDOVER_NOTE_MIN })),
    ).toBeInTheDocument();
    expect(svc.appendPermitHandoverLog).not.toHaveBeenCalled();
  });

  it("checks the note length while it is typed", async () => {
    renderForm();
    await screen.findByRole("combobox", { name: enPermits.handover.from });
    const minText = fill(enPermits.handover.noteMin, { min: HANDOVER_NOTE_MIN });
    fireEvent.change(noteBox(), { target: { value: "Short" } });
    expect(screen.getByText(minText)).toBeInTheDocument();
    expect(
      screen.getByText(fill(enPermits.handover.noteCount, { length: 5, min: HANDOVER_NOTE_MIN })),
    ).toBeInTheDocument();
    fireEvent.change(noteBox(), { target: { value: "Zone left clean" } });
    expect(screen.queryByText(minText)).toBeNull();
  });

  it("records the hand-over with list codes, the named receiver and the trimmed note", async () => {
    const onRecorded = renderForm();
    await screen.findByRole("combobox", { name: enPermits.handover.from });
    await choose(screen.getByRole("combobox", { name: enPermits.handover.from }), "Maintenance");
    await choose(
      screen.getByRole("combobox", { name: enPermits.handover.to }),
      "Exploitation (to review)",
    );
    await choose(
      screen.getByRole("combobox", { name: enPermits.handover.receiver }),
      "Ali Operations",
    );
    fireEvent.change(noteBox(), { target: { value: "  Zone left clean  " } });
    fireEvent.click(screen.getByRole("button", { name: enPermits.handover.submit }));
    await waitFor(() =>
      expect(svc.appendPermitHandoverLog).toHaveBeenCalledWith({
        permit_id: 42,
        handed_from_role: "MAINT",
        handed_to_role: "Exploitation",
        confirmation_note: "Zone left clean",
        received_by_id: 9,
      }),
    );
    expect(onRecorded).toHaveBeenCalled();
  });

  it("sends no receiver when nobody is named", async () => {
    renderForm();
    await screen.findByRole("combobox", { name: enPermits.handover.from });
    await choose(screen.getByRole("combobox", { name: enPermits.handover.from }), "Maintenance");
    await choose(screen.getByRole("combobox", { name: enPermits.handover.to }), "Maintenance");
    fireEvent.change(noteBox(), { target: { value: "Zone left clean" } });
    fireEvent.click(screen.getByRole("button", { name: enPermits.handover.submit }));
    await waitFor(() =>
      expect(svc.appendPermitHandoverLog).toHaveBeenCalledWith(
        expect.objectContaining({ received_by_id: null }),
      ),
    );
  });

  it("tells an administrator where to fill the roles when the plant has none, and blocks saving", async () => {
    svc.listPermitHandoverRoles.mockResolvedValue([]);
    renderForm();
    expect(await screen.findByText(enPermits.handover.noRoles)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: enPermits.handover.submit })).toBeDisabled();
    // No free-text role field.
    expect(screen.queryByRole("textbox", { name: enPermits.handover.from })).toBeNull();
  });

  it("says when the receiver list is longer than shown, and searches by name on the server", async () => {
    svc.listPermitHandoverReceivers.mockResolvedValue({
      items: [{ id: 9, display_name: "Ali Operations" }],
      total: 250,
    });
    renderForm();
    expect(
      await screen.findByText(fill(enPermits.handover.receiversPartial, { shown: 1, total: 250 })),
    ).toBeInTheDocument();
    expect(svc.listPermitHandoverReceivers).toHaveBeenCalledWith(null);

    svc.listPermitHandoverReceivers.mockResolvedValue({
      items: [{ id: 12, display_name: "Karim Shift" }],
      total: 1,
    });
    fireEvent.change(screen.getByRole("textbox", { name: enPermits.handover.receiverSearch }), {
      target: { value: "kar" },
    });
    await waitFor(() => expect(svc.listPermitHandoverReceivers).toHaveBeenCalledWith("kar"));
    expect(
      await screen.findByText(fill(enPermits.handover.receiversAll, { total: 1 })),
    ).toBeInTheDocument();
  });

  it("shows more people with the list's own offset and keeps the count honest", async () => {
    svc.listPermitHandoverReceivers.mockResolvedValueOnce({
      items: [{ id: 9, display_name: "Ali Operations" }],
      total: 3,
    });
    svc.listPermitHandoverReceivers.mockResolvedValueOnce({
      items: [
        { id: 10, display_name: "Bilal Shift" },
        { id: 11, display_name: "Chaima Ops" },
      ],
      total: 3,
    });
    renderForm();
    expect(
      await screen.findByText(fill(enPermits.handover.receiversPartial, { shown: 1, total: 3 })),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: enPermits.handover.receiversMore }));
    await waitFor(() =>
      expect(svc.listPermitHandoverReceivers).toHaveBeenLastCalledWith(null, undefined, 1),
    );
    expect(
      await screen.findByText(fill(enPermits.handover.receiversAll, { total: 3 })),
    ).toBeInTheDocument();
    // Everything is shown: no button left, and the added people can be picked.
    expect(screen.queryByRole("button", { name: enPermits.handover.receiversMore })).toBeNull();
    fireEvent.keyDown(screen.getByRole("combobox", { name: enPermits.handover.receiver }), {
      key: "Enter",
    });
    expect(await screen.findByRole("option", { name: "Chaima Ops" })).toBeInTheDocument();
  });

  it("offers no 'show more' when every matching person is shown", async () => {
    renderForm();
    await screen.findByText(fill(enPermits.handover.receiversAll, { total: 1 }));
    expect(screen.queryByRole("button", { name: enPermits.handover.receiversMore })).toBeNull();
  });

  it.each([
    "PERMIT_HANDOVER_ROLE_NOT_IN_LIST",
    "PERMIT_HANDOVER_NOTE_TOO_SHORT",
    "PERMIT_HANDOVER_RECEIVER_IS_SIGNER",
    "PERMIT_HANDOVER_RECEIVER_UNKNOWN",
  ])("shows the translated refusal %s", async (code) => {
    svc.appendPermitHandoverLog.mockRejectedValue({ code });
    renderForm();
    await screen.findByRole("combobox", { name: enPermits.handover.from });
    await choose(screen.getByRole("combobox", { name: enPermits.handover.from }), "Maintenance");
    await choose(screen.getByRole("combobox", { name: enPermits.handover.to }), "Maintenance");
    fireEvent.change(noteBox(), { target: { value: "Zone left clean" } });
    fireEvent.click(screen.getByRole("button", { name: enPermits.handover.submit }));
    const text = lookup(enTraining, `errors.${code}`);
    expect(text).toBeTruthy();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      String(text)
        .replace(/\{\{\w+\}\}/g, "")
        .replace(/\s+/g, " "),
    );
  });
});
