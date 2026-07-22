import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "integrations" / "blender" / "hot_trimmer_companion" / "tests" / "fixtures" / "behavioral.hottrim.json"
SERVER = ROOT / "integrations" / "blender" / "hot_trimmer_mcp.py"


class McpServerTests(unittest.TestCase):
    def test_initialize_list_and_fit_over_stdio(self):
        requests = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "fit_uv_island", "arguments": {"path": str(FIXTURE), "points": [[0, 0], [1, 0], [1, 0.5], [0, 0.5]]}}},
        ]
        result = subprocess.run([sys.executable, "-u", str(SERVER)], input="\n".join(json.dumps(request) for request in requests) + "\n", text=True, capture_output=True, cwd=ROOT, check=True)
        responses = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(responses[0]["result"]["serverInfo"]["name"], "hot-trimmer")
        names = {tool["name"] for tool in responses[1]["result"]["tools"]}
        self.assertTrue({"validate_package", "inspect_package", "fit_uv_island", "inspect_project", "debug_project", "create_region", "load_texture", "apply_edge_detail", "export_package"} <= names)
        self.assertFalse(responses[2]["result"].get("isError", False))
        self.assertTrue(responses[2]["result"]["structuredContent"]["transformedPoints"])


if __name__ == "__main__":
    unittest.main()
