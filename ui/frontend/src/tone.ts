import type { Tone } from "./boardState";

/** Text colour of a status tone. */
export const TONE_TEXT: Record<Tone, string> = {
  good: "text-good",
  bad: "text-blocked",
  wait: "text-working",
  merged: "text-merged",
  gate: "text-gate",
  idle: "text-muted-foreground",
};

/** Badge variant of a status tone. */
export const TONE_BADGE = { good: "good", bad: "danger", wait: "wait", merged: "merged", gate: "gate", idle: "outline" } as const satisfies Record<Tone, string>;

/** Dot colour of a stage, by its role on the bundled board; other stages are grey. */
export const STAGE_DOT: Record<string, string> = {
  backlog: "bg-stage-backlog",
  analyze: "bg-stage-analyze",
  ready_for_development: "bg-stage-ready",
  in_development: "bg-stage-development",
  in_review: "bg-stage-review",
  done: "bg-stage-done",
};
