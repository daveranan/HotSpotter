"""Minimal Principled material creation for a Hot Trimmer package."""
from pathlib import Path

def create_or_update_material(manifest, bpy):
    material = bpy.data.materials.get(manifest["materialName"]) or bpy.data.materials.new(manifest["materialName"])
    material.use_nodes = True
    material["ht_project_id"] = manifest["projectId"]
    material["ht_material_id"] = manifest["materialId"]
    material["ht_template_id"] = manifest["templateId"]
    material["ht_template_snapshot_hash"] = manifest["templateSnapshotHash"]
    nodes, links = material.node_tree.nodes, material.node_tree.links
    principled = next(node for node in nodes if node.type == "BSDF_PRINCIPLED")
    for record in manifest.get("maps", {}).values():
        role = record["role"]
        image_path = Path(manifest["_package_path"]) / record["relativePath"]
        if not image_path.exists():
            continue
        image = bpy.data.images.load(str(image_path), check_existing=True)
        image.colorspace_settings.name = "sRGB" if role == "BaseColor" else "Non-Color"
        texture = nodes.new("ShaderNodeTexImage")
        texture.image = image
        if role == "BaseColor":
            links.new(texture.outputs["Color"], principled.inputs["Base Color"])
        elif role == "Roughness":
            links.new(texture.outputs["Color"], principled.inputs["Roughness"])
        elif role == "Metallic":
            links.new(texture.outputs["Color"], principled.inputs["Metallic"])
        elif role == "Normal":
            normal = nodes.new("ShaderNodeNormalMap")
            links.new(texture.outputs["Color"], normal.inputs["Color"])
            links.new(normal.outputs["Normal"], principled.inputs["Normal"])
    return material