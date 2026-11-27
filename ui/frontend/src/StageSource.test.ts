import { describe, expect, it } from "vitest";

import { shownTab } from "./StageSource";

const none = { prose: false, script: false, actions: false };

describe("stage dialog What runs tab", () => {
  it("keeps the chosen tab while the stage has it", () => {
    expect(shownTab("actions", { prose: true, script: true, actions: true })).toBe("actions");
    expect(shownTab("script", { ...none, script: true })).toBe("script");
  });

  it("falls back to skill / prompt, then script, then actions", () => {
    expect(shownTab(undefined, { prose: true, script: true, actions: true })).toBe("prose");
    expect(shownTab("actions", { ...none, script: true })).toBe("script");
    expect(shownTab(undefined, { ...none, actions: true })).toBe("actions");
  });

  it("shows none when the stage runs nothing", () => {
    expect(shownTab("prose", none)).toBeUndefined();
  });
});
