import { renderHook, act } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { z } from "zod";

import { useZodForm } from "../form-helpers";

describe("useZodForm", () => {
  const schema = z.object({
    name: z.string().min(1, "Name is required"),
    email: z.string().email("Invalid email"),
  });

  function useTrackedForm() {
    const form = useZodForm(schema, { name: "", email: "" });
    // Subscribe to formState.errors so the proxy tracks changes
    const errors = form.formState.errors;
    return { ...form, errors };
  }

  it("initializes with default values", () => {
    const { result } = renderHook(() => useZodForm(schema, { name: "", email: "" }));
    expect(result.current.getValues()).toEqual({ name: "", email: "" });
  });

  it("reports validation errors for invalid data", async () => {
    const { result } = renderHook(() => useTrackedForm());

    await act(async () => {
      await result.current.trigger();
    });

    expect(result.current.errors.name?.message).toBe("Name is required");
    expect(result.current.errors.email?.message).toBe("Invalid email");
  });

  it("passes validation for correct data", async () => {
    const { result } = renderHook(() =>
      useZodForm(schema, { name: "John", email: "john@example.com" }),
    );

    let isValid = false;
    await act(async () => {
      isValid = await result.current.trigger();
    });

    expect(isValid).toBe(true);
  });
});
