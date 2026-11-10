// Shared by the pi and omp glue (decision 56): neither reads an MCP server from a flag, so the glue
// offers the tools of the item's `workitem` MCP server (decision 61, ask_parent and the others) as
// its own. The launcher sets WORKITEM_MCP to Claude's --mcp-config file (mode 0600, with the
// token); the tools come from the server, so they stay the same as Claude's. Plain JavaScript in a
// .ts file, like notify.ts.
import { readFileSync } from "node:fs";

/** Registers the server's tools with `pi.registerTool`; nothing without WORKITEM_MCP. */
export async function registerMcpTools(pi) {
  const file = process.env.WORKITEM_MCP;
  if (!file) return;
  let server;
  try {
    server = Object.values(JSON.parse(readFileSync(file, "utf8")).mcpServers)[0];
  } catch {
    return;
  }
  const call = async (method, params, signal) => {
    const response = await fetch(server.url, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "application/json", ...server.headers },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
      signal,
    });
    const reply = await response.json();
    if (reply.error) throw new Error(reply.error.message);
    return reply.result;
  };
  let tools;
  try {
    ({ tools } = await call("tools/list", {}));
  } catch {
    return; // The backend is down: the session runs without them, as Claude's would.
  }
  for (const tool of tools) {
    pi.registerTool({
      name: tool.name,
      label: tool.name,
      description: tool.description,
      parameters: tool.inputSchema,
      async execute(_id, params, signal) {
        const result = await call("tools/call", { name: tool.name, arguments: params }, signal);
        const text = result.content.map((c) => c.text).join("\n");
        if (result.isError) throw new Error(text);
        return { content: [{ type: "text", text }], details: {} };
      },
    });
  }
}
