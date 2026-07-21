"""Blender operators for importing and applying Hot Trimmer hotspots."""

import json
import math

import bmesh
import bpy
from bpy.props import EnumProperty, StringProperty
from bpy.types import Operator, Panel
from mathutils import Vector

from .fit import (
    EPSILON,
    IslandDescriptor,
    Match,
    bounds,
    choose_slot,
    circularity_estimate,
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


def _result(context, message):
    context.scene["ht_last_result"] = message


def _fail(operator, context, message):
    _result(context, message)
    operator.report({"ERROR"}, message)
    return {"CANCELLED"}


def _material_for_manifest(manifest):
    return next((material for material in bpy.data.materials if material.get("ht_material_id") == manifest["materialId"]), None)


def _capture_target_faces(context):
    active = context.view_layer.objects.active
    if active is not None and active.mode == "EDIT":
        targets = {}
        for obj in context.objects_in_mode:
            if obj.type != "MESH":
                continue
            bm = bmesh.from_edit_mesh(obj.data)
            bm.faces.ensure_lookup_table()
            selected = tuple(face.index for face in bm.faces if face.select)
            if selected:
                targets[obj] = selected
        return targets
    return {obj: tuple(range(len(obj.data.polygons))) for obj in context.selected_objects if obj.type == "MESH" and obj.data.polygons}


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


def _descriptor(obj, uv_layer, island, edge_faces, adjacency, assignments, compatibility_key):
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
    existing = records[0] if records and records[0] and all(record == records[0] for record in records) and records[0].get("compatibilityKey") == compatibility_key else None
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


def _unwrap_object(context, obj, face_indices):
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
    bpy.ops.uv.unwrap(method="ANGLE_BASED", margin=0.001)
    bpy.ops.object.mode_set(mode="OBJECT")


def _assign_material(mesh, material, face_indices):
    material_index = next((index for index, existing in enumerate(mesh.materials) if existing == material), -1)
    if material_index < 0:
        mesh.materials.append(material)
        material_index = len(mesh.materials) - 1
    for face_index in face_indices:
        mesh.polygons[face_index].material_index = material_index


class HOTTRIM_OT_import_package(Operator):
    bl_idname = "hottrimmer.import_package"
    bl_label = "Import Hot Trimmer Package"
    bl_options = {"REGISTER"}

    manifest_path: StringProperty(subtype="FILE_PATH")

    def invoke(self, context, event):
        if not self.manifest_path:
            self.manifest_path = context.scene.get("ht_manifest_path", "")
            context.window_manager.fileselect_add(self)
            return {"RUNNING_MODAL"}
        return self.execute(context)

    def execute(self, context):
        try:
            manifest = load_manifest(self.manifest_path)
            material = create_or_update_material(manifest, bpy)
        except Exception as error:
            return _fail(self, context, f"Hot Trimmer import failed: {error}")
        context.scene["ht_manifest_path"] = str(manifest["_manifest_path"])
        context.scene["ht_material_name"] = material.name
        _result(context, f"Connected {manifest['materialName']} revision {manifest['materialRevision']}")
        self.report({"INFO"}, context.scene["ht_last_result"])
        return {"FINISHED"}


class HOTTRIM_OT_fit_selected(Operator):
    bl_idname = "hottrimmer.fit_selected"
    bl_label = "Auto Hotspot Selected"
    bl_options = {"REGISTER", "UNDO"}

    classification: EnumProperty(items=CLASSIFICATION_ITEMS, default="AUTO")
    slot_id: StringProperty(default="")

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
        targets = _capture_target_faces(context)
        if not targets:
            return _fail(self, context, "Select at least one mesh face or mesh object")
        classification = self.classification
        if classification == "AUTO":
            classification = getattr(context.scene, "hottrimmer_classification", "AUTO")
        for obj, face_indices in targets.items():
            for face_index in face_indices:
                if _face_world_area(obj, obj.data.polygons[face_index]) <= EPSILON:
                    return _fail(self, context, f"{obj.name}: degenerate face {face_index}; repair geometry before hotspotting")

        active = context.view_layer.objects.active
        selected_objects = tuple(context.selected_objects)
        original_mode = active.mode if active is not None else "OBJECT"
        snapshots = {obj: _snapshot(obj) for obj in targets}
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
                if not _has_usable_uvs(mesh, uv_layer, face_indices):
                    _unwrap_object(context, obj, face_indices)
                    uv_layer = mesh.uv_layers.active
                    if not _has_usable_uvs(mesh, uv_layer, face_indices):
                        raise ValueError(f"{obj.name}: Blender unwrap produced a zero-area island")

            plans = []
            for obj, face_indices in targets.items():
                mesh = obj.data
                uv_layer = mesh.uv_layers.active
                assignments = _assignment_map(mesh)
                islands, edge_faces, adjacency = _form_islands(mesh, uv_layer, face_indices)
                for island in islands:
                    descriptor, existing = _descriptor(obj, uv_layer, island, edge_faces, adjacency, assignments, manifest["compatibilityKey"])
                    points = [tuple(uv_layer.data[loop_index].uv) for face_index in island for loop_index in mesh.polygons[face_index].loop_indices]
                    if existing:
                        assigned_slot = next((slot for slot in slots(manifest) if slot.slot_id == existing.get("slotId") and slot.enabled), None)
                        existing_classification = existing.get("classification")
                        requested_classification = classify = "radial" if classification == "RADIAL" else "rectangular" if classification == "RECTANGULAR" else ("radial" if descriptor.strongly_radial else "rectangular")
                        if assigned_slot and assigned_slot.uv_fit_kind == requested_classification and points_inside_slot(points, assigned_slot):
                            match = Match(assigned_slot, int(existing.get("rotation", 0)), bool(existing.get("mirror", False)), existing_classification or requested_classification)
                            plans.append((obj, island, match, None, existing))
                            continue
                    match = choose_slot(descriptor, slots(manifest), classification, self.slot_id)
                    fitted = transform_uvs(points, match)
                    if not points_inside_slot(fitted, match.slot):
                        raise ValueError(f"{obj.name}: fitted UV escaped hotspot {match.slot.slot_id}")
                    record = {
                        "slotId": match.slot.slot_id,
                        "regionId": match.slot.region_id,
                        "rotation": match.rotation,
                        "mirror": match.mirror,
                        "classification": match.classification,
                        "compatibilityKey": manifest["compatibilityKey"],
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
                _assign_material(mesh, material, island)
                assignments = by_object.setdefault(obj, _assignment_map(mesh))
                for face_index in island:
                    assignments[str(face_index)] = record
            for obj, assignments in by_object.items():
                obj.data["ht_assignments"] = json.dumps(assignments, sort_keys=True, separators=(",", ":"))
                obj.data["ht_compatibility_key"] = manifest["compatibilityKey"]
            slot_names = sorted({plan[2].slot.slot_id for plan in plans})
            _result(context, f"Hotspotted {len(plans)} island(s): {', '.join(slot_names)}")
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
        layout.operator(HOTTRIM_OT_import_package.bl_idname, text="Import Package", icon="FILE_FOLDER")
        path = context.scene.get("ht_manifest_path")
        if path:
            layout.label(text=f"Material: {context.scene.get('ht_material_name', 'Connected')}", icon="LINKED")
        else:
            layout.label(text="No package connected", icon="UNLINKED")
        layout.prop(context.scene, "hottrimmer_classification", text="Classification")
        operator = layout.operator(HOTTRIM_OT_fit_selected.bl_idname, text="Auto Hotspot Selected", icon="UV")
        operator.classification = context.scene.hottrimmer_classification
        message = context.scene.get("ht_last_result", "")
        if message:
            layout.label(text=message, icon="INFO")
