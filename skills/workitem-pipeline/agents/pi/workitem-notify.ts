// pi extension for pipeline sessions (decision 56), loaded with `-e` on each launch. pi has no
// permission prompts, so only a stopped agent counts. In a planning session it also guards edits,
// asks the human before shell commands and marks the record working, like Claude's planning
// hooks and permission prompts. The launcher picks the session id
// (`--session-id`). In an item session it offers the workitem MCP server's tools. Outside a
// pipeline session it does nothing.
import { registerMcpTools } from "../mcp.ts";
import { askedCommand, idleReporter, notify, planNote, refusedEdit, resume } from "../notify.ts";

export default async function (pi) {
  const reporter = idleReporter(1000);
  pi.on("agent_start", () => {
    reporter.busy();
    return Promise.all([resume(), planNote("--status", "working")]);
  });
  pi.on("agent_end", () => reporter.idle());
  pi.on("tool_call", async (event, ctx) => {
    const reason = await refusedEdit(event?.toolName, event?.input, ctx?.cwd ?? process.cwd());
    if (reason) return { block: true, reason };
    const command = askedCommand(event?.toolName, event?.input);
    if (command === null) return;
    notify("permission_prompt");
    const allowed = ctx?.hasUI ? await ctx.ui.confirm("Planning session: run this shell command?", command) : false;
    await planNote("--status", "working");
    if (!allowed) return { block: true, reason: "The human did not allow this shell command; planning sessions do not change code." };
  });
  await registerMcpTools(pi);
}
