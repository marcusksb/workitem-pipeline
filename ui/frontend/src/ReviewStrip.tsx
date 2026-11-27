import { ReactNode } from "react";
import type { Tone } from "./boardState";
import { IconCircleCheck, IconGitMerge, IconGitPullRequest, IconUserCheck } from "./icons";
import { Review } from "./types";

type Slot = { glyph: ReactNode; tone: Tone; label: string; spin?: true };

// One fixed glyph per dimension; the tone (color) carries the state.
export const APPROVAL: Record<Review["approval"], Slot> = {
  approved: { glyph: <IconUserCheck />, tone: "good", label: "PR approved" },
  changes_requested: { glyph: <IconUserCheck />, tone: "bad", label: "changes requested" },
  review_required: { glyph: <IconUserCheck />, tone: "wait", label: "awaiting review" },
  none: { glyph: <IconUserCheck />, tone: "idle", label: "no review needed" },
};

export const MERGE: Record<Review["merge"], Slot> = {
  mergeable: { glyph: <IconGitMerge />, tone: "good", label: "no conflicts" },
  blocked: { glyph: <IconGitMerge />, tone: "wait", label: "no conflicts, merge blocked" },
  conflicting: { glyph: <IconGitMerge />, tone: "bad", label: "merge conflict" },
  unknown: { glyph: <IconGitMerge />, tone: "wait", label: "merge unknown" },
};

export function ci(failedCount: number): Record<Review["ci"], Slot> {
  return {
    passing: { glyph: <IconCircleCheck />, tone: "good", label: "CI passing" },
    failing: { glyph: <IconCircleCheck />, tone: "bad", label: `CI failing — ${failedCount} ${failedCount === 1 ? "check" : "checks"}` },
    pending: { glyph: <IconCircleCheck />, tone: "wait", label: "CI running", spin: true },
    none: { glyph: <IconCircleCheck />, tone: "idle", label: "no CI" },
  };
}

export const DONE: Record<Exclude<Review["state"], "open">, Slot> = {
  merged: { glyph: <IconGitMerge />, tone: "merged", label: "PR merged" },
  closed: { glyph: <IconGitPullRequest />, tone: "idle", label: "PR closed without merge" },
};
