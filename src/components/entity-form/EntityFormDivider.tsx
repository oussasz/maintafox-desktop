import type { HTMLAttributes } from "react";

import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export type EntityFormDividerProps = HTMLAttributes<HTMLHRElement>;

export function EntityFormDivider({ className, ...props }: EntityFormDividerProps) {
  return <hr className={cn(mfEntityForm.divider, className)} {...props} />;
}
