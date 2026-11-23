import { agentLine } from "./boardState";
import { TONE_TEXT } from "./tone";
import { Item } from "./types";
import { cn } from "@/lib/utils";

/** One line with a coloured dot: blocked (and why), waiting (for what), or needs approval at a gate. */
export function AgentStatus({ item, className }: { item: Pick<Item, "agent_status" | "gate" | "approved">; className?: string }) {
  const line = agentLine(item);
  if (!line) return null;
  return (
    <span className={cn("agent-status flex min-w-0 items-start gap-1.5 text-xs font-medium", TONE_TEXT[line.tone], className)} title={line.label}>
      <span className="mt-[5px] size-1.5 shrink-0 rounded-full bg-current" aria-hidden="true" />
      <span className="line-clamp-2 [overflow-wrap:anywhere]">{line.label}</span>
    </span>
  );
}
