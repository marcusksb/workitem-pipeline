import { MouseEvent } from "react";
import { prNumber, prState } from "./boardState";
import { IconGitMerge, IconGitPullRequest } from "./icons";
import { TONE_TEXT } from "./tone";
import { Item } from "./types";
import { cn } from "@/lib/utils";

const stop = (event: MouseEvent) => event.stopPropagation();

/** `#n · <state>` of the item's PR, linking to it; nothing without a PR. */
export function PrStatus({ item, className }: { item: Pick<Item, "pr" | "review">; className?: string }) {
  if (!item.pr) return null;
  const state = prState(item.review);
  const label = `#${prNumber(item.pr) ?? "?"}${state ? ` · ${state.label}` : ""}`;
  return (
    <a
      className={cn("inline-flex min-w-0 items-center gap-1 whitespace-nowrap text-[11.5px] no-underline hover:underline", TONE_TEXT[state?.tone ?? "idle"], className)}
      href={item.pr}
      target="_blank"
      rel="noreferrer"
      title={item.pr}
      onClick={stop}
    >
      {item.review?.state === "merged" ? <IconGitMerge /> : <IconGitPullRequest />}
      {label}
    </a>
  );
}
