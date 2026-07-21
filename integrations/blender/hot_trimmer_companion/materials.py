"""Idempotent Principled material construction for Hot Trimmer packages."""

from pathlib import Path


MATERIAL_METADATA = {
    "ht_project_id": "projectId",
    "ht_material_id": "materialId",
    "ht_template_id": "templateId",
    "ht_template_version": "templateVersion",
    "ht_compatibility_key": "compatibilityKey",
    "ht_template_snapshot_hash": "templateSnapshotHash",
    "ht_material_revision": "materialRevision",
}


def _canonical_role(role):
    return "".join(character for character in role.lower() if character.isalnum())


def _node(nodes, name, node_type):
    existing = nodes.get(name)
    if existing is not None and existing.bl_idname != node_type:
        nodes.remove(existing)
        existing = None
    if existing is None:
        existing = nodes.new(node_type)
        existing.name = name
    existing.label = name
    return existing


def _link(links, output_socket, input_socket):
    for existing in tuple(input_socket.links):
        links.remove(existing)
    links.new(output_socket, input_socket)


def _remove_named(nodes, names):
    for name in names:
        node = nodes.get(name)
        if node is not None:
            nodes.remove(node)


def _principled(nodes):
    node = nodes.get("HT Principled")
    if node is None:
        node = next((candidate for candidate in nodes if candidate.type == "BSDF_PRINCIPLED"), None)
    if node is None:
        node = nodes.new("ShaderNodeBsdfPrincipled")
    node.name = "HT Principled"
    node.label = "HT Principled"
    return node


def create_or_update_material(manifest, bpy):
    loaded_images = {}
    for key in sorted(manifest.get("maps", {})):
        record = manifest["maps"][key]
        role = _canonical_role(record["role"])
        if role not in ("basecolor", "roughness", "metallic", "normal", "height"):
            continue
        image_path = (Path(manifest["_package_path"]) / record["relativePath"]).resolve()
        image = bpy.data.images.load(str(image_path), check_existing=True)
        if not image.has_data:
            image.reload()
        loaded_images[key] = image

    material = next((candidate for candidate in bpy.data.materials if candidate.get("ht_material_id") == manifest["materialId"]), None)
    if material is None:
        material = bpy.data.materials.get(manifest["materialName"])
    if material is None:
        material = bpy.data.materials.new(manifest["materialName"])
    material.name = manifest["materialName"]
    material.use_nodes = True
    for property_name, manifest_name in MATERIAL_METADATA.items():
        material[property_name] = manifest[manifest_name]
    material["ht_manifest_path"] = str(manifest["_manifest_path"])

    nodes, links = material.node_tree.nodes, material.node_tree.links
    principled = _principled(nodes)
    output = next((node for node in nodes if node.type == "OUTPUT_MATERIAL"), None)
    if output is None:
        output = _node(nodes, "HT Material Output", "ShaderNodeOutputMaterial")
    _link(links, principled.outputs["BSDF"], output.inputs["Surface"])

    textures = {}
    for key in sorted(manifest.get("maps", {})):
        record = manifest["maps"][key]
        role = _canonical_role(record["role"])
        if role not in ("basecolor", "roughness", "metallic", "normal", "height"):
            continue
        image = loaded_images[key]
        image.colorspace_settings.name = "sRGB" if role == "basecolor" else "Non-Color"
        image["ht_material_id"] = manifest["materialId"]
        image["ht_map_role"] = role
        texture = _node(nodes, f"HT {role} Texture", "ShaderNodeTexImage")
        texture.image = image
        textures[role] = texture

    role_inputs = {"basecolor": "Base Color", "roughness": "Roughness", "metallic": "Metallic"}
    for role, input_name in role_inputs.items():
        texture = textures.get(role)
        if texture is not None:
            _link(links, texture.outputs["Color"], principled.inputs[input_name])
        else:
            _remove_named(nodes, (f"HT {role} Texture",))

    normal_output = None
    normal_texture = textures.get("normal")
    if normal_texture is not None:
        normal_map = _node(nodes, "HT Normal Map", "ShaderNodeNormalMap")
        if manifest["normalOrientation"].lower() == "directx":
            separate = _node(nodes, "HT Normal Separate", "ShaderNodeSeparateColor")
            invert_green = _node(nodes, "HT Normal Green Invert", "ShaderNodeMath")
            invert_green.operation = "SUBTRACT"
            invert_green.inputs[0].default_value = 1.0
            combine = _node(nodes, "HT Normal Combine", "ShaderNodeCombineColor")
            _link(links, normal_texture.outputs["Color"], separate.inputs["Color"])
            _link(links, separate.outputs["Red"], combine.inputs["Red"])
            _link(links, separate.outputs["Green"], invert_green.inputs[1])
            _link(links, invert_green.outputs[0], combine.inputs["Green"])
            _link(links, separate.outputs["Blue"], combine.inputs["Blue"])
            _link(links, combine.outputs["Color"], normal_map.inputs["Color"])
        else:
            _remove_named(nodes, ("HT Normal Separate", "HT Normal Green Invert", "HT Normal Combine"))
            _link(links, normal_texture.outputs["Color"], normal_map.inputs["Color"])
        normal_output = normal_map.outputs["Normal"]
    else:
        _remove_named(nodes, ("HT normal Texture", "HT Normal Map", "HT Normal Separate", "HT Normal Green Invert", "HT Normal Combine"))

    height_texture = textures.get("height")
    if height_texture is not None:
        bump = _node(nodes, "HT Bump", "ShaderNodeBump")
        _link(links, height_texture.outputs["Color"], bump.inputs["Height"])
        if normal_output is not None:
            _link(links, normal_output, bump.inputs["Normal"])
        _link(links, bump.outputs["Normal"], principled.inputs["Normal"])
    elif normal_output is not None:
        _remove_named(nodes, ("HT height Texture", "HT Bump"))
        _link(links, normal_output, principled.inputs["Normal"])
    else:
        _remove_named(nodes, ("HT height Texture", "HT Bump"))
        for existing in tuple(principled.inputs["Normal"].links):
            links.remove(existing)
    return material
