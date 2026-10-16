import { describe, expect, it } from "vitest";

import type { InterventionRequest } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

import { diActions } from "../di-actions";
import { diNextStep } from "../di-next-step";

function di(overrides: Partial<InterventionRequest> = {}): InterventionRequest {
  return {
    id: 1,
    code: "DI-0001",
    equipment_id: 10,
    sub_asset_ref: null,
    org_node_id: 5,
    status: "submitted",
    title: "Leak",
    description: "Leak on pump",
    origin_type: "operator",
    request_type: "repair",
    symptom_code_id: 3,
    impact_level: "minor",
    production_impact: false,
    safety_flag: false,
    environmental_flag: false,
    quality_flag: false,
    reported_urgency: "normal",
    validated_urgency: null,
    observed_at: null,
    submitted_at: "2026-10-01T08:00:00Z",
    reviewer_id: null,
    screened_at: null,
    approved_at: null,
    deferred_until: null,
    declined_at: null,
    closed_at: null,
    archived_at: null,
    converted_to_wo_id: null,
    converted_at: null,
    reviewer_note: null,
    classification_code_id: null,
    is_recurrence_flag: false,
    recurrence_di_id: null,
    row_version: 1,
    submitter_id: 42,
    created_at: "2026-10-01T08:00:00Z",
    updated_at: "2026-10-01T08:00:00Z",
    ...overrides,
  } as InterventionRequest;
}

const roles =
  (...granted: PermissionName[]) =>
  (p: PermissionName) =>
    granted.includes(p);

describe("diActions — decisions offered match the backend gates", () => {
  it("a requester answers and resubmits their own returned request, nothing else", () => {
    const a = diActions(di({ status: "returned_for_clarification" }), 42, roles(P.DI_CREATE_OWN));
    expect(a.resubmit).toBe(true);
    expect(a.edit).toBe(true);
    expect(a.cancelOwn).toBe(true);
    expect(a.triage || a.screen || a.approve || a.close).toBe(false);
  });

  it("another user cannot resubmit or edit someone else's request without review rights", () => {
    const a = diActions(di({ status: "returned_for_clarification" }), 7, roles(P.DI_CREATE_OWN));
    expect(a.resubmit).toBe(false);
    expect(a.edit).toBe(false);
  });

  it("a screener admits, screens and returns; only an approver approves, defers, reactivates", () => {
    const screener = roles(P.DI_SCREEN, P.DI_REVIEW);
    expect(diActions(di(), 7, screener).triage).toBe(true);
    const inReview = diActions(di({ status: "in_review" }), 7, screener);
    expect(inReview.screen && inReview.returnForInfo && inReview.close).toBe(true);
    expect(inReview.defer).toBe(false);
    const approver = roles(P.DI_APPROVE);
    expect(diActions(di({ status: "awaiting_approval" }), 7, approver).approve).toBe(true);
    expect(diActions(di({ status: "deferred" }), 7, approver).reactivate).toBe(true);
    expect(diActions(di({ status: "closed" }), 7, approver).archive).toBe(true);
  });

  it("conversion is offered once, on an approved request without a work order", () => {
    const can = roles(P.DI_CONVERT);
    expect(diActions(di({ status: "approved" }), 7, can).convert).toBe(true);
    expect(diActions(di({ status: "approved", converted_to_wo_id: 9 }), 7, can).convert).toBe(
      false,
    );
  });
});

describe("diNextStep", () => {
  it("names the requester as the one to act only when information is requested", () => {
    expect(diNextStep(di({ status: "returned_for_clarification" }))).toMatchObject({
      key: "nextStep.answer",
      requesterAction: true,
    });
    expect(diNextStep(di()).requesterAction).toBe(false);
  });

  it("follows the request to its work order and its deferral date", () => {
    expect(
      diNextStep(di({ status: "closed", converted_to_wo_id: 9, converted_to_wo_code: "OT-9" })),
    ).toMatchObject({ key: "nextStep.converted", woCode: "OT-9" });
    expect(diNextStep(di({ status: "deferred", deferred_until: "2026-11-01" }))).toMatchObject({
      key: "nextStep.deferred",
      until: "2026-11-01",
    });
    expect(diNextStep(di({ status: "awaiting_approval" })).key).toBe("nextStep.approval");
    expect(diNextStep(di({ status: "approved" })).key).toBe("nextStep.workOrder");
    expect(diNextStep(di({ status: "in_review" })).key).toBe("nextStep.review");
    expect(diNextStep(di({ status: "closed" })).key).toBe("nextStep.closed");
  });
});
