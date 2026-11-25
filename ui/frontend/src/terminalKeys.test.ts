import { describe, expect, it } from "vitest";
import { isBrowserTabShortcut } from "./terminalKeys";

const key = (key: string, mods: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {}) => ({
  key,
  ctrlKey: !!mods.ctrl,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
  metaKey: !!mods.meta,
});

describe("isBrowserTabShortcut", () => {
  it("leaves the browser's tab switching keys to the browser", () => {
    expect(isBrowserTabShortcut(key("Tab", { ctrl: true }))).toBe(true);
    expect(isBrowserTabShortcut(key("Tab", { ctrl: true, shift: true }))).toBe(true);
    expect(isBrowserTabShortcut(key("PageUp", { ctrl: true }))).toBe(true);
    expect(isBrowserTabShortcut(key("PageDown", { ctrl: true }))).toBe(true);
  });

  it("keeps the keys the agent and the history scroll need", () => {
    expect(isBrowserTabShortcut(key("Tab"))).toBe(false);
    expect(isBrowserTabShortcut(key("Tab", { shift: true }))).toBe(false);
    expect(isBrowserTabShortcut(key("Tab", { alt: true }))).toBe(false);
    expect(isBrowserTabShortcut(key("Tab", { ctrl: true, alt: true }))).toBe(false);
    expect(isBrowserTabShortcut(key("c", { ctrl: true }))).toBe(false);
    expect(isBrowserTabShortcut(key("PageUp", { shift: true }))).toBe(false);
    expect(isBrowserTabShortcut(key("PageUp", { ctrl: true, shift: true }))).toBe(false);
  });
});
