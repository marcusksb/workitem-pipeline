import { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/** Small pieces of text and layout shared by the panels and dialogs. */

export function ErrorText({ className, ...props }: ComponentProps<"p">) {
  return <p className={cn("text-[13px] whitespace-pre-line text-destructive", className)} {...props} />;
}

/** Secondary text: hints, empty states, loading. */
export function Muted({ className, ...props }: ComponentProps<"p">) {
  return <p className={cn("text-[13px] text-muted-foreground", className)} {...props} />;
}

/** The row of buttons at the top of a tab, sticky while it scrolls. */
export function ActionBar({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("sticky top-0 z-[1] flex flex-wrap items-center gap-2 bg-card pb-3 text-[13px]", className)} {...props} />;
}

/** Rendered markdown: 14px/1.6 prose, 13px semibold headings, inline code on a muted background. */
export const prose = [
  "text-[14px] leading-[1.6] [overflow-wrap:anywhere] [&>:first-child]:mt-0",
  "[&_p]:mb-3 [&_ul]:mb-3 [&_ol]:mb-3 [&_pre]:mb-3 [&_table]:mb-3 [&_blockquote]:mb-3",
  "[&_h1]:mt-5 [&_h1]:mb-1.5 [&_h2]:mt-5 [&_h2]:mb-1.5 [&_h3]:mt-5 [&_h3]:mb-1.5 [&_h4]:mt-5 [&_h4]:mb-1.5",
  "[&_h1]:text-[13px] [&_h2]:text-[13px] [&_h3]:text-[13px] [&_h4]:text-[13px] [&_h1]:font-semibold [&_h2]:font-semibold [&_h3]:font-semibold [&_h4]:font-semibold",
  "[&_ul]:list-disc [&_ol]:list-decimal [&_ul]:pl-6 [&_ol]:pl-6 [&_li]:my-0.5 [&_li>ul]:mb-0 [&_li>ol]:mb-0",
  "[&_ul.contains-task-list]:list-none [&_ul.contains-task-list]:pl-1 [&_input]:mr-1.5 [&_input]:accent-primary",
  "[&_a]:text-primary [&_a]:no-underline [&_a:hover]:underline",
  "[&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[12.5px]",
  "[&_pre]:overflow-auto [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:bg-muted [&_pre]:px-3 [&_pre]:py-2.5 [&_pre_code]:bg-transparent [&_pre_code]:p-0",
  "[&_blockquote]:border-l-[3px] [&_blockquote]:border-border [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground",
  "[&_hr]:my-5 [&_hr]:border-border",
  "[&_table]:block [&_table]:overflow-auto [&_table]:border-collapse [&_th]:border [&_td]:border [&_th]:border-border [&_td]:border-border [&_th]:px-2 [&_td]:px-2 [&_th]:py-1 [&_td]:py-1 [&_th]:bg-muted [&_th]:text-left [&_td]:align-top",
].join(" ");

/** The right-hand panel of an item or a plan. It slides in, and slides out while App's `Leaving`
 * wrapper (class `leaving`) keeps it mounted. */
export function SidePanel({ className, ...props }: ComponentProps<"aside">) {
  return (
    <aside
      className={cn(
        "detail absolute inset-y-0 right-0 z-20 flex w-full min-w-0 flex-col overflow-hidden border-l border-border bg-card shadow-[-12px_0_32px_rgb(16_24_40/0.08)] md:w-[max(640px,50%)]",
        "animate-[detail-in_180ms_ease-out] motion-reduce:[animation-duration:1ms] [.leaving>&]:pointer-events-none [.leaving>&]:animate-[detail-out_180ms_ease-in_forwards] motion-reduce:[.leaving>&]:[animation-duration:1ms]",
        className,
      )}
      {...props}
    />
  );
}

/** A tab's scrolling body. */
export const scroll = "min-h-0 flex-1 overflow-auto pb-8";
