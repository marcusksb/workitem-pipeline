import { describe, expect, it } from "vitest";
import { chatRows } from "./chatState";

describe("chatRows", () => {
  it("groups consecutive tool calls and attaches their results", () => {
    const rows = chatRows([
      { kind: "user", text: "fix it" },
      { kind: "tool", id: "a", name: "Read", summary: "src/x.rs" },
      { kind: "result", id: "a", output: "fn x", error: false, more: 0 },
      { kind: "tool", id: "b", name: "Bash", summary: "cargo test" },
      { kind: "assistant", text: "Done." },
      { kind: "result", id: "b", output: "ok", error: true, more: 3 },
    ]);
    expect(rows).toEqual([
      { kind: "user", text: "fix it" },
      {
        kind: "tools",
        calls: [
          { id: "a", name: "Read", summary: "src/x.rs", result: { output: "fn x", error: false, more: 0 } },
          { id: "b", name: "Bash", summary: "cargo test", result: { output: "ok", error: true, more: 3 } },
        ],
      },
      { kind: "assistant", text: "Done." },
    ]);
  });

  it("ignores a result whose call is not in the entries", () => {
    expect(chatRows([{ kind: "result", id: "x", output: "", error: false, more: 0 }])).toEqual([]);
  });
});
