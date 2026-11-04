import { MouseEvent } from "react";
import { issueCategory, prNumber, ticket } from "./boardState";
import { Item } from "./types";

// Clicking a badge must not also select the card it sits in.
const stop = (event: MouseEvent) => event.stopPropagation();

/** Clickable Jira and PR badges of an item. */
export function LinkBadges({ item }: { item: Pick<Item, "issue" | "issue_status" | "issue_status_category" | "pr" | "parent"> }) {
  const issue = ticket(item.issue);
  const group = issueCategory(item.issue_status_category);
  const tint = group ? ` issue-${group}` : "";
  const status = item.issue_status ? `${item.issue_status}\n` : "";
  return (
    <>
      {issue &&
        (issue.url ? (
          <a className={`badge link-badge${tint}`} href={issue.url} target="_blank" rel="noreferrer" title={`${status}${issue.url}`} onClick={stop}>
            {issue.key} ↗
          </a>
        ) : (
          <span className={`badge${tint}`} title={item.issue_status || undefined}>
            {issue.key}
          </span>
        ))}
      {item.parent && (
        <span className="badge" title={`split from ${item.parent}`}>
          ↑ {item.parent}
        </span>
      )}
      {item.pr && (
        <a className="badge link-badge" href={item.pr} target="_blank" rel="noreferrer" title={item.pr} onClick={stop}>
          PR #{prNumber(item.pr) ?? "?"} ↗
        </a>
      )}
    </>
  );
}
