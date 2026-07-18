import { spawnSync } from "node:child_process";

const suites = {
  "manual-layout-presets": "src/manual-layout-presets.test.ts",
};
const requested = process.argv.slice(2);
const files = requested.length
  ? requested.map((name) => suites[name] ?? name)
  : ["src/document-workbench.test.ts", "src/source-frame-partition.test.ts", "src/source-frame-preview-performance.test.ts", "src/manual-layout-presets.test.ts"];
const result = spawnSync(process.execPath, ["--test", ...files], { stdio: "inherit" });
process.exit(result.status ?? 1);
