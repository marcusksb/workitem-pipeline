import { ActivityKind } from "./activity";

/** Icon and second-line quips of each activity toast. Edit freely: one is picked at random per toast. */
export const QUIPS: Record<ActivityKind, { icon: string; lines: string[] }> = {
  approved: { icon: "✅", lines: ["Someone believes in you.", "Two thumbs, one PR.", "The reviewers have spoken.", "Stamp of approval, literally."] },
  blocked: { icon: "✋", lines: ["Your agent is staring at you.", "Help wanted. By you.", "Paused, waiting for a human.", "It needs a hand."] },
};

export const quipFor = (kind: ActivityKind) => {
  const lines = QUIPS[kind].lines;
  return lines[Math.floor(Math.random() * lines.length)];
};
