"""Small, dependency-free MCP server for Hot Trimmer packages.

The server uses MCP's JSON-RPC stdio transport and deliberately delegates
manifest validation and fitting to the Blender companion's existing code.
It does not open or mutate a Blender scene.
"""

from __future__ import annotations

import json
import math
import socket
import sys
from pathlib import Path

from hot_trimmer_companion.fit import IslandDescriptor, choose_slot, transform_uvs
from hot_trimmer_companion.manifest import load_manifest, slots


SERVER_INFO = {"name": "hot-trimmer", "version": "0.1.0"}
PROTOCOL_VERSION = "2024-11-05"


def _json(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def _slot_view(slot):
    return {
        "slotId": slot.slot_id,
        "regionId": slot.region_id,
        "role": slot.role,
        "behaviorRole": slot.behavior_role,
        "uvFitKind": slot.uv_fit_kind,
        "fitAxis": slot.fit_axis,
        "normalizedHotspotRect": slot.normalized_hotspot_rect,
        "keepProportion": slot.keep_proportion,
        "allowedRotations": list(slot.allowed_rotations),
        "mirrorAllowed": slot.mirror_allowed,
        "classificationTags": list(slot.classification_tags),
        "worldSizeMeters": list(slot.world_size_meters),
        "variationGroup": slot.variation_group,
        "enabled": slot.enabled,
        "sampling": slot.sampling,
        "orientation": slot.orientation,
    }


def _load(arguments):
    path = arguments.get("path")
    if not isinstance(path, str) or not path.strip():
        raise ValueError("path must be a non-empty package directory or manifest path")
    return load_manifest(Path(path))


def _inspect(arguments):
    manifest = _load(arguments)
    requested_kind = arguments.get("fitKind")
    requested_role = arguments.get("behaviorRole")
    projected = [slot for slot in slots(manifest) if slot.enabled]
    if requested_kind is not None:
        if requested_kind not in {"rectangular", "radial"}:
            raise ValueError("fitKind must be rectangular or radial")
        projected = [slot for slot in projected if slot.uv_fit_kind == requested_kind]
    if requested_role is not None:
        projected = [slot for slot in projected if slot.behavior_role == requested_role]
    return {
        "manifestPath": str(manifest["_manifest_path"]),
        "packagePath": str(manifest["_package_path"]),
        "projectId": manifest["projectId"],
        "materialId": manifest["materialId"],
        "materialName": manifest["materialName"],
        "template": {"id": manifest["templateId"], "version": manifest["templateVersion"]},
        "compatibilityKey": manifest["compatibilityKey"],
        "materialRevision": manifest["materialRevision"],
        "outputSize": manifest["outputSize"],
        "normalOrientation": manifest["normalOrientation"],
        "maps": sorted(manifest["maps"]),
        "slotCount": len(slots(manifest)),
        "enabledSlotCount": sum(slot.enabled for slot in slots(manifest)),
        "slots": [_slot_view(slot) for slot in projected],
    }


def _fit(arguments):
    manifest = _load(arguments)
    points = arguments.get("points")
    if not isinstance(points, list) or len(points) < 3:
        raise ValueError("points must contain at least three [u, v] pairs")
    try:
        points = tuple((float(point[0]), float(point[1])) for point in points)
    except (TypeError, IndexError, ValueError) as error:
        raise ValueError("points must contain numeric [u, v] pairs") from error
    if any(not math.isfinite(value) for point in points for value in point):
        raise ValueError("points must contain finite coordinates")
    world_area = arguments.get("worldArea", 1.0)
    if isinstance(world_area, bool) or not isinstance(world_area, (int, float)) or not math.isfinite(world_area) or world_area <= 0:
        raise ValueError("worldArea must be a positive finite number")
    bounds = (min(point[0] for point in points), min(point[1] for point in points), max(point[0] for point in points), max(point[1] for point in points))
    width, height = bounds[2] - bounds[0], bounds[3] - bounds[1]
    uv_area = abs(sum(points[i][0] * points[(i + 1) % len(points)][1] - points[(i + 1) % len(points)][0] * points[i][1] for i in range(len(points))) / 2.0)
    descriptor = IslandDescriptor(
        uv_bounds=bounds,
        uv_aspect=width / height if height else 0.0,
        uv_area=uv_area,
        world_area=float(world_area),
        long_axis_orientation="U" if width >= height else "V",
        boundary_closed=bool(arguments.get("boundaryClosed", True)),
        circularity=float(arguments.get("circularity", 0.0)),
    )
    match = choose_slot(descriptor, slots(manifest), arguments.get("fitKind", "AUTO"), arguments.get("requestedSlotId", ""), int(arguments.get("variationIndex", 0)), bool(arguments.get("distribute", False)))
    transformed = transform_uvs(points, match)
    return {"classification": match.classification, "slot": _slot_view(match.slot), "rotation": match.rotation, "mirror": match.mirror, "transformedPoints": [list(point) for point in transformed]}


TOOLS = [
    {"name": "validate_package", "description": "Validate a Hot Trimmer package and return its authoritative identity.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "description": "Package directory or manifest.hottrim.json path"}}, "required": ["path"]}},
    {"name": "inspect_package", "description": "Inspect validated package metadata and enabled hotspot slots.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "fitKind": {"type": "string", "enum": ["rectangular", "radial"]}, "behaviorRole": {"type": "string"}}, "required": ["path"]}},
    {"name": "fit_uv_island", "description": "Choose a compatible hotspot and fit a UV polygon into it deterministically.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "points": {"type": "array", "items": {"type": "array", "minItems": 2, "maxItems": 2}}, "worldArea": {"type": "number", "exclusiveMinimum": 0}, "fitKind": {"type": "string", "enum": ["AUTO", "RECTANGULAR", "RADIAL"]}, "requestedSlotId": {"type": "string"}, "variationIndex": {"type": "integer", "minimum": 0}, "distribute": {"type": "boolean"}, "boundaryClosed": {"type": "boolean"}, "circularity": {"type": "number", "minimum": 0, "maximum": 1}}, "required": ["path", "points"]}},
]

APP_TOOLS = [
    {"name": "inspect_project", "description": "Inspect the currently open Hot Trimmer project and document state.", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "render_preview_1024", "description": "Render the authoritative 1024x1024 preview for all requested maps and return tile payloads as base64.", "inputSchema": {"type": "object", "properties": {"revision": {"type": "integer"}, "requestedMaps": {"type": "array", "items": {"type": "string"}}}}},
    {"name": "debug_project", "description": "Return the app's bounded Stage 15-20 diagnostic payload.", "inputSchema": {"type": "object", "properties": {"selectedRegionId": {"type": ["string", "null"]}, "requestedView": {"type": "string"}, "previewProfile": {"type": "string"}, "comparisonMode": {"type": "string"}, "activeTool": {"type": "string"}}}},
    {"name": "create_region", "description": "Create a region in the current authored source-frame document using a grid rectangle.", "inputSchema": {"type": "object", "properties": {"gridRect": {"type": "object", "properties": {"x": {"type": "integer"}, "y": {"type": "integer"}, "width": {"type": "integer"}, "height": {"type": "integer"}}, "required": ["x", "y", "width", "height"]}}, "required": ["gridRect"]}},
    {"name": "edit_region", "description": "Edit a region with an allowlisted document command (split, merge, move, resize, content, projection, radial, behavior, or output settings).", "inputSchema": {"type": "object", "properties": {"command": {"type": "object", "description": "TrimSheetDocumentCommand region edit"}}, "required": ["command"]}},
    {"name": "list_templates", "description": "List saved authored layout templates.", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "save_template", "description": "Create or update an authored layout template.", "inputSchema": {"type": "object", "properties": {"preset": {"type": "object"}}, "required": ["preset"]}},
    {"name": "apply_template", "description": "Apply an authored layout template to the current document.", "inputSchema": {"type": "object", "properties": {"preset": {"type": "object"}, "instanceId": {"type": "string"}}, "required": ["preset"]}},
    {"name": "delete_template", "description": "Delete a user-authored layout template.", "inputSchema": {"type": "object", "properties": {"presetId": {"type": "string"}}, "required": ["presetId"]}},
    {"name": "load_texture", "description": "Import or replace a texture channel in the current material source set.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "sourceSetId": {"type": "string"}, "channel": {"type": "string", "enum": ["base_color", "normal", "height", "roughness", "metallic", "ambient_occlusion", "specular", "opacity", "edge_mask", "material_id"]}, "normalConvention": {"type": "string", "enum": ["opengl", "directx", "not_applicable"]}}, "required": ["path", "sourceSetId", "channel"]}},
    {"name": "apply_edge_detail", "description": "Apply the authoritative edge-detail intent to the current document.", "inputSchema": {"type": "object", "properties": {"intent": {"type": "object", "description": "EdgeDetailIntentV1 payload from the Hot Trimmer contract"}}, "required": ["intent"]}},
    {"name": "adjust_edge_settings", "description": "Adjust edge-detail settings using the authoritative EdgeDetailIntentV1 contract.", "inputSchema": {"type": "object", "properties": {"intent": {"type": "object"}}, "required": ["intent"]}},
    {"name": "export_package", "description": "Run the native GPU export for the current document.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "revision": {"type": "integer"}, "requestedMaps": {"type": "array", "items": {"type": "string"}}}, "required": ["path", "revision"]}},
]


def _app_call(name, arguments):
    request_id = f"mcp-{id(arguments)}"
    payload = _json({"id": request_id, "method": "call", "name": name, "arguments": arguments}) + "\n"
    with socket.create_connection(("127.0.0.1", 39871), timeout=125) as connection:
        connection.sendall(payload.encode("utf-8"))
        data = b""
        while not data.endswith(b"\n"):
            chunk = connection.recv(1024 * 1024)
            if not chunk:
                break
            data += chunk
    if not data:
        raise RuntimeError("Hot Trimmer MCP bridge returned no response; launch the app first")
    response = json.loads(data.decode("utf-8"))
    if response.get("isError"):
        raise ValueError(response.get("error", "Hot Trimmer rejected the MCP request"))
    return response


def _result(value):
    return {"content": [{"type": "text", "text": _json(value)}], "structuredContent": value}


def handle(request):
    method, request_id = request.get("method"), request.get("id")
    if method == "initialize":
        return {"jsonrpc": "2.0", "id": request_id, "result": {"protocolVersion": PROTOCOL_VERSION, "capabilities": {"tools": {}}, "serverInfo": SERVER_INFO}}
    if method == "ping":
        return {"jsonrpc": "2.0", "id": request_id, "result": {}}
    if method == "tools/list":
        return {"jsonrpc": "2.0", "id": request_id, "result": {"tools": TOOLS + APP_TOOLS}}
    if method == "tools/call":
        name = request.get("params", {}).get("name")
        arguments = request.get("params", {}).get("arguments", {})
        try:
            if name in {tool["name"] for tool in APP_TOOLS}:
                return {"jsonrpc": "2.0", "id": request_id, "result": _result(_app_call(name, arguments))}
            value = {"valid": True, "manifest": _inspect(arguments)} if name == "validate_package" else _inspect(arguments) if name == "inspect_package" else _fit(arguments) if name == "fit_uv_island" else None
            if value is None:
                raise ValueError(f"unknown tool: {name}")
            return {"jsonrpc": "2.0", "id": request_id, "result": _result(value)}
        except Exception as error:
            return {"jsonrpc": "2.0", "id": request_id, "result": {"isError": True, "content": [{"type": "text", "text": str(error)}]}}
    if method and method.startswith("notifications/"):
        return None
    return {"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": f"method not found: {method}"}}


def main():
    for line in sys.stdin:
        try:
            response = handle(json.loads(line))
            if response is not None:
                sys.stdout.write(_json(response) + "\n")
                sys.stdout.flush()
        except (json.JSONDecodeError, TypeError) as error:
            sys.stdout.write(_json({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": str(error)}}) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
