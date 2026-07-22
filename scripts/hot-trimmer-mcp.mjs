import net from "node:net";
import readline from "node:readline";

const PORT = 39871;
const TOOLS = [
  ["inspect_project", "Inspect the currently open Hot Trimmer project and document state", {}],
  ["render_preview_1024", "Render the authoritative 1024x1024 preview for all requested maps", { revision: { type: "integer" }, requestedMaps: { type: "array" } }],
  ["debug_project", "Return the app's bounded Stage 15-20 diagnostic payload", { selectedRegionId: { type: ["string", "null"] }, requestedView: { type: "string" }, previewProfile: { type: "string" }, comparisonMode: { type: "string" }, activeTool: { type: "string" } }],
  ["create_region", "Create a region using a grid rectangle", { gridRect: { type: "object" } }],
  ["edit_region", "Edit a region with an allowlisted document command", { command: { type: "object" } }],
  ["list_templates", "List saved authored layout templates", {}],
  ["save_template", "Create or update an authored layout template", { preset: { type: "object" } }],
  ["apply_template", "Apply an authored layout template", { preset: { type: "object" }, instanceId: { type: "string" } }],
  ["delete_template", "Delete a user-authored layout template", { presetId: { type: "string" } }],
  ["load_texture", "Import or replace a texture channel", { path: { type: "string" }, sourceSetId: { type: "string" }, channel: { type: "string" }, normalConvention: { type: "string" } }],
  ["apply_edge_detail", "Apply the authoritative edge-detail intent", { intent: { type: "object" } }],
  ["adjust_edge_settings", "Adjust edge-detail settings", { intent: { type: "object" } }],
  ["export_package", "Run the native GPU export", { path: { type: "string" }, revision: { type: "integer" }, requestedMaps: { type: "array" } }],
].map(([name, description, properties]) => ({ name, description, inputSchema: { type: "object", properties } }));

function appCall(name, argumentsValue) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: "127.0.0.1", port: PORT });
    let buffer = "";
    socket.setTimeout(125000);
    socket.on("connect", () => socket.write(`${JSON.stringify({ id: `codex-${Date.now()}`, method: "call", name, arguments: argumentsValue })}\n`));
    socket.on("data", (chunk) => {
      buffer += chunk.toString("utf8");
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      const response = JSON.parse(buffer.slice(0, newline));
      socket.end();
      resolve(response);
    });
    socket.on("timeout", () => { socket.destroy(); reject(new Error("Hot Trimmer MCP bridge timed out")); });
    socket.on("error", (error) => reject(new Error(`Hot Trimmer is not running or MCP bridge is unavailable: ${error.message}`)));
  });
}

function mcpResult(value) {
  return { content: [{ type: "text", text: JSON.stringify(value) }], structuredContent: value };
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of input) {
  if (!line.trim()) continue;
  let request;
  try { request = JSON.parse(line); } catch { continue; }
  let response;
  if (request.method === "initialize") {
    response = { jsonrpc: "2.0", id: request.id, result: { protocolVersion: "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "hot-trimmer", version: "0.2.0" } } };
  } else if (request.method === "tools/list") {
    response = { jsonrpc: "2.0", id: request.id, result: { tools: TOOLS } };
  } else if (request.method === "tools/call") {
    try {
      const value = await appCall(request.params.name, request.params.arguments ?? {});
      response = { jsonrpc: "2.0", id: request.id, result: value.isError ? value : mcpResult(value) };
    } catch (error) {
      response = { jsonrpc: "2.0", id: request.id, result: { isError: true, content: [{ type: "text", text: error.message }] } };
    }
  } else if (request.method?.startsWith("notifications/")) {
    continue;
  } else {
    response = { jsonrpc: "2.0", id: request.id, error: { code: -32601, message: `method not found: ${request.method}` } };
  }
  process.stdout.write(`${JSON.stringify(response)}\n`);
}
