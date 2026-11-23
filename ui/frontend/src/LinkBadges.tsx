import { MouseEvent } from "react";
import { issueCategory, prNumber, ticket } from "./boardState";
import { IconArrowUp } from "./icons";
import { Item } from "./types";
import { Badge } from "@/components/ui/badge";

// Clicking a badge must not also select the card it sits in.
const stop = (event: MouseEvent) => event.stopPropagation();

/** Badge variant of an issue status category. */
export const ISSUE_BADGE = { todo: "outline", progress: "info", done: "good" } as const;

/** Clickable issue (key and status), parent and, unless `pr` is false, PR badges of an item. */
export function LinkBadges({ item, pr = true }: { item: Pick<Item, "issue" | "issue_status" | "issue_status_category" | "pr" | "parent">; pr?: boolean }) {
  const issue = ticket(item.issue);
  const group = issueCategory(item.issue_status_category);
  const variant = group ? ISSUE_BADGE[group] : "outline";
  const label = issue && (item.issue_status ? `${issue.key} · ${item.issue_status}` : issue.key);
  return (
    <>
      {issue &&
        (issue.url ? (
          <Badge asChild variant={variant}>
            <a className="no-underline hover:underline" href={issue.url} target="_blank" rel="noreferrer" title={issue.url} onClick={stop}>
              {label}
            </a>
          </Badge>
        ) : (
          <Badge variant={variant}>{label}</Badge>
        ))}
      {item.parent && (
        <Badge className="max-w-full" title={`split from ${item.parent}`}>
          <IconArrowUp />
          <span className="truncate">{item.parent}</span>
        </Badge>
      )}
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
