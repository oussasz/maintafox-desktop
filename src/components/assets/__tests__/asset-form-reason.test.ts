import { describe, expect, it } from "vitest";

import { assetFormDisabledReasonKey } from "@/components/assets/asset-form-reason";

const ready = { saving: false, photosBusy: false, catalogReady: true, catalogFailed: false };

describe("assetFormDisabledReasonKey", () => {
  it("is null when the button is available", () => {
    expect(assetFormDisabledReasonKey(ready)).toBeNull();
  });

  it("says why the button is grey", () => {
    expect(assetFormDisabledReasonKey({ ...ready, saving: true })).toBe("actionReason.saving");
    expect(assetFormDisabledReasonKey({ ...ready, photosBusy: true })).toBe(
      "actionReason.photosBusy",
    );
    expect(assetFormDisabledReasonKey({ ...ready, catalogReady: false })).toBe(
      "actionReason.catalogLoading",
    );
    expect(assetFormDisabledReasonKey({ ...ready, catalogReady: false, catalogFailed: true })).toBe(
      "actionReason.catalogFailed",
    );
  });
});
