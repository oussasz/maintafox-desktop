import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";

import { mfButton } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

const buttonVariants = cva(mfButton.base, {
  variants: {
    variant: {
      default: mfButton.primary,
      destructive: mfButton.destructive,
      outline: mfButton.outline,
      secondary: mfButton.secondary,
      ghost: mfButton.ghost,
      link: mfButton.link,
    },
    size: {
      default: mfButton.sizeDefault,
      sm: mfButton.sizeSm,
      lg: mfButton.sizeLg,
      icon: mfButton.sizeIcon,
    },
  },
  defaultVariants: {
    variant: "default",
    size: "default",
  },
});

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp className={cn(buttonVariants({ variant, size, className }))} ref={ref} {...props} />
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
