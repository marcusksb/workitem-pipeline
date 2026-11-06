import { ActivityKind } from "./activity";

/** Icon and second-line quips of each activity toast. Edit freely: one is picked at random per toast. */
export const QUIPS: Record<ActivityKind, { icon: string; lines: string[] }> = {
  added: { icon: "🐣", lines: ["A new challenger appears.", "Fresh off the press.", "Another one for the pile.", "It hatched."] },
  removed: { icon: "🧹", lines: ["Swept away.", "Gone, but not forgotten. Mostly forgotten.", "Poof.", "Nothing to see here anymore."] },
  moved: { icon: "➡️", lines: ["On the move.", "One column closer to glory.", "Shuffling along nicely.", "Progress! Probably."] },
  done: { icon: "🎉", lines: ["Ship it. Oh wait, you did.", "Another one bites the dust.", "Take a sip of coffee, you earned it.", "Crushed it."] },
  approved: { icon: "✅", lines: ["Someone believes in you.", "Two thumbs, one PR.", "The reviewers have spoken.", "Stamp of approval, literally."] },
  changes_requested: { icon: "✏️", lines: ["Back to the drawing board. Just a little.", "Feedback is a gift. Apparently.", "So close.", "Opinions have arrived."] },
  ci_red: { icon: "🔴", lines: ["The robots have opinions.", "Red is a strong color.", "Something is on fire. Small fire.", "Computer says no."] },
  ci_green: { icon: "✨", lines: ["All green, all good.", "The robots are pleased.", "Tests passed. Nobody is more surprised than CI.", "Green means go."] },
  merged: { icon: "🚀", lines: ["Into main it goes. No take-backs.", "Liftoff.", "Merged and never looking back.", "It's out of your hands now."] },
  closed: { icon: "🪦", lines: ["It had a good run.", "Closed. Rest in peace, little PR.", "Not every branch makes it.", "Farewell."] },
  conflict: { icon: "⚔️", lines: ["Main moved on without you.", "Git wants a word.", "Two branches enter, one branch leaves.", "Conflict! Grab a rebase."] },
  jira: { icon: "🎫", lines: ["Jira has news.", "The ticket moved. The ticket always moves.", "Status update from the mothership.", "Somebody clicked a button in Jira."] },
  blocked: { icon: "✋", lines: ["Your agent is staring at you.", "Help wanted. By you.", "Paused, waiting for a human.", "It needs a hand."] },
};

export const quipFor = (kind: ActivityKind) => {
  const lines = QUIPS[kind].lines;
  return lines[Math.floor(Math.random() * lines.length)];
};
