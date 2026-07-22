# Hot Trimmer MCP

`hot_trimmer_mcp.py` exposes the existing Hot Trimmer contracts over MCP's
JSON-RPC stdio transport. Package tools validate and fit exported packages.
When the desktop app is running, the same server also exposes live tools for
inspection, bounded debugging, region creation, texture loading, edge detail,
and native export. The app owns the live state and starts a loopback bridge on
`127.0.0.1:39871`.

Run it from the repository root:

```text
python -u integrations/blender/hot_trimmer_mcp.py
```

Example client configuration:

```json
{
  "mcpServers": {
    "hot-trimmer": {
      "command": "node",
      "args": ["F:/Playground/Blender/HotTrimmer/scripts/hot-trimmer-mcp.mjs"]
    }
  }
}
```

Launch Hot Trimmer before using the live tools. The proxy returns a clear
connection error if the app is not running. Live operations use the app's
existing revision, validation, GPU, and atomic-export guards.
