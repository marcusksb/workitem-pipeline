import { ReactNode } from "react";
import { IconCircleCheck, IconGitMerge, IconThumbsUp, IconUserCheck } from "./icons";
import { Review } from "./types";

type Tone = "good" | "bad" | "wait" | "idle";

type Slot = { glyph: ReactNode; tone: Tone; label: string };

// One fixed glyph per dimension; the tone (color) carries the state. Shapes never
// change so the three slots are recognizable without reading.
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
    pending: { glyph: <IconCircleCheck />, tone: "wait", label: "CI running", },
    none: { glyph: <IconCircleCheck />, tone: "idle", label: "no CI" },
  };
}

/** Approval, merge and CI status of a PR, as a three-slot icon strip; a fourth slot leads it when I approved the PR. */
export function ReviewStrip({ review }: { review: Review | null }) {
  if (!review) return null;
  const slots = [
    ...(review.approved_by_me ? [{ glyph: <IconThumbsUp />, tone: "good" as Tone, label: "approved by you" }] : []),
    APPROVAL[review.approval],
    MERGE[review.merge],
    { ...ci(review.failed.length)[review.ci], spin: review.ci === "pending" },
  ];
  return (
    <div className="review-strip">
      {slots.map(({ glyph, tone, label, spin }: Slot & { spin?: boolean }) => (
        <span
          key={label}
          role="img"
          aria-label={label}
          title={label}
          className={`slot ${tone}${spin ? " spin" : ""}`}
        >
          {glyph}
        </span>
      ))}
    </div>
  );
}
