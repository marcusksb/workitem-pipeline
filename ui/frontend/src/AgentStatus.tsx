import { agentLine } from "./boardState";
import { ci } from "./ReviewStrip";
import { TONE_TEXT } from "./tone";
import { Item } from "./types";
import { cn } from "@/lib/utils";

/** One line with a coloured dot: blocked, waiting (for what), or needs approval at a gate. Waiting on CI is the spinning CI icon alone. */
export function AgentStatus({ item, className }: { item: Pick<Item, "agent_status" | "gate" | "approved" | "review">; className?: string }) {
  const line = agentLine(item);
  if (!line) return null;
  if (line.ci) {
    return (
      <span
        role="img"
        aria-label={line.label}
        title={line.label}
        className={cn("agent-status inline-flex [&>svg]:size-3.5 motion-safe:animate-spin", TONE_TEXT[line.tone], className)}
      >
        {ci(0).pending.glyph}
      </span>
    );
  }
  return (
    <span className={cn("agent-status flex min-w-0 items-start gap-1.5 text-xs font-medium", TONE_TEXT[line.tone], className)} title={line.label}>
      <span className="mt-[5px] size-1.5 shrink-0 rounded-full bg-current" aria-hidden="true" />
      <span className="line-clamp-2 [overflow-wrap:anywhere]">{line.label}</span>
    </span>
  );
}
