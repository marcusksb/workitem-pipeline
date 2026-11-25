import { MouseEvent } from "react";
import { issueCategory, prNumber, ticket } from "./boardState";
import { IconArrowUp } from "./icons";
import { ISSUE_BADGE } from "./tone";
import { Item } from "./types";
import { Badge } from "@/components/ui/badge";

// Clicking a badge must not also select the card it sits in.
const stop = (event: MouseEvent) => event.stopPropagation();

/** The item's issue as a clickable badge (key and status), coloured by the status category. Nothing without an issue. */
export function IssueBadge({ item }: { item: Pick<Item, "issue" | "issue_status" | "issue_status_category"> }) {
  const issue = ticket(item.issue);
  if (!issue) return null;
  const group = issueCategory(item.issue_status_category);
  const variant = group ? ISSUE_BADGE[group] : "outline";
  const label = item.issue_status ? `${issue.key} · ${item.issue_status}` : issue.key;
  return issue.url ? (
    <Badge asChild variant={variant}>
      <a className="no-underline hover:underline" href={issue.url} target="_blank" rel="noreferrer" title={issue.url} onClick={stop}>
        <span className="truncate">{label}</span>
      </a>
    </Badge>
  ) : (
    <Badge variant={variant} title={label}>
      <span className="truncate">{label}</span>
    </Badge>
  );
}

/** The item's parent as a short "parent" badge; `onOpen` (absent when the parent is not loaded) makes it open the parent. */
function ParentBadge({ parent, onOpen }: { parent: string; onOpen?: () => void }) {
  const title = `split from ${parent}`;
  return onOpen ? (
    <Badge asChild className="cursor-pointer hover:underline">
      <button
        type="button"
        title={title}
        aria-label={`Open parent ${parent}`}
        onClick={(event) => {
          event.stopPropagation();
          onOpen();
        }}
      >
        <IconArrowUp />
        parent
      </button>
    </Badge>
  ) : (
    <Badge title={title}>
      <IconArrowUp />
      parent
    </Badge>
  );
}

/** Clickable issue (key and status), parent and, unless `pr` is false, PR badges of an item. */
export function LinkBadges({ item, pr = true, onParent }: { item: Pick<Item, "issue" | "issue_status" | "issue_status_category" | "pr" | "parent">; pr?: boolean; onParent?: () => void }) {
  return (
    <>
      <IssueBadge item={item} />
      {item.parent && <ParentBadge parent={item.parent} onOpen={onParent} />}
      {pr && item.pr && (
        <Badge asChild variant="info">
          <a className="no-underline hover:underline" href={item.pr} target="_blank" rel="noreferrer" title={item.pr} onClick={stop}>
            PR #{prNumber(item.pr) ?? "?"} ↗
          </a>
        </Badge>
      )}
    </>
  );
}
