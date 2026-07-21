"""Real headless-Blender acceptance fixture for Prompt 20B."""

import base64
import json
import math
from pathlib import Path
import shutil
import struct
import sys
import tempfile

import bmesh
import bpy


ROOT = Path(__file__).resolve().parents[4]
INTEGRATIONS = ROOT / "integrations" / "blender"
FIXTURE = Path(__file__).parent / "fixtures" / "behavioral.hottrim.json"
sys.path.insert(0, str(INTEGRATIONS))

import hot_trimmer_companion


PNG_1X1 = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Z3pAAAAAASUVORK5CYII=")
ROLE_FILES = {
    "baseColor": ("Base Color", "base-color.png", "sRGB"),
    "roughness": ("Roughness", "roughness.png", "Non-Color"),
    "metallic": ("Metallic", "metallic.png", "Non-Color"),
    "normal": ("Normal", "normal.png", "Non-Color"),
    "height": ("Height", "height.png", "Non-Color"),
}


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def expect_operator_error(operation, expected_text):
    try:
        result = operation()
    except RuntimeError as error:
        require(expected_text in str(error), f"unexpected Blender operator error: {error}")
        return
    require(result == {"CANCELLED"}, f"operator did not reject input: {result}")


def uv_bytes(mesh):
    layer = mesh.uv_layers.active
    return b"".join(struct.pack("<ff", item.uv.x, item.uv.y) for item in layer.data)


def face_uvs(mesh, face_index):
    layer = mesh.uv_layers.active
    return [tuple(layer.data[index].uv) for index in mesh.polygons[face_index].loop_indices]


def uv_aspect(points):
    width = max(point[0] for point in points) - min(point[0] for point in points)
    height = max(point[1] for point in points) - min(point[1] for point in points)
    return width / height


def polygon_uv_area(points):
    return abs(0.5 * sum(points[index][0] * points[(index + 1) % len(points)][1] - points[(index + 1) % len(points)][0] * points[index][1] for index in range(len(points))))


def make_package(directory):
    data = json.loads(FIXTURE.read_text(encoding="utf-8"))
    data["maps"] = {}
    for key, (role, filename, color_space) in ROLE_FILES.items():
        (directory / filename).write_bytes(PNG_1X1)
        data["maps"][key] = {
            "role": role,
            "relativePath": filename,
            "dimensions": [1, 1],
            "bitDepth": 8,
            "colorSpace": color_space,
            "checksum": f"fixture-{key}",
        }
    path = directory / "manifest.hottrim.json"
    path.write_text(json.dumps(data, sort_keys=True), encoding="utf-8")
    return path, data


def assign_quad_uv(mesh, face_index, coordinates):
    layer = mesh.uv_layers.active
    for loop_index, coordinate in zip(mesh.polygons[face_index].loop_indices, coordinates):
        layer.data[loop_index].uv = coordinate


def rectangular_object():
    vertices = [
        (0, 0, 0), (2, 0, 0), (2, 1, 0), (0, 1, 0),
        (3, 0, 0), (4, 0, 0), (4, 2, 0), (3, 2, 0),
        (5, 0, 0), (6, 0, 0), (6, 1, 0), (5, 1, 0),
    ]
    mesh = bpy.data.meshes.new("HT Rectangular Fixture Mesh")
    mesh.from_pydata(vertices, [], ((0, 1, 2, 3), (4, 5, 6, 7), (8, 9, 10, 11)))
    mesh.uv_layers.new(name="FixtureUV")
    assign_quad_uv(mesh, 0, ((0, 0), (2, 0), (2, 1), (0, 1)))
    assign_quad_uv(mesh, 1, ((3, 0), (4, 0), (4, 2), (3, 2)))
    assign_quad_uv(mesh, 2, ((7, 7), (8, 7), (8, 8), (7, 8)))
    obj = bpy.data.objects.new("HT Rectangular Fixture", mesh)
    bpy.context.collection.objects.link(obj)
    unrelated = bpy.data.materials.new("Unrelated Fixture Material")
    mesh.materials.append(unrelated)
    for polygon in mesh.polygons:
        polygon.material_index = 0
        polygon.select = polygon.index < 2
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    return obj


def radial_object(segments=16):
    vertices = [(0, 0, 0)] + [(math.cos(2 * math.pi * index / segments), math.sin(2 * math.pi * index / segments), 0) for index in range(segments)]
    faces = [(0, index + 1, (index + 1) % segments + 1) for index in range(segments)]
    mesh = bpy.data.meshes.new("HT Radial Fixture Mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.uv_layers.new(name="FixtureRadialUV")
    layer = mesh.uv_layers.active
    for polygon in mesh.polygons:
        for loop_index in polygon.loop_indices:
            vertex = mesh.loops[loop_index].vertex_index
            coordinate = vertices[vertex]
            layer.data[loop_index].uv = (0.5 + coordinate[0] * 0.5, 0.5 + coordinate[1] * 0.5)
    obj = bpy.data.objects.new("HT Radial Fixture", mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def slot_rects(data):
    return {slot["slotId"]: slot["normalizedHotspotRect"] for slot in data["slots"]}


def assert_face_inside(mesh, face_index, rect, tolerance=1.0e-6):
    for u, v in face_uvs(mesh, face_index):
        require(rect["x"] - tolerance <= u <= rect["x"] + rect["width"] + tolerance, f"face {face_index} U escaped selected hotspot")
        require(rect["y"] - tolerance <= v <= rect["y"] + rect["height"] + tolerance, f"face {face_index} V escaped selected hotspot")


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    hot_trimmer_companion.register()
    import_properties = bpy.ops.hottrimmer.import_package.get_rna_type().properties
    require("filepath" in import_properties and import_properties["filepath"].subtype == "FILE_PATH", "import dialog is not bound to Blender's standard filepath")
    require("manifest_path" not in import_properties and "directory" not in import_properties, "import dialog exposes a stale custom path field")
    with tempfile.TemporaryDirectory(prefix="hot-trimmer-20b-") as temp_name:
        temp = Path(temp_name)
        manifest_path, manifest_data = make_package(temp)
        result = bpy.ops.hottrimmer.import_package(filepath=str(temp))
        require(result == {"FINISHED"}, "real package-directory import operator failed")
        material = bpy.data.materials["Hot Trimmer Fixture"]
        require(material.get("ht_compatibility_key") == "fixture-v1", "material package metadata missing")
        require(material.node_tree.nodes.active is not None and material.node_tree.nodes.active.name == "HT basecolor Texture", "Base Color texture is not the active preview image")

        rect = rectangular_object()
        mesh = rect.data
        unselected_uv_before = tuple(face_uvs(mesh, 2))
        unselected_material_before = mesh.polygons[2].material_index
        vertices_before = tuple(tuple(vertex.co) for vertex in mesh.vertices)
        seams_before = tuple(edge.use_seam for edge in mesh.edges)
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.context.tool_settings.mesh_select_mode = (False, False, True)
        bm = bmesh.from_edit_mesh(mesh)
        bm.faces.ensure_lookup_table()
        for face in bm.faces:
            face.select_set(face.index < 2)
        bmesh.update_edit_mesh(mesh, loop_triangles=False, destructive=False)
        selected_faces_before = tuple(face.select for face in bm.faces)
        require(rect.mode == "EDIT", "fixture did not enter edit mode")
        result = bpy.ops.hottrimmer.fit_selected(classification="AUTO")
        require(result == {"FINISHED"}, "actual edit-mode hotspot operator failed")
        require(rect.mode == "EDIT", "operator did not restore original edit mode")
        bm = bmesh.from_edit_mesh(mesh)
        bm.faces.ensure_lookup_table()
        require(tuple(face.select for face in bm.faces) == selected_faces_before, "face selection was not preserved")
        bpy.ops.object.mode_set(mode="OBJECT")
        assignments = json.loads(mesh["ht_assignments"])
        require(assignments["0"]["slotId"] == "rect_wide", "wide island did not choose expected manifest slot")
        require(assignments["1"]["slotId"] == "rect_tall", "tall island did not choose expected manifest slot")
        material_index = next(index for index, candidate in enumerate(mesh.materials) if candidate == material)
        require(mesh.polygons[0].material_index == material_index and mesh.polygons[1].material_index == material_index, "Hot Trimmer material was not assigned")
        require(rect.active_material == material, "Hot Trimmer material slot was not made active for preview")
        require(mesh.polygons[2].material_index == unselected_material_before, "unselected face material changed")
        require(tuple(face_uvs(mesh, 2)) == unselected_uv_before, "unselected face UV changed")
        require(tuple(tuple(vertex.co) for vertex in mesh.vertices) == vertices_before, "mesh positions changed")
        require(tuple(edge.use_seam for edge in mesh.edges) == seams_before, "mesh seams changed")
        rects = slot_rects(manifest_data)
        assert_face_inside(mesh, 0, rects[assignments["0"]["slotId"]])
        assert_face_inside(mesh, 1, rects[assignments["1"]["slotId"]])
        require(abs(uv_aspect(face_uvs(mesh, 0)) - 2.0) < 1.0e-5, "wide island proportion was not preserved")
        require(abs(uv_aspect(face_uvs(mesh, 1)) - 0.5) < 1.0e-5, "tall island proportion was not preserved")
        first_uv_bytes = uv_bytes(mesh)
        first_assignments = mesh["ht_assignments"]
        bpy.ops.object.mode_set(mode="EDIT")
        bm = bmesh.from_edit_mesh(mesh)
        bm.faces.ensure_lookup_table()
        for face in bm.faces:
            face.select_set(face.index < 2)
        bmesh.update_edit_mesh(mesh, loop_triangles=False, destructive=False)
        result = bpy.ops.hottrimmer.fit_selected(classification="AUTO")
        require(result == {"FINISHED"}, "second identical hotspot run failed")
        require(rect.mode == "EDIT", "second run did not preserve edit mode")
        bpy.ops.object.mode_set(mode="OBJECT")
        require(uv_bytes(mesh) == first_uv_bytes, "second identical run was not byte-stable")
        require(mesh["ht_assignments"] == first_assignments, "second identical assignment was not stable")

        for obj in bpy.context.selected_objects:
            obj.select_set(False)
        bpy.ops.mesh.primitive_cube_add(calc_uvs=False, location=(10.0, 0.0, 0.0))
        closed_cube = bpy.context.object
        closed_cube.name = "HT Closed No-UV Fixture"
        closed_mesh = closed_cube.data
        require(len(closed_mesh.uv_layers) == 0, "closed fixture unexpectedly began with UVs")
        closed_seams_before = tuple(edge.use_seam for edge in closed_mesh.edges)
        closed_positions_before = tuple(tuple(vertex.co) for vertex in closed_mesh.vertices)
        result = bpy.ops.hottrimmer.fit_selected(classification="AUTO")
        require(result == {"FINISHED"}, "one-click closed-mesh automatic unwrap and hotspot failed")
        require(closed_mesh.uv_layers.active is not None, "automatic unwrap did not create a UV map")
        require(all(polygon_uv_area(face_uvs(closed_mesh, polygon.index)) > 1.0e-9 for polygon in closed_mesh.polygons), "automatic unwrap left a zero-area face")
        require(len(json.loads(closed_mesh["ht_assignments"])) == len(closed_mesh.polygons), "automatic unwrap did not assign every closed-mesh face")
        require(closed_cube.active_material == material, "closed-mesh hotspot did not activate the Hot Trimmer material")
        require(tuple(edge.use_seam for edge in closed_mesh.edges) == closed_seams_before, "temporary automatic unwrap seams were not restored")
        require(tuple(tuple(vertex.co) for vertex in closed_mesh.vertices) == closed_positions_before, "automatic unwrap changed closed-mesh positions")

        for obj in bpy.context.selected_objects:
            obj.select_set(False)
        radial = radial_object()
        radial.select_set(True)
        bpy.context.view_layer.objects.active = radial
        radial_before = uv_bytes(radial.data)
        result = bpy.ops.hottrimmer.fit_selected(classification="AUTO")
        require(result == {"FINISHED"}, "actual radial hotspot operator failed")
        radial_assignments = json.loads(radial.data["ht_assignments"])
        require({record["slotId"] for record in radial_assignments.values()} == {"radial_disc"}, "radial disc selected a non-radial slot")
        radial_points = [tuple(item.uv) for item in radial.data.uv_layers.active.data]
        radial_bounds = (max(point[0] for point in radial_points) - min(point[0] for point in radial_points), max(point[1] for point in radial_points) - min(point[1] for point in radial_points))
        require(abs(radial_bounds[0] / radial_bounds[1] - 1.0) < 1.0e-5, "radial circular proportion was not preserved")
        for face_index in range(len(radial.data.polygons)):
            assert_face_inside(radial.data, face_index, rects["radial_disc"])
        require(uv_bytes(radial.data) != radial_before, "radial operator did not author final UVs")

        saved_rect_uvs = uv_bytes(mesh)
        saved_radial_uvs = uv_bytes(radial.data)
        blend_path = temp / "prompt-20b.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend_path))
        bpy.ops.wm.open_mainfile(filepath=str(blend_path))
        mesh = bpy.data.meshes["HT Rectangular Fixture Mesh"]
        radial_mesh = bpy.data.meshes["HT Radial Fixture Mesh"]
        require(uv_bytes(mesh) == saved_rect_uvs and uv_bytes(radial_mesh) == saved_radial_uvs, "UVs did not survive save/reopen")
        require(json.loads(mesh["ht_assignments"])["0"]["slotId"] == "rect_wide", "assignment metadata did not survive save/reopen")
        require(json.loads(radial_mesh["ht_assignments"])["0"]["classification"] == "radial", "radial classification did not survive save/reopen")

        material = bpy.data.materials["Hot Trimmer Fixture"]
        node_names_before = tuple(sorted(node.name for node in material.node_tree.nodes))
        images_before = tuple(sorted(image.filepath for image in bpy.data.images if image.get("ht_material_id") == "material-fixture"))
        result = bpy.ops.hottrimmer.import_package(filepath=str(manifest_path))
        require(result == {"FINISHED"}, "package reimport failed")
        material = bpy.data.materials["Hot Trimmer Fixture"]
        require(tuple(sorted(node.name for node in material.node_tree.nodes)) == node_names_before, "package reimport duplicated or changed material nodes")
        require(tuple(sorted(image.filepath for image in bpy.data.images if image.get("ht_material_id") == "material-fixture")) == images_before, "package reimport duplicated images")
        for name in ("HT basecolor Texture", "HT roughness Texture", "HT metallic Texture", "HT normal Texture", "HT height Texture", "HT Normal Map", "HT Bump"):
            require(sum(node.name == name for node in material.node_tree.nodes) == 1, f"named material node is not unique: {name}")
        require(material.node_tree.nodes.get("HT Normal Green Invert") is not None, "DirectX normal convention was not represented")

        malformed = json.loads(manifest_path.read_text(encoding="utf-8"))
        malformed["slots"][0]["normalizedHotspotRect"]["x"] = 0.9
        malformed_path = temp / "malformed.hottrim.json"
        malformed_path.write_text(json.dumps(malformed), encoding="utf-8")
        material_count = len(bpy.data.materials)
        material_nodes = tuple(sorted(node.name for node in material.node_tree.nodes))
        connected_path = bpy.context.scene["ht_manifest_path"]
        expect_operator_error(lambda: bpy.ops.hottrimmer.import_package(filepath=str(malformed_path)), "normalizedHotspotRect")
        require(len(bpy.data.materials) == material_count and tuple(sorted(node.name for node in material.node_tree.nodes)) == material_nodes, "malformed import partially mutated materials")
        require(bpy.context.scene["ht_manifest_path"] == connected_path, "malformed import partially mutated scene connection state")

        unsupported = bpy.data.objects.new("Unsupported Radial Fixture", mesh.copy())
        bpy.context.collection.objects.link(unsupported)
        for obj in bpy.context.selected_objects:
            obj.select_set(False)
        unsupported.select_set(True)
        bpy.context.view_layer.objects.active = unsupported
        unsupported_uvs = uv_bytes(unsupported.data)
        unsupported_materials = tuple(unsupported.data.materials)
        unsupported_assignments = unsupported.data.get("ht_assignments")
        expect_operator_error(lambda: bpy.ops.hottrimmer.fit_selected(classification="RADIAL"), "unsupported radial topology")
        require(uv_bytes(unsupported.data) == unsupported_uvs, "unsupported radial failure partially mutated UVs")
        require(tuple(unsupported.data.materials) == unsupported_materials, "unsupported radial failure partially mutated materials")
        require(unsupported.data.get("ht_assignments") == unsupported_assignments, "unsupported radial failure partially mutated assignments")

    hot_trimmer_companion.unregister()
    print("Prompt 20B headless Blender behavioral fixture: PASS")


if __name__ == "__main__":
    run()
