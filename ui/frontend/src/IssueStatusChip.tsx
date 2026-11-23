import { issueCategory } from "./boardState";
import { ISSUE_BADGE } from "./tone";
import { Badge } from "@/components/ui/badge";

/** The issue status of a workitem, coloured by its status category; nothing when unknown. */
export function IssueStatusChip({ status, category }: { status: string; category: string }) {
  if (!status) return null;
  const group = issueCategory(category);
  return <Badge variant={group ? ISSUE_BADGE[group] : "outline"}>{status}</Badge>;
}
