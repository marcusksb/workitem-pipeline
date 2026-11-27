import { ComponentProps, type CSSProperties, type PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

/** Small pieces of text and layout shared by the panels and dialogs. */

export function ErrorText({ className, ...props }: ComponentProps<"p">) {
  return <p className={cn("text-[15px] whitespace-pre-line text-destructive", className)} {...props} />;
}

/** Secondary text: hints, empty states, loading. */
export function Muted({ className, ...props }: ComponentProps<"p">) {
  return <p className={cn("text-[15px] text-muted-foreground", className)} {...props} />;
}

/** The row of buttons at the top of a tab, sticky while it scrolls. */
export function ActionBar({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("sticky top-0 z-[1] flex flex-wrap items-center gap-2 bg-card pb-3 text-[15px]", className)} {...props} />;
}

/** Rendered markdown: 16px/1.6 prose, 15px semibold headings, inline code on a muted background. */
export const prose = [
  "text-[16px] leading-[1.6] [overflow-wrap:anywhere] [&>:first-child]:mt-0",
  "[&_p]:mb-3 [&_ul]:mb-3 [&_ol]:mb-3 [&_pre]:mb-3 [&_table]:mb-3 [&_blockquote]:mb-3",
  "[&_h1]:mt-5 [&_h1]:mb-1.5 [&_h2]:mt-5 [&_h2]:mb-1.5 [&_h3]:mt-5 [&_h3]:mb-1.5 [&_h4]:mt-5 [&_h4]:mb-1.5",
  "[&_h1]:text-[15px] [&_h2]:text-[15px] [&_h3]:text-[15px] [&_h4]:text-[15px] [&_h1]:font-semibold [&_h2]:font-semibold [&_h3]:font-semibold [&_h4]:font-semibold",
  "[&_ul]:list-disc [&_ol]:list-decimal [&_ul]:pl-6 [&_ol]:pl-6 [&_li]:my-0.5 [&_li>ul]:mb-0 [&_li>ol]:mb-0",
  "[&_ul.contains-task-list]:list-none [&_ul.contains-task-list]:pl-1 [&_input]:mr-1.5 [&_input]:accent-primary",
  "[&_a]:text-primary [&_a]:no-underline [&_a:hover]:underline",
  "[&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[14.5px]",
  "[&_pre]:overflow-auto [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:bg-muted [&_pre]:px-3 [&_pre]:py-2.5 [&_pre_code]:bg-transparent [&_pre_code]:p-0",
  "[&_blockquote]:border-l-[3px] [&_blockquote]:border-border [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground",
  "[&_hr]:my-5 [&_hr]:border-border",
  "[&_table]:block [&_table]:overflow-auto [&_table]:border-collapse [&_th]:border [&_td]:border [&_th]:border-border [&_td]:border-border [&_th]:px-2 [&_td]:px-2 [&_th]:py-1 [&_td]:py-1 [&_th]:bg-muted [&_th]:text-left [&_td]:align-top",
].join(" ");

const PANEL_WIDTH_KEY = "workload.panel.width";
/** The narrowest a dragged panel may get; below `md` it is full width anyway. */
const MIN_PANEL_WIDTH = 320;

function readPanelWidth(): number | null {
  try {
    const raw = localStorage.getItem(PANEL_WIDTH_KEY);
    if (raw === null) return null;
    const width = Number(raw);
    return Number.isFinite(width) ? Math.max(MIN_PANEL_WIDTH, Math.round(width)) : null;
  } catch {
    return null;
  }
}

function savePanelWidth(width: number) {
  try {
    localStorage.setItem(PANEL_WIDTH_KEY, String(Math.round(width)));
  } catch {
    // the width just is not remembered
  }
}

function clearPanelWidth() {
  try {
    localStorage.removeItem(PANEL_WIDTH_KEY);
  } catch {
    // nothing stored to clear
  }
}

/** The right-hand panel of an item or a plan. It slides in, and slides out while App's `Leaving`
 * wrapper (class `leaving`) keeps it mounted. Its left edge drags to resize it, and the width is
 * shared with the other panel through localStorage; a double-click restores the responsive default. */
export function SidePanel({ className, children, ...props }: ComponentProps<"aside">) {
  const aside = useRef<HTMLElement>(null);
  const drag = useRef<{ x: number; width: number } | null>(null);
  const [width, setWidth] = useState(readPanelWidth);

  useEffect(() => () => document.body.classList.remove("select-none"), []);

  const clamp = (width: number) => {
    const room = aside.current?.closest("main")?.clientWidth ?? Number.POSITIVE_INFINITY;
    return Math.min(Math.max(width, MIN_PANEL_WIDTH), room);
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    const panel = aside.current;
    if (!panel || event.button !== 0) return;
    // Keep focus where it was, so a terminal inside the panel keeps receiving keys.
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, width: panel.getBoundingClientRect().width };
    document.body.classList.add("select-none");
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (!start) return;
    setWidth(clamp(start.width + start.x - event.clientX));
  };

  const endDrag = () => {
    const start = drag.current;
    if (!start) return;
    drag.current = null;
    document.body.classList.remove("select-none");
    setWidth((width) => {
      if (width !== null) savePanelWidth(width);
      return width;
    });
  };

  const reset = () => {
    setWidth(null);
    clearPanelWidth();
  };

  return (
    <aside
      ref={aside}
      style={width === null ? undefined : ({ "--panel-width": `${width}px` } as CSSProperties)}
      className={cn(
        "detail absolute inset-y-0 right-0 z-20 flex w-full min-w-0 flex-col overflow-hidden border-l border-border bg-card shadow-[-12px_0_32px_rgb(16_24_40/0.08)] md:w-[var(--panel-width,max(640px,50%))]",
        width !== null && "max-w-full",
        "animate-[detail-in_180ms_ease-out] motion-reduce:[animation-duration:1ms] [.leaving>&]:pointer-events-none [.leaving>&]:animate-[detail-out_180ms_ease-in_forwards] motion-reduce:[.leaving>&]:[animation-duration:1ms]",
        className,
      )}
      {...props}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize panel"
        data-panel-handle
        className="absolute inset-y-0 left-0 z-10 hidden w-2 cursor-col-resize touch-none md:block"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={reset}
      />
      {children}
    </aside>
  );
}

/** A tab's scrolling body. */
export const scroll = "min-h-0 flex-1 overflow-auto pb-8";
