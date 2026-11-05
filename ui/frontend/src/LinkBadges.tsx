import { MouseEvent } from "react";
import { issueCategory, prNumber, ticket } from "./boardState";
import { Item } from "./types";

// Clicking a badge must not also select the card it sits in.
const stop = (event: MouseEvent) => event.stopPropagation();

/** Clickable Jira (key and status) and PR badges of an item. */
export function LinkBadges({ item }: { item: Pick<Item, "issue" | "issue_status" | "issue_status_category" | "pr" | "parent"> }) {
  const issue = ticket(item.issue);
  const group = issueCategory(item.issue_status_category);
  const tint = group ? ` issue-${group}` : "";
  const label = issue && (item.issue_status ? `${issue.key} · ${item.issue_status}` : issue.key);
  return (
    <>
      {issue &&
        (issue.url ? (
          <a className={`badge link-badge${tint}`} href={issue.url} target="_blank" rel="noreferrer" title={issue.url} onClick={stop}>
            {label}
          </a>
        ) : (
          <span className={`badge${tint}`}>{label}</span>
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
