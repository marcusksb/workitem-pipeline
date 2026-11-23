import { ReactNode } from "react";
import type { Tone } from "./boardState";
import { IconCircleCheck, IconGitMerge, IconGitPullRequest, IconThumbsUp, IconUserCheck } from "./icons";
import { TONE_BADGE } from "./tone";
import { Review } from "./types";
import { Badge } from "@/components/ui/badge";

type Slot = { glyph: ReactNode; tone: Tone; label: string };

// One fixed glyph per dimension; the tone (color) carries the state.
const APPROVAL: Record<Review["approval"], Slot> = {
  approved: { glyph: <IconUserCheck />, tone: "good", label: "PR approved" },
  changes_requested: { glyph: <IconUserCheck />, tone: "bad", label: "changes requested" },
  review_required: { glyph: <IconUserCheck />, tone: "wait", label: "awaiting review" },
  none: { glyph: <IconUserCheck />, tone: "idle", label: "no review needed" },
};

const MERGE: Record<Review["merge"], Slot> = {
  mergeable: { glyph: <IconGitMerge />, tone: "good", label: "no conflicts" },
  conflicting: { glyph: <IconGitMerge />, tone: "bad", label: "merge conflict" },
  unknown: { glyph: <IconGitMerge />, tone: "wait", label: "merge unknown" },
};

function ci(failedCount: number): Record<Review["ci"], Slot> {
  return {
    passing: { glyph: <IconCircleCheck />, tone: "good", label: "CI passing" },
    failing: { glyph: <IconCircleCheck />, tone: "bad", label: `CI failing — ${failedCount} ${failedCount === 1 ? "check" : "checks"}` },
    pending: { glyph: <IconCircleCheck />, tone: "wait", label: "CI running" },
    none: { glyph: <IconCircleCheck />, tone: "idle", label: "no CI" },
  };
}

const DONE: Record<Exclude<Review["state"], "open">, Slot> = {
  merged: { glyph: <IconGitMerge />, tone: "merged", label: "PR merged" },
  closed: { glyph: <IconGitPullRequest />, tone: "idle", label: "PR closed without merge" },
};

/** Approval, merge and CI status of an open PR, one badge each; a fourth leads when I approved the PR. A merged or closed PR gets one badge. */
export function ReviewStrip({ review }: { review: Review | null }) {
  if (!review) return null;
  const slots = review.state !== "open" ? [DONE[review.state]] : [
    ...(review.approved_by_me ? [{ glyph: <IconThumbsUp />, tone: "good" as Tone, label: "approved by you" }] : []),
    APPROVAL[review.approval],
    MERGE[review.merge],
    ci(review.failed.length)[review.ci],
  ];
  return (
    <>
      {slots.map(({ glyph, tone, label }) => (
        <Badge key={label} variant={TONE_BADGE[tone]}>
          {glyph}
          {label}
        </Badge>
      ))}
    </>
  );
}
