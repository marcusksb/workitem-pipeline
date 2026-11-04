import { issueCategory } from "./boardState";

/** The Jira status of a workitem, coloured by its status category; nothing when unknown. */
export function IssueStatusChip({ status, category }: { status: string; category: string }) {
  if (!status) return null;
  const group = issueCategory(category);
  return <span className={group ? `chip issue-${group}` : "chip"}>{status}</span>;
}
