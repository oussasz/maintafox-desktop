/**
 * form-helpers.ts
 *
 * Typed wrapper around react-hook-form with Zod resolver.
 * All Phase 2 forms use this helper to ensure consistent
 * validation behavior and error display patterns.
 */

import { zodResolver } from "@hookform/resolvers/zod";
import { useForm, type DefaultValues, type FieldValues } from "react-hook-form";
import type { ZodType } from "zod";

/**
 * Create a react-hook-form instance pre-configured with a Zod schema.
 *
 * Usage:
 * ```tsx
 * const schema = z.object({ name: z.string().min(1) });
 * type FormData = z.infer<typeof schema>;
 * const form = useZodForm(schema, { name: "" });
 * ```
 */
export function useZodForm<T extends FieldValues>(
  schema: ZodType<T>,
  defaultValues: DefaultValues<T>,
) {
  return useForm<T>({
    resolver: zodResolver(schema),
    defaultValues,
    mode: "onBlur",
  });
}
