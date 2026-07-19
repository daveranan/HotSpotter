import { spawnSync } from "node:child_process";

const suites = {
  "gpu-tiled-preview": "src/gpu-tiled-preview.test.ts",
  "manual-layout-presets": "src/manual-layout-presets.test.ts",
  "multi-source-patch-assignment": "src/multi-source-patch-assignment.test.ts",
};
const requested = process.argv.slice(2);
const files = requested.length
  ? requested.map((name) => suites[name] ?? name)
  : ["src/document-workbench.test.ts", "src/source-frame-partition.test.ts", "src/source-frame-preview-performance.test.ts", "src/manual-layout-presets.test.ts"];
const result = spawnSync(process.execPath, ["--test", ...files], { stdio: "inherit" });
if ((result.status ?? 1) !== 0) process.exit(result.status ?? 1);
if (requested.includes("multi-source-patch-assignment")) {
  const native = spawnSync("cargo", ["test", "-p", "hot-trimmer-sheet-compiler", "multi_source_patch_assignment"], { stdio: "inherit", cwd: new URL("../..", import.meta.url) });
  process.exit(native.status ?? 1);
}
process.exit(0);
