import * as React from "react";
import { cn } from "@/lib/utils";

const field =
  "w-full min-w-0 rounded-md border border-input bg-card px-2.5 text-[15px] text-foreground shadow-xs transition-[border-color,box-shadow] outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/20 disabled:cursor-not-allowed disabled:border-transparent disabled:bg-foreground/5 disabled:text-muted-foreground disabled:shadow-none";

function Input({ className, ...props }: React.ComponentProps<"input">) {
  return <input data-slot="input" className={cn(field, "h-8", className)} {...props} />;
}

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return <textarea data-slot="textarea" className={cn(field, "py-1.5 leading-normal", className)} {...props} />;
}

/** A native `<select>`, styled like the inputs. */
function NativeSelect({ className, ...props }: React.ComponentProps<"select">) {
  return <select data-slot="native-select" className={cn(field, "h-8 cursor-pointer pr-1.5", className)} {...props} />;
}

export { Input, Textarea, NativeSelect };
