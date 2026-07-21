import type { CompiledMapView, ContentReference, Patch, SourceChannel } from "@hot-trimmer/ipc-contracts";

export function compiledMapViewForSourceChannel(channel: SourceChannel): CompiledMapView | null {
  switch (channel) {
    case "base_color": return "baseColor";
    case "normal": return "normal";
    case "height": return "height";
    case "roughness": return "roughness";
    case "metallic": return "metallic";
    case "ambient_occlusion": return "ambientOcclusion";
    case "material_id": return "materialId";
    case "specular":
    case "opacity":
    case "edge_mask":
      return null;
  }
}

export function sourceChannelForCompiledMapView(view: CompiledMapView): SourceChannel | null {
  switch (view) {
    case "baseColor": return "base_color";
    case "normal": return "normal";
    case "height": return "height";
    case "roughness": return "roughness";
    case "metallic": return "metallic";
    case "ambientOcclusion": return "ambient_occlusion";
    case "materialId": return "material_id";
    case "regionId": return null;
    case "edgeMask": return null;
  }
}

export function sourceSetIdForRegion(options: {
  content: ContentReference | null | undefined;
  primarySourceSetId: string | null | undefined;
  patches: readonly Pick<Patch, "id" | "sourceId">[];
  sourceSets: readonly { id: string; sourceIds: readonly string[] }[];
}): string | null {
  const { content } = options;
  if (!content) return null;
  if (content.type === "material_source") return content.id;
  if (content.type === "inherit_primary_material") return options.primarySourceSetId ?? null;
  if (content.type !== "patch") return null;
  const sourceId = options.patches.find((patch) => patch.id === content.id)?.sourceId;
  return sourceId ? options.sourceSets.find((sourceSet) => sourceSet.sourceIds.includes(sourceId))?.id ?? null : null;
}

export function canInteractWithPatch(pointEditPatchId: string | null, patchId: string): boolean {
  return pointEditPatchId === null || pointEditPatchId === patchId;
}
