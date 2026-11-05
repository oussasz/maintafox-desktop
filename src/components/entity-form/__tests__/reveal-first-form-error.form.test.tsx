import { zodResolver } from "@hookform/resolvers/zod";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { z } from "zod";

import { FormField } from "@/components/ui/FormField";

import { revealFirstFormError } from "../reveal-first-form-error";

const schema = z.object({
  asset_code: z.string().min(1, "code requis"),
  criticality_code: z.string().min(1, "criticité requise"),
});
type Values = z.infer<typeof schema>;

/** A long form whose required custom picker is not a registered input, like the asset form. */
function LongForm() {
  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { asset_code: "PMP-01", criticality_code: "" },
  });
  return (
    <form
      id="long-form"
      onSubmit={(e) =>
        void handleSubmit(
          () => {},
          () => revealFirstFormError("long-form"),
        )(e)
      }
    >
      <FormField name="asset_code" label="Code" error={errors.asset_code?.message} required>
        <input id="asset_code" {...register("asset_code")} />
      </FormField>
      <FormField
        name="criticality_code"
        label="Criticité"
        error={errors.criticality_code?.message}
        required
      >
        <button id="criticality_code" type="button">
          Choisir
        </button>
      </FormField>
      <button type="submit">Créer</button>
    </form>
  );
}

describe("revealFirstFormError in a react-hook-form submit", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("scrolls to and focuses the missing required picker after an invalid submit", async () => {
    render(<LongForm />);
    fireEvent.click(screen.getByRole("button", { name: "Créer" }));

    // The label names the picker ("Criticité"), as in the asset form.
    await waitFor(() =>
      expect(document.activeElement).toBe(document.getElementById("criticality_code")),
    );
    expect(screen.getByText("criticité requise").scrollIntoView).toHaveBeenCalledWith({
      block: "center",
    });
  });
});
