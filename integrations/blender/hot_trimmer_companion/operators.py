import bpy
from bpy.props import EnumProperty, StringProperty
from bpy.types import Operator
from .fit import fit_values
from .manifest import load_manifest, selected_fit_kind, slots
from .materials import create_or_update_material

class HOTTRIM_OT_import_package(Operator):
    bl_idname = "hottrimmer.import_package"
    bl_label = "Import Hot Trimmer Package"
    manifest_path: StringProperty(subtype="FILE_PATH")
    def execute(self, context):
        manifest = load_manifest(self.manifest_path)
        create_or_update_material(manifest, bpy)
        context.scene["ht_manifest_path"] = self.manifest_path
        return {"FINISHED"}

class HOTTRIM_OT_fit_selected(Operator):
    bl_idname = "hottrimmer.fit_selected"
    bl_label = "Fit Selected"
    classification: EnumProperty(items=(("AUTO", "Auto", "Use manifest slot semantics"), ("RECTANGULAR", "Rectangular", "Force rectangular fit"), ("RADIAL", "Radial", "Force radial fit")), default="AUTO")
    slot_id: StringProperty()
    def execute(self, context):
        manifest = load_manifest(context.scene["ht_manifest_path"])
        slot = next(candidate for candidate in slots(manifest) if candidate.slot_id == self.slot_id)
        kind = selected_fit_kind(slot, self.classification)
        context.scene["ht_last_fit_kind"] = kind
        context.scene["ht_last_fit_values"] = str(fit_values(slot, self.classification))
        return {"FINISHED"}