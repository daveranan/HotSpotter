"""Blender operators for importing and applying Hot Trimmer hotspots."""

import json
import hashlib
import math
import time

import bmesh
import bpy
from bpy.props import BoolProperty, EnumProperty, StringProperty
from bpy.types import Operator, Panel
from bpy.app.handlers import persistent
from mathutils import Vector

from .fit import (
    EPSILON,
    IslandDescriptor,
    Match,
    bounds,
    choose_slot,
    circularity_estimate,
    island_behavior_role,
    points_inside_slot,
    polygon_signed_area,
    transform_uvs,
)
from .manifest import load_manifest, slots
from .materials import create_or_update_material


CLASSIFICATION_ITEMS = (
    ("AUTO", "Auto", "Choose rectangular or radial from supported island evidence"),
    ("RECTANGULAR", "Rectangular", "Use only rectangular hotspots"),
    ("RADIAL", "Radial", "Use only radial hotspots for an already circular UV island"),
)

ASSIGNMENT_ALGORITHM_VERSION = 6
_AUTO_UPDATING = False
_AUTO_PENDING = set()
_AUTO_DUE_TIME = 0.0
_AUTO_DEBOUNCE_SECONDS = 0.35
_AUTO_WATCH_INTERVAL_SECONDS = 0.25


def _result(context, message):
    context.scene["ht_last_result"] = message


def _fail(operator, context, message):
    _result(context, message)
    operator.report({"ERROR"}, message)
    return {"CANCELLED"}


def _material_for_manifest(manifest):
    return next((material for material in bpy.data.materials if material.get("ht_material_id") == manifest["materialId"]), None)


def _classification_override(value):
    value = (value or "").lower()
    if value == "radial":
        return "RADIAL"
    if value == "rectangular":
        return "RECTANGULAR"
    return "AUTO"


def _safe_face_index(value):
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _assigned_indices(assignments):
    return tuple(sorted(index for value in assignments for index in (_safe_face_index(value),) if index is not None and index >= 0))


def _updated_mesh_objects_from_depsgraph(depsgraph):
    if depsgraph is None:
        return ()
    objects = []
    seen = set()
    for update in getattr(depsgraph, "updates", ()):
        if not getattr(update, "is_updated_geometry", False):
            continue
        source = getattr(update, "id", None)
        candidates = []
        if isinstance(source, bpy.types.Object) and getattr(source, "type", None) == "MESH":
            candidates = [source]
        elif isinstance(source, bpy.types.Mesh):
            for candidate in bpy.data.objects:
                if candidate.type == "MESH" and candidate.data == source:
                    candidates.append(candidate)
        for candidate in candidates:
            key = candidate.as_pointer()
            if key not in seen:
                seen.add(key)
                objects.append(candidate)
    return tuple(objects)


def _objects_with_current_assignments(scene):
    if scene is None:
        return ()
    objects = []
    for obj in tuple(scene.objects):
        if obj.type != "MESH":
            continue
        mesh = obj.data
        raw_assignments = mesh.get("ht_assignments", "")
        if raw_assignments not in ("", "{}", None):
            try:
                assignments = json.loads(raw_assignments)
            except (TypeError, json.JSONDecodeError):
                assignments = {}
            if isinstance(assignments, dict) and assignments:
                objects.append(obj)
    return tuple(objects)


def _mesh_geometry_signature(obj):
    """Stable topology/position/island-boundary signature; ignores UV/material edits."""
    if obj.mode == "EDIT":
        bm = bmesh.from_edit_mesh(obj.data)
        bm.verts.ensure_lookup_table()
        bm.edges.ensure_lookup_table()
        bm.faces.ensure_lookup_table()
        vertices = tuple((round(vertex.co.x, 7), round(vertex.co.y, 7), round(vertex.co.z, 7)) for vertex in bm.verts)
        faces = tuple(tuple(vertex.index for vertex in face.verts) for face in bm.faces)
        boundaries = tuple((bool(edge.seam), not bool(edge.smooth)) for edge in bm.edges)
        counts = (len(bm.verts), len(bm.edges), len(bm.faces))
    else:
        mesh = obj.data
        vertices = tuple((round(vertex.co.x, 7), round(vertex.co.y, 7), round(vertex.co.z, 7)) for vertex in mesh.vertices)
        faces = tuple(tuple(polygon.vertices) for polygon in mesh.polygons)
        boundaries = tuple((bool(edge.use_seam), bool(getattr(edge, "use_edge_sharp", False))) for edge in mesh.edges)
        counts = (len(mesh.vertices), len(mesh.edges), len(mesh.polygons))
    payload = repr((counts, vertices, faces, boundaries)).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _store_geometry_signature(obj):
    signature = _mesh_geometry_signature(obj)
    obj.data["ht_geometry_signature"] = signature
    return signature


def _schedule_auto_update(obj, scene):
    global _AUTO_DUE_TIME
    if obj is None or obj.type != "MESH" or not obj.data.get("ht_assignments"):
        return
    already_pending = obj.name in _AUTO_PENDING
    _AUTO_PENDING.add(obj.name)
    # The polling fallback observes the same changed signature repeatedly
    # until the queued rebuild completes.  Only the first observation starts
    # the debounce; resetting it on every poll would postpone the update
    # forever because the poll interval is shorter than the debounce.
    if not already_pending:
        _AUTO_DUE_TIME = time.monotonic() + _AUTO_DEBOUNCE_SECONDS
    if scene is not None:
        scene["ht_last_result"] = f"Live hotspot pending: {obj.name}"
    if not bpy.app.timers.is_registered(run_pending_auto_updates):
        bpy.app.timers.register(run_pending_auto_updates, first_interval=_AUTO_DEBOUNCE_SECONDS, persistent=True)


def _record_for_faces(assignments, faces):
    record = None
    for face_index in faces:
        candidate = assignments.get(str(face_index))
        if candidate:
            record = candidate
            break
    return record


def _build_assignment_record(existing_record, manifest, match, descriptor, rectified_quad):
    return {
        "slotId": match.slot.slot_id,
        "regionId": match.slot.region_id,
        "rotation": match.rotation,
        "mirror": match.mirror,
        "classification": match.classification,
        "islandRole": island_behavior_role(descriptor, match.classification),
        "slotBehaviorRole": match.slot.behavior_role,
        "compatibilityKey": manifest["compatibilityKey"],
        "templateSnapshotHash": manifest["templateSnapshotHash"],
        "algorithmVersion": ASSIGNMENT_ALGORITHM_VERSION,
        "variationCycle": (existing_record or {}).get("variationCycle", 0),
        "rectifiedQuad": bool(rectified_quad),
    }


def _refresh_object_assignments(obj, manifest, material, context=None):
    mesh = obj.data
    if context is not None and obj.mode == "EDIT":
        active = getattr(context.view_layer.objects, "active", None)
        if active == obj:
            try:
                bmesh.update_edit_mesh(mesh, loop_triangles=False, destructive=False)
            except Exception:
                pass
    assignments = {key: value for key, value in _assignment_map(mesh).items() if _assignment_is_current(value, manifest)}
    if not assignments:
        return False
    indices = tuple(index for index in _assigned_indices(assignments) if index < len(mesh.polygons))
    if not indices:
        mesh["ht_assignments"] = json.dumps({}, separators=(",", ":"), sort_keys=True)
        return False
    uv_layer = mesh.uv_layers.active
    if uv_layer is None:
        return False
    usable = tuple(index for index in indices if _has_usable_uvs(mesh, uv_layer, (index,)))
    if not usable:
        return False
    islands, edge_faces, adjacency = _form_islands(mesh, uv_layer, usable)
    if not islands:
        return False
    updated = {key: assignments[key] for key in (str(index) for index in indices) if key in assignments}
    material_index = None
    changed = False
    for island in islands:
        descriptor, _ = _descriptor(obj, uv_layer, island, edge_faces, adjacency, assignments, manifest)
        existing_record = _record_for_faces(assignments, island)
        override = _classification_override((existing_record or {}).get("classification"))
        slot_id = (existing_record or {}).get("slotId")
        rectified_quad = bool((existing_record or {}).get("rectifiedQuad"))
        variation_cycle = int((existing_record or {}).get("variationCycle", 0))
        try:
            match = choose_slot(descriptor, slots(manifest), override, slot_id, variation_cycle, distribute=False)
        except Exception:
            try:
                match = choose_slot(descriptor, slots(manifest), "AUTO", "", variation_cycle, distribute=False)
            except Exception:
                continue
        if existing_record:
            rotation = existing_record.get("rotation")
            mirror = existing_record.get("mirror")
            if isinstance(rotation, int) and rotation != match.rotation:
                match = Match(match.slot, rotation, match.mirror, match.classification)
            if isinstance(mirror, bool) and mirror != match.mirror:
                match = Match(match.slot, match.rotation, mirror, match.classification)
        points = [tuple(uv_layer.data[loop_index].uv) for face in island for loop_index in mesh.polygons[face].loop_indices]
        try:
            fitted = transform_uvs(points, match, fill_rect=rectified_quad)
        except Exception:
            continue
        if not points_inside_slot(fitted, match.slot):
            continue
        cursor = 0
        for face_index in island:
            for loop_index in mesh.polygons[face_index].loop_indices:
                uv_layer.data[loop_index].uv = fitted[cursor]
                cursor += 1
        if material_index is None:
            material_index = _assign_material(mesh, material, island)
            obj.active_material_index = material_index
        else:
            _assign_material(mesh, material, island)
        for face_index in island:
            updated[str(face_index)] = _build_assignment_record(existing_record, manifest, match, descriptor, rectified_quad)
        changed = True
    if changed:
        mesh["ht_assignments"] = json.dumps(updated, separators=(",", ":"), sort_keys=True)
        mesh["ht_compatibility_key"] = manifest["compatibilityKey"]
    return changed


@persistent
def hottrimmer_auto_update_handler(scene, depsgraph):
    if _AUTO_UPDATING:
        return
    if scene is None or not getattr(scene, "hottrimmer_live_update", True):
        return
    if not scene.get("ht_manifest_path"):
        return
    targets = _updated_mesh_objects_from_depsgraph(depsgraph)
    for obj in targets:
        if not obj.data.get("ht_assignments"):
            continue
        current = _mesh_geometry_signature(obj)
        stored = obj.data.get("ht_geometry_signature")
        if not stored:
            obj.data["ht_geometry_signature"] = current
        elif stored != current:
            _schedule_auto_update(obj, scene)


def poll_auto_updates():
    """Persistent fallback for Edit Mode tools that omit depsgraph mesh updates."""
    if _AUTO_UPDATING:
        return _AUTO_WATCH_INTERVAL_SECONDS
    context = getattr(bpy, "context", None)
    scene = getattr(context, "scene", None) if context is not None else None
    if scene is None or not getattr(scene, "hottrimmer_live_update", True) or not scene.get("ht_manifest_path"):
        return _AUTO_WATCH_INTERVAL_SECONDS
    for obj in _objects_with_current_assignments(scene):
        try:
            current = _mesh_geometry_signature(obj)
        except (ReferenceError, RuntimeError):
            continue
        stored = obj.data.get("ht_geometry_signature")
        if not stored:
            obj.data["ht_geometry_signature"] = current
        elif stored != current:
            _schedule_auto_update(obj, scene)
    return _AUTO_WATCH_INTERVAL_SECONDS


def start_auto_updates():
    if not bpy.app.timers.is_registered(poll_auto_updates):
        bpy.app.timers.register(poll_auto_updates, first_interval=_AUTO_WATCH_INTERVAL_SECONDS, persistent=True)


def run_pending_auto_updates(force=False):
    """Debounced timer callback; public for the headless behavioral fixture."""
    global _AUTO_UPDATING
    if _AUTO_UPDATING:
        return _AUTO_DEBOUNCE_SECONDS
    if not _AUTO_PENDING:
        return None
    remaining = _AUTO_DUE_TIME - time.monotonic()
    if not force and remaining > 0.0:
        return max(0.05, remaining)
    context = getattr(bpy, "context", None)
    scene = getattr(context, "scene", None) if context is not None else None
    if context is None or scene is None or not getattr(scene, "hottrimmer_live_update", True):
        _AUTO_PENDING.clear()
        return None
    active = context.view_layer.objects.active
    pending_objects = [bpy.data.objects.get(name) for name in tuple(sorted(_AUTO_PENDING))]
    pending_objects = [obj for obj in pending_objects if obj is not None and obj.name in context.view_layer.objects]
    if active is not None and active.mode == "EDIT":
        pending_objects = [obj for obj in pending_objects if obj == active]
        if not pending_objects:
            return _AUTO_DEBOUNCE_SECONDS
    _AUTO_UPDATING = True
    try:
        for obj in pending_objects:
            original_active = context.view_layer.objects.active
            original_selected = tuple(context.selected_objects)
            original_mode = original_active.mode if original_active is not None else "OBJECT"
            try:
                if original_active is not None and original_active.mode != "OBJECT" and original_active != obj:
                    continue
                if original_active is None or original_active.mode == "OBJECT":
                    for candidate in context.selected_objects:
                        candidate.select_set(False)
                    obj.select_set(True)
                    context.view_layer.objects.active = obj
                result = bpy.ops.hottrimmer.fit_selected(
                    classification=getattr(scene, "hottrimmer_classification", "AUTO"),
                    process_all_faces=True,
                    best_effort=True,
                    live_update=True,
                )
                if result == {"FINISHED"}:
                    _store_geometry_signature(obj)
                    scene["ht_last_result"] = f"Live hotspot updated: {obj.name}"
            except Exception as error:
                scene["ht_last_result"] = f"Live hotspot best-effort warning: {obj.name}: {error}"
            finally:
                if original_active is None or original_mode == "OBJECT":
                    for candidate in context.selected_objects:
                        candidate.select_set(False)
                    for candidate in original_selected:
                        if candidate.name in context.view_layer.objects:
                            candidate.select_set(True)
                    if original_active is not None and original_active.name in context.view_layer.objects:
                        context.view_layer.objects.active = original_active
            _AUTO_PENDING.discard(obj.name)
    finally:
        _AUTO_UPDATING = False
    return _AUTO_DEBOUNCE_SECONDS if _AUTO_PENDING else None


def stop_auto_updates():
    _AUTO_PENDING.clear()
    if bpy.app.timers.is_registered(run_pending_auto_updates):
        bpy.app.timers.unregister(run_pending_auto_updates)
    if bpy.app.timers.is_registered(poll_auto_updates):
        bpy.app.timers.unregister(poll_auto_updates)


def _capture_target_faces(context):
    active = context.view_layer.objects.active
    if active is not None and active.mode == "EDIT":
        targets = {}
        for obj in context.objects_in_mode:
            if obj.type != "MESH":
                continue
            bm = bmesh.from_edit_mesh(obj.data)
            bm.faces.ensure_lookup_table()
            polygon_count = len(obj.data.polygons)
            selected = tuple(face.index for face in bm.faces if face.select and 0 <= face.index < polygon_count)
            if selected:
                targets[obj] = selected
        if not targets and active and active.type == "MESH":
            targets[active] = tuple(range(len(active.data.polygons)))
        return targets
    if active is not None and active.type == "MESH" and not context.selected_objects:
        return {active: tuple(range(len(active.data.polygons)))}
    return {obj: tuple(range(len(obj.data.polygons))) for obj in context.selected_objects if obj.type == "MESH" and obj.data.polygons}


def _valid_faces(mesh, face_indices):
    polygon_count = len(mesh.polygons)
    return tuple(face for face in face_indices if 0 <= face < polygon_count)


def _face_world_area(obj, polygon):
    points = [obj.matrix_world @ obj.data.vertices[index].co for index in polygon.vertices]
    origin = points[0]
    return sum((points[index] - origin).cross(points[index + 1] - origin).length * 0.5 for index in range(1, len(points) - 1))


def _uv_face_area(mesh, uv_layer, polygon):
    points = [tuple(uv_layer.data[index].uv) for index in polygon.loop_indices]
    return abs(polygon_signed_area(points))


def _has_usable_uvs(mesh, uv_layer, face_indices):
    if uv_layer is None:
        return False
    for face_index in face_indices:
        polygon = mesh.polygons[face_index]
        points = [tuple(uv_layer.data[index].uv) for index in polygon.loop_indices]
        if any(not math.isfinite(value) for point in points for value in point) or abs(polygon_signed_area(points)) <= EPSILON:
            return False
    return True


def _edge_is_sharp(edge):
    return bool(getattr(edge, "use_edge_sharp", False))


def _edge_uvs(mesh, uv_layer, polygon, edge_index):
    result = {}
    for loop_index in polygon.loop_indices:
        loop = mesh.loops[loop_index]
        if loop.edge_index == edge_index:
            next_loop_index = polygon.loop_start + ((loop_index - polygon.loop_start + 1) % polygon.loop_total)
            result[loop.vertex_index] = tuple(uv_layer.data[loop_index].uv)
            result[mesh.loops[next_loop_index].vertex_index] = tuple(uv_layer.data[next_loop_index].uv)
            break
    return result


def _uv_connected(mesh, uv_layer, first, second, edge_index):
    first_uvs = _edge_uvs(mesh, uv_layer, mesh.polygons[first], edge_index)
    second_uvs = _edge_uvs(mesh, uv_layer, mesh.polygons[second], edge_index)
    return first_uvs.keys() == second_uvs.keys() and all(math.dist(first_uvs[key], second_uvs[key]) <= 1.0e-6 for key in first_uvs)


def _form_islands(mesh, uv_layer, face_indices):
    selected = set(face_indices)
    edge_faces = {}
    for face_index in face_indices:
        for loop_index in mesh.polygons[face_index].loop_indices:
            edge_faces.setdefault(mesh.loops[loop_index].edge_index, []).append(face_index)
    adjacency = {face_index: set() for face_index in face_indices}
    for edge_index, connected_faces in edge_faces.items():
        if len(connected_faces) != 2:
            continue
        edge = mesh.edges[edge_index]
        first, second = connected_faces
        if not edge.use_seam and not _edge_is_sharp(edge) and _uv_connected(mesh, uv_layer, first, second, edge_index):
            adjacency[first].add(second)
            adjacency[second].add(first)
    islands = []
    remaining = set(selected)
    while remaining:
        seed = min(remaining)
        stack = [seed]
        island = set()
        while stack:
            face_index = stack.pop()
            if face_index in island:
                continue
            island.add(face_index)
            remaining.discard(face_index)
            stack.extend(sorted(adjacency[face_index] - island, reverse=True))
        islands.append(tuple(sorted(island)))
    return tuple(islands), edge_faces, adjacency


def _assignment_map(mesh):
    raw = mesh.get("ht_assignments", "{}")
    try:
        value = json.loads(raw)
    except (TypeError, json.JSONDecodeError):
        value = {}
    return value if isinstance(value, dict) else {}


def _assignment_is_current(record, manifest):
    return (
        isinstance(record, dict)
        and record.get("compatibilityKey") == manifest["compatibilityKey"]
        and record.get("templateSnapshotHash") == manifest["templateSnapshotHash"]
        and record.get("algorithmVersion") == ASSIGNMENT_ALGORITHM_VERSION
    )


def _descriptor(obj, uv_layer, island, edge_faces, adjacency, assignments, manifest):
    mesh = obj.data
    points = []
    uv_area = 0.0
    world_area = 0.0
    for face_index in island:
        polygon = mesh.polygons[face_index]
        face_points = [tuple(uv_layer.data[index].uv) for index in polygon.loop_indices]
        points.extend(face_points)
        uv_area += abs(polygon_signed_area(face_points))
        world_area += _face_world_area(obj, polygon)
    uv_bounds = bounds(points)
    width, height = uv_bounds[2] - uv_bounds[0], uv_bounds[3] - uv_bounds[1]
    if width <= EPSILON or height <= EPSILON or uv_area <= EPSILON:
        raise ValueError(f"{obj.name}: zero-area UV island")
    boundary_vertices = {}
    boundary_degrees = {}
    for face_index in island:
        polygon = mesh.polygons[face_index]
        for loop_index in polygon.loop_indices:
            edge_index = mesh.loops[loop_index].edge_index
            connected = edge_faces.get(edge_index, ())
            is_boundary = len(connected) != 2 or not any(other in adjacency[face_index] for other in connected if other != face_index)
            if not is_boundary:
                continue
            edge_uvs = _edge_uvs(mesh, uv_layer, polygon, edge_index)
            for vertex_index, uv in edge_uvs.items():
                boundary_vertices[(vertex_index, round(uv[0], 7), round(uv[1], 7))] = uv
                boundary_degrees[(vertex_index, round(uv[0], 7), round(uv[1], 7))] = boundary_degrees.get((vertex_index, round(uv[0], 7), round(uv[1], 7)), 0) + 1
    boundary_closed = bool(boundary_degrees) and all(degree == 2 for degree in boundary_degrees.values())
    records = [assignments.get(str(face_index)) for face_index in island]
    existing = (
        records[0]
        if records
        and records[0]
        and all(record == records[0] for record in records)
        and _assignment_is_current(records[0], manifest)
        else None
    )
    return IslandDescriptor(
        uv_bounds=uv_bounds,
        uv_aspect=width / height,
        uv_area=uv_area,
        world_area=world_area,
        long_axis_orientation="U" if width >= height else "V",
        boundary_closed=boundary_closed,
        circularity=circularity_estimate(tuple(boundary_vertices.values()), uv_area),
        existing_slot_id=existing.get("slotId") if existing else None,
        existing_compatibility_key=existing.get("compatibilityKey") if existing else None,
    ), existing


def _snapshot(obj):
    mesh = obj.data
    active_uv = mesh.uv_layers.active
    if obj.mode == "EDIT":
        bm = bmesh.from_edit_mesh(mesh)
        bm.faces.ensure_lookup_table()
        bm.edges.ensure_lookup_table()
        bm.verts.ensure_lookup_table()
        face_selection = tuple(face.select for face in bm.faces)
        edge_selection = tuple(edge.select for edge in bm.edges)
        vertex_selection = tuple(vertex.select for vertex in bm.verts)
    else:
        face_selection = tuple(polygon.select for polygon in mesh.polygons)
        edge_selection = tuple(edge.select for edge in mesh.edges)
        vertex_selection = tuple(vertex.select for vertex in mesh.vertices)
    return {
        "uv_name": active_uv.name if active_uv else None,
        "uvs": [tuple(item.uv) for item in active_uv.data] if active_uv else None,
        "materials": tuple(mesh.materials),
        "material_indices": tuple(polygon.material_index for polygon in mesh.polygons),
        "assignments_exists": "ht_assignments" in mesh,
        "assignments": mesh.get("ht_assignments"),
        "face_selection": face_selection,
        "edge_selection": edge_selection,
        "vertex_selection": vertex_selection,
    }


def _restore_snapshot(obj, snapshot):
    mesh = obj.data
    if snapshot["uv_name"] is None:
        for layer in tuple(mesh.uv_layers):
            if layer.name == "HotTrimmerUV":
                mesh.uv_layers.remove(layer)
    else:
        layer = mesh.uv_layers.get(snapshot["uv_name"])
        if layer is not None:
            mesh.uv_layers.active = layer
            for item, coordinate in zip(layer.data, snapshot["uvs"]):
                item.uv = coordinate
    mesh.materials.clear()
    for material in snapshot["materials"]:
        mesh.materials.append(material)
    for polygon, material_index in zip(mesh.polygons, snapshot["material_indices"]):
        polygon.material_index = material_index
    if snapshot["assignments_exists"]:
        mesh["ht_assignments"] = snapshot["assignments"]
    elif "ht_assignments" in mesh:
        del mesh["ht_assignments"]
    for polygon, selected in zip(mesh.polygons, snapshot["face_selection"]):
        polygon.select = selected
    for edge, selected in zip(mesh.edges, snapshot["edge_selection"]):
        edge.select = selected
    for vertex, selected in zip(mesh.vertices, snapshot["vertex_selection"]):
        vertex.select = selected


def _restore_selection(obj, snapshot):
    mesh = obj.data
    for polygon, selected in zip(mesh.polygons, snapshot["face_selection"]):
        polygon.select = selected
    for edge, selected in zip(mesh.edges, snapshot["edge_selection"]):
        edge.select = selected
    for vertex, selected in zip(mesh.vertices, snapshot["vertex_selection"]):
        vertex.select = selected


def _restore_context(context, active, selected_objects, original_mode):
    if context.view_layer.objects.active and context.view_layer.objects.active.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for obj in context.selected_objects:
        obj.select_set(False)
    for obj in selected_objects:
        if obj.name in context.view_layer.objects:
            obj.select_set(True)
    if active is not None and active.name in context.view_layer.objects:
        context.view_layer.objects.active = active
        if original_mode == "EDIT":
            bpy.ops.object.mode_set(mode="EDIT")


def _prepare_edit_selection(context, obj, face_indices):
    if context.view_layer.objects.active and context.view_layer.objects.active.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for candidate in context.selected_objects:
        candidate.select_set(False)
    obj.select_set(True)
    context.view_layer.objects.active = obj
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="DESELECT")
    bm = bmesh.from_edit_mesh(obj.data)
    bm.faces.ensure_lookup_table()
    for face_index in face_indices:
        bm.faces[face_index].select_set(True)
    bmesh.update_edit_mesh(obj.data, loop_triangles=False, destructive=False)


def _unwrap_object(context, obj, face_indices):
    _prepare_edit_selection(context, obj, face_indices)
    bm = bmesh.from_edit_mesh(obj.data)
    bm.faces.ensure_lookup_table()
    bm.edges.ensure_lookup_table()
    selected = {bm.faces[index] for index in face_indices}
    original_seams = tuple(edge.seam for edge in bm.edges)
    try:
        for edge in bm.edges:
            selected_links = [face for face in edge.link_faces if face in selected]
            if not selected_links:
                continue
            selection_boundary = len(selected_links) != len(edge.link_faces)
            sharp_boundary = not edge.smooth
            # Face shading is not an island boundary.  Otherwise a flat-shaded
            # segmented arch is cut into one island per polygon.  Authored
            # seams and explicitly sharp edges remain authoritative.
            if selection_boundary or sharp_boundary:
                edge.seam = True
        bmesh.update_edit_mesh(obj.data, loop_triangles=False, destructive=False)
        result = bpy.ops.uv.unwrap(method="ANGLE_BASED", margin=0.001)
        if result != {"FINISHED"}:
            raise ValueError(f"{obj.name}: Blender unwrap could not solve the selected faces")
    finally:
        bm = bmesh.from_edit_mesh(obj.data)
        bm.edges.ensure_lookup_table()
        for edge, seam in zip(bm.edges, original_seams):
            edge.seam = seam
        bmesh.update_edit_mesh(obj.data, loop_triangles=False, destructive=False)
    bpy.ops.object.mode_set(mode="OBJECT")


def _smart_project_object(context, obj, face_indices):
    _prepare_edit_selection(context, obj, face_indices)
    result = bpy.ops.uv.smart_project()
    bpy.ops.object.mode_set(mode="OBJECT")
    if result != {"FINISHED"}:
        raise ValueError(f"{obj.name}: Blender Smart UV Project could not solve the selected faces")


def _rectify_single_quad(obj, uv_layer, island):
    """Straighten one quad island into a geometry-proportional UV rectangle."""
    if len(island) != 1:
        return False
    mesh = obj.data
    polygon = mesh.polygons[island[0]]
    if polygon.loop_total != 4:
        return False
    loop_indices = tuple(polygon.loop_indices)
    world_points = [obj.matrix_world @ mesh.vertices[mesh.loops[index].vertex_index].co for index in loop_indices]
    edge_lengths = [(world_points[(index + 1) % 4] - world_points[index]).length for index in range(4)]
    width = (edge_lengths[0] + edge_lengths[2]) * 0.5
    height = (edge_lengths[1] + edge_lengths[3]) * 0.5
    if width <= EPSILON or height <= EPSILON:
        raise ValueError(f"{obj.name}: degenerate quad island; repair geometry before hotspotting")
    for loop_index, coordinate in zip(loop_indices, ((0.0, 0.0), (width, 0.0), (width, height), (0.0, height))):
        uv_layer.data[loop_index].uv = coordinate
    return True


def _assign_material(mesh, material, face_indices):
    material_index = next((index for index, existing in enumerate(mesh.materials) if existing == material), -1)
    if material_index < 0:
        mesh.materials.append(material)
        material_index = len(mesh.materials) - 1
    for face_index in face_indices:
        mesh.polygons[face_index].material_index = material_index
    return material_index


def _enable_material_preview(context):
    """Make a successful 3D View invocation visibly show the assigned maps."""
    space = getattr(context, "space_data", None)
    if getattr(space, "type", None) != "VIEW_3D":
        return False
    shading = getattr(space, "shading", None)
    if shading is None:
        return False
    if shading.type not in {"MATERIAL", "RENDERED"}:
        shading.type = "MATERIAL"
    return True


class HOTTRIM_OT_import_package(Operator):
    bl_idname = "hottrimmer.import_package"
    bl_label = "Import Hot Trimmer Package"
    bl_options = {"REGISTER"}

    filepath: StringProperty(name="Hot Trimmer Manifest", subtype="FILE_PATH")
    filter_glob: StringProperty(default="*.hottrim.json", options={"HIDDEN"})
    filename_ext = ".hottrim.json"

    def invoke(self, context, event):
        # ``filepath`` is the Blender file browser's canonical selected-file
        # property. Custom path/directory properties render as unrelated side
        # fields and do not receive the highlighted file.
        connected_manifest = context.scene.get("ht_manifest_path", "")
        self.filepath = connected_manifest
        _result(context, "Select manifest.hottrim.json, then click Import Hot Trimmer Package")
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        try:
            manifest = load_manifest(self.filepath)
            material = create_or_update_material(manifest, bpy)
        except Exception as error:
            return _fail(self, context, f"Hot Trimmer import failed: {error}")
        context.scene["ht_manifest_path"] = str(manifest["_manifest_path"])
        context.scene["ht_material_name"] = material.name
        _result(context, f"Connected {manifest['materialName']} revision {manifest['materialRevision']} from {manifest['_package_path'].name}")
        self.report({"INFO"}, context.scene["ht_last_result"])
        return {"FINISHED"}


class HOTTRIM_OT_fit_selected(Operator):
    bl_idname = "hottrimmer.fit_selected"
    bl_label = "Auto Hotspot Selected"
    bl_options = {"REGISTER", "UNDO"}

    classification: EnumProperty(items=CLASSIFICATION_ITEMS, default="AUTO")
    slot_id: StringProperty(default="")
    process_all_faces: BoolProperty(default=False, options={"HIDDEN"})
    best_effort: BoolProperty(default=False, options={"HIDDEN"})
    live_update: BoolProperty(default=False, options={"HIDDEN"})

    def execute(self, context):
        manifest_path = context.scene.get("ht_manifest_path")
        if not manifest_path:
            return _fail(self, context, "Import a Hot Trimmer package first")
        try:
            manifest = load_manifest(manifest_path)
        except Exception as error:
            return _fail(self, context, f"Hot Trimmer package is invalid: {error}")
        material = _material_for_manifest(manifest)
        if material is None:
            return _fail(self, context, "Imported Hot Trimmer material is missing; import the package again")
        active = context.view_layer.objects.active
        selected_objects = tuple(context.selected_objects)
        original_mode = active.mode if active is not None else "OBJECT"
        if self.process_all_faces and active is not None and active.type == "MESH":
            if active.mode == "EDIT":
                bmesh.update_edit_mesh(active.data, loop_triangles=False, destructive=False)
                bpy.ops.object.mode_set(mode="OBJECT")
            targets = {active: tuple(range(len(active.data.polygons)))}
        else:
            targets = _capture_target_faces(context)
        targets = {
            obj: _valid_faces(obj.data, face_indices)
            for obj, face_indices in targets.items()
            if _valid_faces(obj.data, face_indices)
        }
        if not targets:
            return _fail(self, context, "Select at least one mesh face or mesh object")
        classification = self.classification
        if classification == "AUTO":
            classification = getattr(context.scene, "hottrimmer_classification", "AUTO")
        nondegenerate_targets = {}
        for obj, face_indices in targets.items():
            usable_faces = tuple(face_index for face_index in face_indices if _face_world_area(obj, obj.data.polygons[face_index]) > EPSILON)
            if len(usable_faces) != len(face_indices) and not self.best_effort:
                degenerate = next(face_index for face_index in face_indices if face_index not in usable_faces)
                _restore_context(context, active, selected_objects, original_mode)
                return _fail(self, context, f"{obj.name}: degenerate face {degenerate}; repair geometry before hotspotting")
            if usable_faces:
                nondegenerate_targets[obj] = usable_faces
        targets = nondegenerate_targets
        if not targets:
            if self.best_effort and active is not None and active.type == "MESH":
                active.data["ht_assignments"] = "{}"
                _store_geometry_signature(active)
                _restore_context(context, active, selected_objects, original_mode)
                _result(context, f"Live hotspot skipped: {active.name} has no non-degenerate faces")
                return {"FINISHED"}
            _restore_context(context, active, selected_objects, original_mode)
            return _fail(self, context, "No non-degenerate mesh faces to hotspot")
        snapshots = {obj: _snapshot(obj) for obj in targets}
        variation_cycle = max(0, int(context.scene.get("ht_hotspot_cycle", 0)))
        try:
            if original_mode == "EDIT":
                bpy.ops.object.mode_set(mode="OBJECT")
            for obj, face_indices in targets.items():
                mesh = obj.data
                uv_layer = mesh.uv_layers.active
                if classification == "RADIAL" and not _has_usable_uvs(mesh, uv_layer, face_indices):
                    raise ValueError(f"{obj.name}: unsupported radial topology")
                if uv_layer is None:
                    uv_layer = mesh.uv_layers.new(name="HotTrimmerUV")
                preserve_auto_radial = False
                if classification == "AUTO" and _has_usable_uvs(mesh, uv_layer, face_indices):
                    current_islands, current_edge_faces, current_adjacency = _form_islands(mesh, uv_layer, face_indices)
                    current_assignments = _assignment_map(mesh)
                    current_descriptors = [
                        _descriptor(obj, uv_layer, island, current_edge_faces, current_adjacency, current_assignments, manifest)[0]
                        for island in current_islands
                    ]
                    preserve_auto_radial = bool(current_descriptors) and all(descriptor.strongly_radial for descriptor in current_descriptors)
                # Rebuild the selected islands from mesh geometry on every
                # click.  Starting from previously fitted hotspot UVs would
                # feed the old target rectangle back into measurement and lock
                # subsequent cycles to the same slot shape.
                if classification != "RADIAL" and not preserve_auto_radial:
                    _unwrap_object(context, obj, face_indices)
                    uv_layer = mesh.uv_layers.active
                    if not _has_usable_uvs(mesh, uv_layer, face_indices):
                        _smart_project_object(context, obj, face_indices)
                        uv_layer = mesh.uv_layers.active
                    if not _has_usable_uvs(mesh, uv_layer, face_indices):
                        raise ValueError(f"{obj.name}: automatic unwrap produced a zero-area island; inspect the selected faces for overlapping or invalid geometry")

            plans = []
            island_ordinal = 0
            for obj, face_indices in targets.items():
                mesh = obj.data
                uv_layer = mesh.uv_layers.active
                assignments = _assignment_map(mesh)
                islands, edge_faces, adjacency = _form_islands(mesh, uv_layer, face_indices)
                for island in islands:
                    rectified_quad = classification != "RADIAL" and _rectify_single_quad(obj, uv_layer, island)
                    descriptor, _existing = _descriptor(obj, uv_layer, island, edge_faces, adjacency, assignments, manifest)
                    points = [tuple(uv_layer.data[loop_index].uv) for face_index in island for loop_index in mesh.polygons[face_index].loop_indices]
                    match = choose_slot(
                        descriptor,
                        slots(manifest),
                        classification,
                        self.slot_id,
                        variation_index=variation_cycle + island_ordinal,
                        distribute=True,
                    )
                    island_ordinal += 1
                    fitted = transform_uvs(points, match, fill_rect=rectified_quad)
                    if not points_inside_slot(fitted, match.slot):
                        raise ValueError(f"{obj.name}: fitted UV escaped hotspot {match.slot.slot_id}")
                    record = {
                        "slotId": match.slot.slot_id,
                        "regionId": match.slot.region_id,
                        "rotation": match.rotation,
                        "mirror": match.mirror,
                        "classification": match.classification,
                        "islandRole": island_behavior_role(descriptor, match.classification),
                        "slotBehaviorRole": match.slot.behavior_role,
                        "compatibilityKey": manifest["compatibilityKey"],
                        "templateSnapshotHash": manifest["templateSnapshotHash"],
                        "algorithmVersion": ASSIGNMENT_ALGORITHM_VERSION,
                        "variationCycle": variation_cycle,
                        "rectifiedQuad": rectified_quad,
                    }
                    plans.append((obj, island, match, fitted, record))

            by_object = {}
            for obj, island, match, fitted, record in plans:
                mesh = obj.data
                uv_layer = mesh.uv_layers.active
                if fitted is not None:
                    cursor = 0
                    for face_index in island:
                        for loop_index in mesh.polygons[face_index].loop_indices:
                            uv_layer.data[loop_index].uv = fitted[cursor]
                            cursor += 1
                material_index = _assign_material(mesh, material, island)
                obj.active_material_index = material_index
                assignments = by_object.setdefault(obj, {} if self.process_all_faces else _assignment_map(mesh))
                for face_index in island:
                    assignments[str(face_index)] = record
            for obj, assignments in by_object.items():
                obj.data["ht_assignments"] = json.dumps(assignments, sort_keys=True, separators=(",", ":"))
                obj.data["ht_compatibility_key"] = manifest["compatibilityKey"]
            context.scene["ht_hotspot_cycle"] = variation_cycle + 1
            slot_names = sorted({plan[2].slot.slot_id for plan in plans})
            preview_note = "; Base Color preview active" if _enable_material_preview(context) else ""
            _result(context, f"Cycle {variation_cycle + 1}: hotspotted {len(plans)} island(s) across {len(slot_names)} manifest slot(s){preview_note}")
        except Exception as error:
            if context.view_layer.objects.active and context.view_layer.objects.active.mode != "OBJECT":
                bpy.ops.object.mode_set(mode="OBJECT")
            for obj, snapshot in snapshots.items():
                _restore_snapshot(obj, snapshot)
            _restore_context(context, active, selected_objects, original_mode)
            return _fail(self, context, str(error))
        for obj, snapshot in snapshots.items():
            _restore_selection(obj, snapshot)
        _restore_context(context, active, selected_objects, original_mode)
        for obj in targets:
            _store_geometry_signature(obj)
        self.report({"INFO"}, context.scene["ht_last_result"])
        return {"FINISHED"}


class HOTTRIM_PT_panel(Panel):
    bl_label = "Hot Trimmer"
    bl_idname = "HOTTRIM_PT_panel"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Hot Trimmer"

    def draw(self, context):
        layout = self.layout
        layout.label(text="Add-on 0.3.14")
        layout.operator(HOTTRIM_OT_import_package.bl_idname, text="Import Package", icon="FILE_FOLDER")
        path = context.scene.get("ht_manifest_path")
        if path:
            layout.label(text=f"Material: {context.scene.get('ht_material_name', 'Connected')}", icon="LINKED")
        else:
            layout.label(text="No package connected", icon="UNLINKED")
        layout.prop(context.scene, "hottrimmer_classification", text="Classification")
        layout.prop(context.scene, "hottrimmer_live_update", text="Live Mesh Updates")
        operator = layout.operator(HOTTRIM_OT_fit_selected.bl_idname, text="Auto Hotspot Selected", icon="UV")
        operator.classification = context.scene.hottrimmer_classification
        operator.slot_id = ""
        message = context.scene.get("ht_last_result", "")
        if message:
            layout.label(text=message, icon="INFO")
