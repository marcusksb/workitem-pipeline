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

/** Badge variant of an issue status category. */
export const ISSUE_BADGE = { todo: "outline", progress: "info", done: "good" } as const;
