import { describe, expect, it } from "vitest";
import { nextTheme } from "./theme";

describe("nextTheme", () => {
  it("switches away from the OS theme while none is picked", () => {
    expect(nextTheme(undefined, true)).toBe("light");
    expect(nextTheme(undefined, false)).toBe("dark");
  });

  it("switches away from the picked theme, whatever the OS says", () => {
    for (const osDark of [true, false]) {
      expect(nextTheme("dark", osDark)).toBe("light");
      expect(nextTheme("light", osDark)).toBe("dark");
    }
  });
});
