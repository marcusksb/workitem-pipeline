import * as React from "react";
import { Slot } from "radix-ui";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex h-5 w-fit shrink-0 items-center gap-1 overflow-hidden whitespace-nowrap rounded-[5px] border px-1.5 text-[11.5px] font-medium [&>svg]:pointer-events-none [&>svg]:size-3",
  {
    variants: {
      variant: {
        default: "border-transparent bg-primary text-primary-foreground",
        secondary: "border-transparent bg-secondary text-muted-foreground",
        outline: "border-border bg-card text-muted-foreground",
        danger: "border-transparent bg-destructive/10 text-destructive",
        good: "border-good/30 bg-card text-good",
        wait: "border-working/30 bg-card text-working",
        merged: "border-merged/30 bg-card text-merged",
        gate: "border-gate/30 bg-card text-gate",
        info: "border-primary/30 bg-card text-primary",
      },
    },
    defaultVariants: { variant: "outline" },
  },
);

function Badge({
  className,
  variant,
  asChild = false,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : "span";
  return <Comp data-slot="badge" className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
