import type { AssignmentProvenance, SourceChannel } from "@hot-trimmer/ipc-contracts";

export interface SourceFileAssignment {
  path: string;
  channel: SourceChannel;
  assignmentProvenance: AssignmentProvenance;
  confidenceMilli: number;
}

const channelOrder: readonly SourceChannel[] = [
  "base_color", "normal", "height", "roughness", "metallic", "ambient_occlusion",
  "specular", "opacity", "edge_mask", "material_id",
];

export function suggestedChannel(path: string): SourceChannel | null {
  const name = (path.split(/[\\/]/).at(-1) ?? path).replace(/\.[^.]+$/, "").toLowerCase();
  const tests: ReadonlyArray<[SourceChannel, RegExp]> = [
    ["ambient_occlusion", /(^|[_ .-])(ambient[_ .-]?occlusion|ao|occlusion)([_ .-]|$)/],
    ["material_id", /(^|[_ .-])(material[_ .-]?id|mat[_ .-]?id|id[_ .-]?map)([_ .-]|$)/],
    ["edge_mask", /(^|[_ .-])(edge[_ .-]?mask|edges?)([_ .-]|$)/],
    ["base_color", /(^|[_ .-])(base[_ .-]?colou?r|albedo|diffuse|diff|color|colour|d)([_ .-]|$)/],
    ["normal", /(^|[_ .-])(normal|norm|nrm|n)([_ .-]|$)/],
    ["height", /(^|[_ .-])(height|bump|displacement|disp|h)([_ .-]|$)/],
    ["roughness", /(^|[_ .-])(roughness|rough|r)([_ .-]|$)/],
    ["metallic", /(^|[_ .-])(metallic|metalness|metal|m)([_ .-]|$)/],
    ["specular", /(^|[_ .-])(specular|spec|s)([_ .-]|$)/],
    ["opacity", /(^|[_ .-])(opacity|alpha|transparency|transparent)([_ .-]|$)/],
  ];
  return tests.find(([, pattern]) => pattern.test(name))?.[0] ?? null;
}

export function assignSourceFiles(
  paths: string[],
  occupiedChannels: readonly SourceChannel[],
): SourceFileAssignment[] {
  const openChannels = channelOrder.filter((channel) => !occupiedChannels.includes(channel));
  const assigned = new Set<SourceChannel>();
  const remaining = [...paths];
  const result: SourceFileAssignment[] = [];
  if (openChannels.includes("base_color")) {
    const baseIndex = remaining.findIndex((path) => suggestedChannel(path) === "base_color");
    const unknownIndex = remaining.findIndex((path) => suggestedChannel(path) === null);
    const selectedIndex = baseIndex >= 0 ? baseIndex : unknownIndex;
    if (selectedIndex >= 0) {
      const [basePath] = remaining.splice(selectedIndex, 1);
      if (basePath) {
        result.push({
          path: basePath, channel: "base_color",
          assignmentProvenance: baseIndex >= 0 ? "filename_suggested" : "user_assigned",
          confidenceMilli: baseIndex >= 0 ? 700 : 500,
        });
        assigned.add("base_color");
      }
    }
  }
  for (let index = 0; index < remaining.length;) {
    const channel = suggestedChannel(remaining[index]!);
    if (channel && openChannels.includes(channel) && !assigned.has(channel)) {
      result.push({ path: remaining[index]!, channel, assignmentProvenance: "filename_suggested", confidenceMilli: 700 });
      assigned.add(channel); remaining.splice(index, 1);
    } else index += 1;
  }
  return result;
}
