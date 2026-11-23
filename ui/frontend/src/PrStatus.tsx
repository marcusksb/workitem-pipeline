import { MouseEvent } from "react";
import { prNumber } from "./boardState";
import { IconGitPullRequest } from "./icons";
import { APPROVAL, ci, DONE, MERGE } from "./ReviewStrip";
import { TONE_BADGE, TONE_TEXT } from "./tone";
import { Item } from "./types";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

const stop = (event: MouseEvent) => event.stopPropagation();

/**
 * The item's PR: a `#n` pill linking to it, coloured by whether it can merge (or merged/closed),
 * then for an open PR an approval icon and a CI icon (none without CI). Nothing without a PR.
 */
export function PrStatus({ item, className }: { item: Pick<Item, "pr" | "review">; className?: string }) {
  if (!item.pr) return null;
  const review = item.review;
  const pill = review && (review.state === "open" ? MERGE[review.merge] : DONE[review.state]);
  const icons = review?.state === "open" ? [APPROVAL[review.approval], ...(review.ci === "none" ? [] : [ci(review.failed.length)[review.ci]])] : [];
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5", className)}>
      <Badge asChild variant={pill ? TONE_BADGE[pill.tone] : "outline"} className="relative no-underline hover:underline">
        <a href={item.pr} target="_blank" rel="noreferrer" title={pill ? `${item.pr} · ${pill.label}` : item.pr} onClick={stop}>
          <IconGitPullRequest />#{prNumber(item.pr) ?? "?"}
          {pill && <span className="sr-only">{pill.label}</span>}
        </a>
      </Badge>
      {icons.map(({ glyph, tone, label }) => (
        <span key={label} role="img" aria-label={label} title={label} className={cn("inline-flex [&>svg]:size-3.5", TONE_TEXT[tone])}>
          {glyph}
        </span>
      ))}
    </span>
  );
}
