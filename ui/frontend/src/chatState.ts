import { ChatEntry } from "./types";

/** A tool call with its result, once the transcript has it. */
export type ToolCall = { id: string; name: string; summary: string; result?: { output: string; error: boolean; more: number } };

/** What the Chat tab draws: text bubbles, and runs of tool calls between them as one group. */
export type ChatRow = { kind: "prompt" | "user" | "assistant"; text: string } | { kind: "tools"; calls: ToolCall[] };

export function chatRows(entries: ChatEntry[]): ChatRow[] {
  const rows: ChatRow[] = [];
  const calls = new Map<string, ToolCall>();
  for (const entry of entries) {
    if (entry.kind === "result") {
      const call = calls.get(entry.id);
      if (call) call.result = { output: entry.output, error: entry.error, more: entry.more };
    } else if (entry.kind === "tool") {
      const call: ToolCall = { id: entry.id, name: entry.name, summary: entry.summary };
      calls.set(entry.id, call);
      const last = rows[rows.length - 1];
      if (last?.kind === "tools") last.calls.push(call);
      else rows.push({ kind: "tools", calls: [call] });
    } else {
      rows.push({ kind: entry.kind, text: entry.text });
    }
  }
  return rows;
}
