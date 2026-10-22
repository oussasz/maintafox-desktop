/**
 * The received changes that wait are shown with what they wait for, never as a bare count: an
 * open conflict by its number, a record not received yet, an app update.
 */
import { render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { SyncWaitingList } from "@/components/sync/SyncWaitingList";
import enShell from "@/i18n/en/shell.json";

const mocks = vi.hoisted(() => ({ listWaitingInboxRows: vi.fn() }));
vi.mock("@/services/sync-service", () => mocks);

const onConflict = {
  state: "conflict",
  conflict_id: 12,
  entity_type: "pm_plan_versions",
  entity_sync_id: "47c8f911",
  reason_code: null,
};

describe("SyncWaitingList", () => {
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

  beforeEach(() => mocks.listWaitingInboxRows.mockReset());

  it("names the conflict the rows wait for and lists each row with its reason", async () => {
    mocks.listWaitingInboxRows.mockResolvedValue({
      total: 3,
      groups: [
        { target: onConflict, count: 2 },
        {
          target: {
            state: "own_reason",
            conflict_id: null,
            entity_type: null,
            entity_sync_id: null,
            reason_code: "MISSING_FIELD",
          },
          count: 1,
        },
      ],
      rows: [
        {
          inbox_id: 7,
          entity_type: "work_orders",
          entity_sync_id: "wo-1",
          payload_json: JSON.stringify({ code: "OT-0012", title: "Pump PM" }),
          reason_code: "MISSING_PARENT",
          detail_message: "pm_occurrence_id needs pm_occurrences occ-1, not received yet",
          waits_for_entity_type: "pm_occurrences",
          waits_for_sync_id: "occ-1",
          target: onConflict,
          attempt_count: 4,
          last_attempt_at: null,
        },
      ],
    });

    render(<SyncWaitingList count={3} />);

    expect(
      await screen.findByText(
        "3 received change(s) are waiting. They are applied automatically once what they wait for is here.",
      ),
    ).toBeInTheDocument();
    const onConflictText = "Waiting for conflict #12 (PM plan version) to be resolved";
    // The group and the listed row both say what they wait for.
    expect(screen.getAllByText(onConflictText)).toHaveLength(2);
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(
      screen.getByText("Sent by an older app version: waiting for the other PC to be updated"),
    ).toBeInTheDocument();
    expect(screen.getByText("OT-0012")).toBeInTheDocument();
    // The recorded text is only in the technical details.
    expect(screen.getByText(/occ-1, not received yet/).closest("details")).not.toBeNull();
    // Rows beyond the list are counted, not hidden.
    expect(screen.getByText("2 more not listed")).toBeInTheDocument();
  });

  it("shows nothing and loads nothing when no row waits", () => {
    const { container } = render(<SyncWaitingList count={0} />);
    expect(container).toBeEmptyDOMElement();
    expect(mocks.listWaitingInboxRows).not.toHaveBeenCalled();
  });

  it("reports a load failure with a retry instead of a bare count", async () => {
    mocks.listWaitingInboxRows.mockRejectedValueOnce({ code: "DB", message: "down" });
    render(<SyncWaitingList count={5} />);
    expect(await screen.findByText("The waiting changes could not be loaded.")).toBeInTheDocument();
  });
});
