import { ComponentProps, ReactNode } from "react";
import { Dialog as Root, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

type Props = { title: string; onClose: () => void; className?: string; children: ReactNode };

/** Modal dialog, open while it is mounted. Escape, the close button or a click outside call
 * `onClose`, which may refuse (for example to keep unsaved changes) by not unmounting it. */
export function Dialog({ title, onClose, className, children }: Props) {
  return (
    <Root open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className={className} aria-describedby={undefined}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        {children}
      </DialogContent>
    </Root>
  );
}

/** A dialog's form: fields stacked, the submit button at the bottom right. */
export function DialogForm({ className, ...props }: ComponentProps<"form">) {
  return <form className={cn("grid gap-3 [&>button[type=submit]]:justify-self-end", className)} {...props} />;
}

/** A labelled control. */
export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="grid gap-1.5 text-[15px] font-medium text-muted-foreground">
      {label}
      {children}
    </label>
  );
}

/** A radio button or checkbox with its text to the right. */
export function Choice({ children }: { children: ReactNode }) {
  return <label className="flex cursor-pointer items-center gap-2 text-[15px] [&>input]:accent-primary">{children}</label>;
}
